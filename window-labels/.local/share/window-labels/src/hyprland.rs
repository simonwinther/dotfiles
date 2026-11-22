//! Minimal Hyprland IPC: JSON requests and the event stream.
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceRef {
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub address: String,
    pub mapped: bool,
    pub hidden: bool,
    #[serde(default = "visible_by_default")]
    pub visible: bool,
    pub at: [i32; 2],
    pub size: [i32; 2],
    pub workspace: WorkspaceRef,
    pub monitor: i64,
    pub title: String,
    pub fullscreen: u8,
    pub grouped: Vec<String>,
    #[serde(rename = "focusHistoryID")]
    pub focus_history_id: i64,
}

fn visible_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Monitor {
    pub id: i64,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub active_workspace: WorkspaceRef,
    pub special_workspace: WorkspaceRef,
}

fn socket_dir() -> io::Result<PathBuf> {
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .ok_or_else(|| io::Error::other("HYPRLAND_INSTANCE_SIGNATURE is not set"))?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    Ok(runtime.join("hypr").join(signature))
}

pub fn request<T: DeserializeOwned>(command: &str) -> io::Result<T> {
    let mut stream = UnixStream::connect(socket_dir()?.join(".socket.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply)?;
    serde_json::from_slice(&reply).map_err(io::Error::other)
}

/// Calls `notify` for every compositor event until the stream closes.
pub fn watch_events(mut notify: impl FnMut() -> bool) -> io::Result<()> {
    let stream = UnixStream::connect(socket_dir()?.join(".socket2.sock"))?;
    for line in BufReader::new(stream).lines() {
        line?;
        if !notify() {
            break;
        }
    }
    Ok(())
}
