//! Hyprland IPC, spoken directly over its unix sockets.
//!
//! The Python version shelled out to `hyprctl`, which meant a fork+exec for
//! every state read. Talking to `.socket.sock` ourselves turns that into a
//! sub-millisecond round trip with no process involved.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(1);

/// What the socket2 listener hands back to the main loop.
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// A `workspacev2` line, whose payload already carries the new id. Lets the
    /// indicator start moving without waiting for a state read. The line says
    /// nothing about which monitor, so it applies to the focused one.
    Switched(i32),
    /// Something else changed; re-read state to find out what.
    Changed,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub active: i32,
    pub occupied: HashSet<i32>,
    pub focused: bool,
}

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1000);
    PathBuf::from(format!("/run/user/{uid}"))
}

fn socket_path(name: &str) -> Option<PathBuf> {
    let signature = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    Some(runtime_dir().join("hypr").join(signature).join(name))
}

/// Send one request to the command socket and read the whole reply.
fn request(payload: &str) -> Option<String> {
    let stream = UnixStream::connect(socket_path(".socket.sock")?).ok()?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    let mut stream = stream;
    stream.write_all(payload.as_bytes()).ok()?;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).ok()?;
    Some(String::from_utf8_lossy(&reply).into_owned())
}

/// Fire a dispatch off the main thread; nothing here needs its reply, and a
/// busy compositor should never stall a click.
pub fn dispatch(command: String) {
    std::thread::spawn(move || {
        let _ = request(&format!("[[BATCH]]{command}"));
    });
}

/// Active workspace and occupancy for every monitor, in a single round trip.
/// One read covers all of them, so a pill per output costs no more IPC than one.
pub fn snapshot_all(workspace_count: i32) -> Option<HashMap<String, Snapshot>> {
    let raw = request("[[BATCH]]j/monitors ; j/workspaces")?;
    // The replies arrive as back-to-back JSON documents.
    let mut documents =
        serde_json::Deserializer::from_str(&raw).into_iter::<serde_json::Value>();
    let monitors = documents.next()?.ok()?;
    let workspaces = documents.next()?.ok()?;

    let mut snapshots: HashMap<String, Snapshot> = monitors
        .as_array()?
        .iter()
        .filter_map(|entry| {
            let name = entry["name"].as_str()?.to_owned();
            let snapshot = Snapshot {
                active: entry["activeWorkspace"]["id"].as_i64().unwrap_or(1) as i32,
                occupied: HashSet::new(),
                focused: entry["focused"].as_bool() == Some(true),
            };
            Some((name, snapshot))
        })
        .collect();

    for entry in workspaces.as_array()? {
        let (Some(monitor), Some(id)) = (entry["monitor"].as_str(), entry["id"].as_i64()) else {
            continue;
        };
        let id = id as i32;
        if !(1..=workspace_count).contains(&id) {
            continue;
        }
        if let Some(snapshot) = snapshots.get_mut(monitor) {
            snapshot.occupied.insert(id);
        }
    }

    Some(snapshots)
}

/// Read the event stream forever, reconnecting if it drops.
pub fn listen(send: impl Fn(Event) + Send + 'static) {
    std::thread::spawn(move || {
        let Some(path) = socket_path(".socket2.sock") else {
            return;
        };
        loop {
            if let Ok(stream) = UnixStream::connect(&path) {
                // Anything that happened before this connect (or during a drop)
                // was missed, so resync rather than waiting on the failsafe.
                send(Event::Changed);
                let mut reader = BufReader::new(stream);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    match reader.read_until(b'\n', &mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    // Event lines can carry window titles, which are not
                    // guaranteed to be valid UTF-8.
                    let text = String::from_utf8_lossy(&line);
                    let Some((event, payload)) = text.trim_end().split_once(">>") else {
                        continue;
                    };
                    match event {
                        "workspacev2" => {
                            let id = payload.split(',').next().and_then(|v| v.parse().ok());
                            if let Some(id) = id {
                                send(Event::Switched(id));
                            }
                            send(Event::Changed);
                        }
                        "workspace"
                        | "createworkspace"
                        | "createworkspacev2"
                        | "destroyworkspace"
                        | "destroyworkspacev2"
                        | "moveworkspace"
                        | "moveworkspacev2"
                        | "focusedmon"
                        | "focusedmonv2"
                        | "monitoradded"
                        | "monitoraddedv2"
                        | "monitorremoved" => send(Event::Changed),
                        _ => {}
                    }
                }
            }
            // Covers both a failed connect and a clean EOF (Hyprland
            // restarting), either of which would otherwise spin this loop.
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}
