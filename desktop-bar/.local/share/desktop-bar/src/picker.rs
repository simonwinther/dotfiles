//! Bounded, private request/response transport for command-line selection menus.
use crate::modules::Update;
use serde::{Deserialize, Serialize};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::{
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Selection cancelled")
    }
}
impl std::error::Error for Cancelled {}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub title: String,
    pub query: String,
    pub entries: Vec<String>,
    pub input_only: bool,
}
impl Options {
    pub fn validate(&self) -> io::Result<()> {
        if self.title.len() > 256
            || self.query.len() > 4096
            || self.entries.len() > 20_000
            || self.entries.iter().any(|entry| entry.len() > 64 * 1024)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Selection menu exceeds its limits",
            ));
        }
        Ok(())
    }
    pub fn arguments(args: &[String]) -> io::Result<Self> {
        let mut options = Self {
            title: "Select".into(),
            ..Default::default()
        };
        let mut arguments = args.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--dmenu" => {}
                "--inputonly" => options.input_only = true,
                "--placeholder" | "-p" => {
                    options.title = arguments
                        .next()
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidInput, "Missing menu title")
                        })?
                        .clone()
                }
                "--search" | "--query" | "-s" => {
                    options.query = arguments
                        .next()
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidInput, "Missing search query")
                        })?
                        .clone()
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("Unknown selection option: {argument}"),
                    ));
                }
            }
        }
        if !options.input_only {
            let mut input = String::new();
            io::stdin()
                .take(MAX_BYTES as u64 + 1)
                .read_to_string(&mut input)?;
            if input.len() > MAX_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Selection input is too large",
                ));
            }
            options.entries = input.lines().map(str::to_owned).collect();
        }
        options.validate()?;
        Ok(options)
    }
}

pub struct Request {
    pub options: Options,
    response: mpsc::SyncSender<Option<String>>,
    finished: AtomicBool,
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("title", &self.options.title)
            .finish_non_exhaustive()
    }
}
impl Request {
    pub fn new(options: Options) -> (Arc<Self>, mpsc::Receiver<Option<String>>) {
        let (response, answers) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                options,
                response,
                finished: AtomicBool::new(false),
            }),
            answers,
        )
    }
    pub fn finish(&self, answer: Option<String>) {
        if !self.finished.swap(true, Ordering::Relaxed) {
            let _ = self.response.try_send(answer);
        }
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        self.finish(None);
    }
}

fn path() -> PathBuf {
    crate::theme::runtime().join("desktop/menus.sock")
}
fn write_message(stream: &mut UnixStream, value: &impl Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Selection message is too large",
        ));
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)
}
fn read_message<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Selection message is too large",
        ));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn client(options: &Options) -> io::Result<Option<String>> {
    let mut stream = UnixStream::connect(path())?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    write_message(&mut stream, options)?;
    read_message(&mut stream)
}

pub struct Server(PathBuf);
impl Server {
    pub fn start(sender: Sender<Update>) -> io::Result<Self> {
        let path = path();
        if let Ok(metadata) = path.symlink_metadata() {
            if !metadata.file_type().is_socket() || UnixStream::connect(&path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "A selection server already owns this path",
                ));
            }
            std::fs::remove_file(&path)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let active = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new()
            .name("bar-menu-ipc".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = match stream {
                        Ok(stream) => stream,
                        // Keep serving after transient failures such as fd exhaustion.
                        Err(error) => {
                            if error.kind() != io::ErrorKind::Interrupted {
                                eprintln!("desktop-bar: selection socket: {error}");
                                std::thread::sleep(Duration::from_millis(250));
                            }
                            continue;
                        }
                    };
                    if active.fetch_add(1, Ordering::Relaxed) >= 4 {
                        active.fetch_sub(1, Ordering::Relaxed);
                        continue;
                    }
                    let active = active.clone();
                    let sender = sender.clone();
                    std::thread::spawn(move || {
                        let mut handle = || -> io::Result<()> {
                            stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                            stream.set_write_timeout(Some(Duration::from_secs(3)))?;
                            let options: Options = read_message(&mut stream)?;
                            options.validate()?;
                            let (request, answers) = Request::new(options);
                            sender.send(Update::Picker(request)).map_err(|_| {
                                io::Error::new(io::ErrorKind::BrokenPipe, "Desktop closed")
                            })?;
                            let answer = answers
                                .recv_timeout(Duration::from_secs(600))
                                .unwrap_or(None);
                            write_message(&mut stream, &answer)
                        };
                        let _ = handle();
                        active.fetch_sub(1, Ordering::Relaxed);
                    });
                }
            })?;
        Ok(Self(path))
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responses_preserve_literal_selection_and_cancel_when_dropped() {
        let (request, answers) = Request::new(Options::default());
        request.finish(Some("literal $HOME; $(value)".into()));
        assert_eq!(answers.recv().unwrap().unwrap(), "literal $HOME; $(value)");
        let (request, answers) = Request::new(Options::default());
        drop(request);
        assert_eq!(answers.recv().unwrap(), None);
    }
    #[test]
    fn invalid_lengths_fail_before_allocating() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        server.write_all(&[255; 4]).unwrap();
        assert!(read_message::<Options>(&mut client).is_err());
        assert!(
            Options {
                entries: vec!["x".repeat(65537)],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
