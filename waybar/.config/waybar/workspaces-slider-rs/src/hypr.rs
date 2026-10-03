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

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub active: i32,
    pub occupied: HashSet<i32>,
    pub remote: HashMap<i32, usize>,
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

/// Switch using Hyprland's Lua dispatchers without stalling pointer input.
pub fn switch_workspace(monitor: &str, workspace: &str) {
    let command = format!(
        "eval hl.dispatch(hl.dsp.focus({{ monitor = {monitor:?} }})); \
         hl.dispatch(hl.dsp.focus({{ workspace = {workspace:?} }}))"
    );
    std::thread::spawn(move || {
        if let Some(reply) = request(&command) {
            if reply.trim() != "ok" {
                eprintln!("workspace switch failed: {reply}");
            }
        }
    });
}

pub fn focused_monitor() -> Option<String> {
    let raw = request("j/monitors")?;
    let monitors: serde_json::Value = serde_json::from_str(&raw).ok()?;
    monitors
        .as_array()?
        .iter()
        .find(|monitor| monitor["focused"].as_bool() == Some(true))
        .and_then(|monitor| monitor["name"].as_str())
        .map(str::to_owned)
}

/// One monitor's active workspace and occupancy across all monitors.
pub fn snapshot(monitor: &str, workspace_count: i32) -> Option<Snapshot> {
    let raw = request("[[BATCH]]j/monitors ; j/workspaces")?;
    parse_snapshot(&raw, monitor, workspace_count)
}

fn parse_snapshot(raw: &str, monitor: &str, workspace_count: i32) -> Option<Snapshot> {
    // The replies arrive as back-to-back JSON documents.
    let mut documents =
        serde_json::Deserializer::from_str(raw).into_iter::<serde_json::Value>();
    let monitors = documents.next()?.ok()?;
    let workspaces = documents.next()?.ok()?;
    let monitors = monitors.as_array()?;
    let workspaces = workspaces.as_array()?;

    let active = monitors
        .iter()
        .find(|entry| entry["name"].as_str() == Some(monitor))
        .and_then(|entry| entry["activeWorkspace"]["id"].as_i64())
        .unwrap_or(1) as i32;

    // Sorting connector names keeps a monitor's color consistent between
    // sliders, regardless of which monitor a slider itself is running on.
    let mut monitor_names: Vec<&str> = monitors
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    monitor_names.sort_unstable();

    let occupied: HashSet<i32> = workspaces
        .iter()
        .filter_map(|entry| entry["id"].as_i64())
        .map(|id| id as i32)
        .filter(|id| (1..=workspace_count).contains(id))
        .collect();

    let remote = workspaces
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_i64()? as i32;
            let owner = entry["monitor"].as_str()?;
            if !occupied.contains(&id) || owner == monitor {
                return None;
            }
            let color = monitor_names.iter().position(|name| *name == owner)?;
            Some((id, color))
        })
        .collect();

    Some(Snapshot { active, occupied, remote })
}

/// Read the event stream forever, reconnecting if it drops.
pub fn listen(send: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let Some(path) = socket_path(".socket2.sock") else {
            return;
        };
        loop {
            if let Ok(stream) = UnixStream::connect(&path) {
                // Anything that happened before this connect (or during a drop)
                // was missed, so resync rather than waiting on the failsafe.
                send();
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
                    let Some((event, _)) = text.trim_end().split_once(">>") else {
                        continue;
                    };
                    match event {
                        // Workspace events carry no monitor, so resolve the
                        // active workspace from the next monitor snapshot.
                        "workspace"
                        | "workspacev2"
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
                        | "monitorremoved"
                        // Omarchy reloads Hyprland after switching its theme.
                        | "configreloaded" => send(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn distinguish_monitor_ownership_and_keep_the_active_workspace_local() {
        let mut monitors = json!([
            {"name": "DVI-D-1", "activeWorkspace": {"id": 1}},
            {"name": "HDMI-A-1", "activeWorkspace": {"id": 2}},
            {"name": "DP-2", "activeWorkspace": {"id": 3}}
        ]);
        let mut workspaces = json!([
            {"id": 1, "monitor": "DVI-D-1", "windows": 2},
            {"id": 2, "monitor": "HDMI-A-1", "windows": 0},
            {"id": 3, "monitor": "DP-2", "windows": 2},
            {"id": 7, "monitor": "HDMI-A-1", "windows": 1},
            {"id": 10, "monitor": "DP-2", "windows": 1},
            {"id": 11, "monitor": "DP-2", "windows": 1},
            {"id": -98, "monitor": "DP-2", "windows": 1}
        ]);
        let raw = format!("{monitors}\n{workspaces}");
        let occupied: HashSet<i32> = [1, 2, 3, 7, 10].into_iter().collect();

        for (monitor, active) in [("DVI-D-1", 1), ("HDMI-A-1", 2), ("DP-2", 3)] {
            let snapshot = parse_snapshot(&raw, monitor, 10).unwrap();
            assert_eq!(snapshot.active, active);
            assert_eq!(snapshot.occupied, occupied);
            assert!(!snapshot.remote.contains_key(&active));
        }
        let snapshot = parse_snapshot(&raw, "DP-2", 10).unwrap();
        assert_eq!(snapshot.remote.keys().copied().collect::<HashSet<_>>(), [1, 2, 7].into_iter().collect());
        assert_ne!(snapshot.remote[&1], snapshot.remote[&2]);
        assert_eq!(snapshot.remote[&2], snapshot.remote[&7]);
        let hdmi = parse_snapshot(&raw, "HDMI-A-1", 10).unwrap();
        assert_eq!(snapshot.remote[&1], hdmi.remote[&1]);

        // Switching a different screen updates its label without moving the
        // indicator on DP-2, even when the previous workspace disappears.
        monitors[1]["activeWorkspace"]["id"] = json!(4);
        workspaces[1]["id"] = json!(4);
        let raw = format!("{monitors}\n{workspaces}");
        let snapshot = parse_snapshot(&raw, "DP-2", 10).unwrap();
        assert_eq!(snapshot.active, 3);
        assert_eq!(snapshot.occupied, [1, 3, 4, 7, 10].into_iter().collect());
        assert!(!snapshot.remote.contains_key(&2));
        assert_eq!(snapshot.remote[&4], snapshot.remote[&7]);

        // Ownership can change while occupancy and the active workspace stay
        // identical, so this must still update the label's color.
        workspaces[3]["monitor"] = json!("DP-2");
        let raw = format!("{monitors}\n{workspaces}");
        let moved = parse_snapshot(&raw, "DP-2", 10).unwrap();
        assert_eq!(moved.active, snapshot.active);
        assert_eq!(moved.occupied, snapshot.occupied);
        assert!(!moved.remote.contains_key(&7));
    }
}
