use crate::Result;
use crate::config::MAX_TEXT_BYTES;
use crate::hyprland::Hyprland;
use crate::{paste, shell_quote};
use serde::{Deserialize, Serialize};
use std::env;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use wl_clipboard_rs::copy::{MimeType, Options, Source};

const MAX_REQUEST: u64 = (MAX_TEXT_BYTES * 6 + 128) as u64;

#[derive(Serialize, Deserialize)]
struct Selection {
    text: String,
}

#[derive(Serialize, Deserialize)]
struct Reply {
    error: Option<String>,
}

pub fn send(hyprland: &Hyprland, executable: &Path, text: &str) -> Result<()> {
    let runtime = env::var_os("XDG_RUNTIME_DIR").ok_or("A running desktop session is required")?;
    let directory = tempfile::Builder::new()
        .prefix("text-picker-")
        .tempdir_in(runtime)?;
    let path = directory.path().join("handoff.sock");
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let command = format!(
        "exec {} --handoff {}",
        shell_quote(executable.as_os_str()),
        shell_quote(path.as_os_str())
    );
    hyprland.dispatch(&command)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err("The clipboard helper did not start".into());
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&serde_json::to_vec(&Selection {
        text: text.to_owned(),
    })?)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.take(8192).read_to_end(&mut response)?;
    let reply: Reply = serde_json::from_slice(&response)?;
    if let Some(error) = reply.error {
        return Err(error.into());
    }
    Ok(())
}

pub fn serve(path: &Path) -> Result<()> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut request = Vec::new();
    (&mut stream)
        .take(MAX_REQUEST + 1)
        .read_to_end(&mut request)?;
    if request.len() as u64 > MAX_REQUEST {
        return Err("Clipboard request is too large".into());
    }
    let selection: Selection = serde_json::from_slice(&request)?;
    if selection.text.is_empty()
        || selection.text.len() > MAX_TEXT_BYTES
        || selection.text.contains('\0')
    {
        return Err("Invalid clipboard text".into());
    }
    let (ready, waiting) = mpsc::channel();
    let clipboard = thread::spawn(move || {
        let mut options = Options::new();
        options.foreground(true);
        match options.prepare_copy(
            Source::Bytes(selection.text.into_bytes().into()),
            MimeType::Text,
        ) {
            Ok(prepared) => {
                let _ = ready.send(None);
                prepared.serve().map_err(|error| error.to_string())
            }
            Err(error) => {
                let error = error.to_string();
                let _ = ready.send(Some(error.clone()));
                Err(error)
            }
        }
    });
    let error = waiting.recv_timeout(Duration::from_secs(3))?;
    stream.write_all(&serde_json::to_vec(&Reply {
        error: error.clone(),
    })?)?;
    stream.shutdown(Shutdown::Write)?;
    drop(stream);
    if let Some(error) = error {
        return Err(error.into());
    }
    if let Err(error) = paste::after_picker() {
        crate::notify(&error.to_string());
    }
    // This process was launched by Hyprland, outside the picker's terminal.
    // Keep serving clipboard requests until another application takes ownership.
    clipboard
        .join()
        .map_err(|_| "The clipboard worker stopped")?
        .map_err(Into::into)
}
