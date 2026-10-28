//! Hyprland IPC, spoken directly over its unix sockets.
//!
//! The Python version shelled out to `hyprctl`, which meant a fork+exec for
//! every state read. Talking to `.socket.sock` ourselves turns that into a
//! sub-millisecond round trip with no process involved.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy)]
pub enum Event {
    OutputsChanged,
    Changed,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub active: i32,
    pub space: Option<String>,
    pub occupied: HashSet<i32>,
    pub remote: HashMap<i32, usize>,
}

fn runtime_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(1000);
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

/// Private Hyprspace IPC uses message boundaries, unlike Hyprland's stream.
/// Replies are bounded so malformed or future responses cannot grow memory.
fn spaces_request(payload: &str) -> Option<serde_json::Value> {
    let path = socket_path("hyprspace.sock")?;
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    if bytes.len() >= address.sun_path.len() {
        return None;
    }
    for (target, source) in address.sun_path.iter_mut().zip(bytes) {
        *target = *source as libc::c_char;
    }
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC, 0) };
    if raw < 0 {
        return None;
    }
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    let timeout = libc::timeval {
        tv_sec: IO_TIMEOUT.as_secs() as _,
        tv_usec: 0,
    };
    for option in [libc::SO_RCVTIMEO, libc::SO_SNDTIMEO] {
        if unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                &timeout as *const _ as *const libc::c_void,
                std::mem::size_of_val(&timeout) as libc::socklen_t,
            )
        } != 0
        {
            return None;
        }
    }
    if unsafe {
        libc::connect(
            socket.as_raw_fd(),
            &address as *const _ as *const libc::sockaddr,
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    } != 0
    {
        return None;
    }
    if unsafe {
        libc::send(
            socket.as_raw_fd(),
            payload.as_ptr().cast(),
            payload.len(),
            libc::MSG_NOSIGNAL,
        )
    } != payload.len() as isize
    {
        return None;
    }
    let size = unsafe {
        libc::recv(
            socket.as_raw_fd(),
            std::ptr::null_mut(),
            0,
            libc::MSG_PEEK | libc::MSG_TRUNC,
        )
    };
    if size <= 0 || size > 1024 * 1024 {
        return None;
    }
    let mut reply = vec![0_u8; size as usize];
    let count = unsafe {
        libc::recv(
            socket.as_raw_fd(),
            reply.as_mut_ptr().cast(),
            reply.len(),
            libc::MSG_TRUNC,
        )
    };
    if count != size {
        return None;
    }
    serde_json::from_slice(&reply[..count as usize]).ok()
}

/// Selection carries the displayed space UUID. If the user switches spaces
/// before a queued click arrives, the compositor rejects it without moving focus.
type WorkspaceCommand = (String, String, Option<String>);

pub fn switch_workspace(monitor: &str, workspace: &str, space: Option<&str>) {
    static COMMANDS: OnceLock<mpsc::SyncSender<WorkspaceCommand>> = OnceLock::new();
    let sender = COMMANDS.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<WorkspaceCommand>(32);
        std::thread::spawn(move || {
            for (monitor, workspace, space) in receiver {
                execute_workspace(monitor, workspace, space);
            }
        });
        sender
    });
    // FIFO avoids reordered rapid clicks; the bounded queue never blocks input
    // or creates one thread per wheel event during an IPC interruption.
    if sender
        .try_send((
            monitor.to_owned(),
            workspace.to_owned(),
            space.map(str::to_owned),
        ))
        .is_err()
    {
        eprintln!("workspace command queue is full or unavailable");
    }
}

fn execute_workspace(monitor: String, workspace: String, space: Option<String>) {
    if let Some(space) = space {
        let payload = format!(
            "workspace {}",
            serde_json::json!({
                "monitor": monitor, "workspace": workspace, "space": space
            })
        );
        match spaces_request(&payload) {
            Some(reply) if reply["success"] == true => {}
            Some(reply) => eprintln!("workspace switch rejected: {}", reply["error"]),
            None => eprintln!("workspace switch unavailable: Hyprspace did not reply"),
        }
    } else {
        // Native compatibility is used only after an explicit enabled:false
        // reply. An unavailable plugin never exposes another space's state.
        let command = format!(
            "eval hl.dispatch(hl.dsp.focus({{ monitor = {monitor:?} }})); \
             hl.dispatch(hl.dsp.focus({{ workspace = {workspace:?} }}))"
        );
        if let Some(reply) = request(&command) {
            if reply.trim() != "ok" {
                eprintln!("workspace switch failed: {reply}");
            }
        }
    }
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

/// Enabled, unmirrored outputs that use the full Waybar configuration.
pub fn monitor_names() -> Option<HashSet<String>> {
    let raw = request("j/monitors")?;
    let monitors: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    let managed = std::env::var_os("WORKSPACES_SLIDER_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let path = config_dir.join("desktop/workspaces.json");
            path.is_file().then_some(path)
        });
    let raw =
        std::fs::read(managed.unwrap_or_else(|| config_dir.join("waybar/config.jsonc"))).ok()?;
    let config: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    eligible_monitors(&monitors, &config)
}

fn eligible_monitors(
    monitors: &serde_json::Value,
    config: &serde_json::Value,
) -> Option<HashSet<String>> {
    Some(
        monitors
            .as_array()?
            .iter()
            .filter(|monitor| monitor["disabled"].as_bool() != Some(true))
            .filter(|monitor| {
                !monitor["mirrorOf"]
                    .as_str()
                    .is_some_and(|name| name != "none")
            })
            .filter(|monitor| selected_output(monitor, &config["output"]))
            .filter_map(|monitor| monitor["name"].as_str())
            .map(str::to_owned)
            .collect(),
    )
}

fn selected_output(monitor: &serde_json::Value, output: &serde_json::Value) -> bool {
    let patterns: Vec<&str> = match output {
        serde_json::Value::Null => return true,
        serde_json::Value::String(pattern) => vec![pattern],
        serde_json::Value::Array(patterns) => {
            patterns.iter().filter_map(|value| value.as_str()).collect()
        }
        _ => return false,
    };
    let matches = |pattern: &str| {
        let pattern = pattern.strip_prefix("desc:").unwrap_or(pattern);
        pattern == "*"
            || monitor["name"].as_str() == Some(pattern)
            || monitor["description"].as_str() == Some(pattern)
    };
    if patterns
        .iter()
        .any(|pattern| pattern.strip_prefix('!').is_some_and(matches))
    {
        return false;
    }
    !patterns.is_empty()
        && (patterns.iter().all(|pattern| pattern.starts_with('!'))
            || patterns
                .iter()
                .any(|pattern| !pattern.starts_with('!') && matches(pattern)))
}

/// Resolve only active-space metadata. Absence or an unknown schema fails
/// closed rather than silently falling back to the compositor's global IDs.
pub fn snapshot(monitor: &str, workspace_count: i32) -> Option<Snapshot> {
    let state = spaces_request("spaces")?;
    if state["schema"] != 1 || state["type"] != "spaces" {
        return None;
    }
    if state["enabled"].as_bool()? {
        parse_spaces_snapshot(&state, monitor, workspace_count)
    } else {
        let raw = request("[[BATCH]]j/monitors ; j/workspaces")?;
        parse_snapshot(&raw, monitor, workspace_count)
    }
}

fn parse_spaces_snapshot(
    state: &serde_json::Value,
    monitor: &str,
    workspace_count: i32,
) -> Option<Snapshot> {
    if state["schema"] != 1 || state["type"] != "spaces" || state["enabled"] != true {
        return None;
    }
    let space = state["active"].as_str()?.to_owned();
    if space.is_empty() {
        return None;
    }
    let monitors = state["monitors"].as_array()?;
    let active = monitors
        .iter()
        .find(|entry| entry["name"].as_str() == Some(monitor))?["number"]
        .as_i64()
        .map(|number| i32::try_from(number).unwrap_or(0))?;
    let mut monitor_names: Vec<&str> = monitors
        .iter()
        .filter_map(|entry| entry["name"].as_str())
        .collect();
    monitor_names.sort_unstable();
    monitor_names.dedup();
    let mut occupied = HashSet::new();
    let mut remote = HashMap::new();
    for entry in state["workspaces"].as_array()? {
        if entry["special"].as_bool() != Some(false) || entry["occupied"].as_bool() != Some(true) {
            continue;
        }
        let Some(number) = entry["number"]
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
        else {
            continue;
        };
        if !(1..=workspace_count).contains(&number) {
            continue;
        }
        occupied.insert(number);
        if let Some(owner) = entry["monitor"].as_str().filter(|owner| *owner != monitor) {
            if let Some(color) = monitor_names.iter().position(|name| *name == owner) {
                remote.insert(number, color);
            }
        }
    }
    Some(Snapshot {
        active,
        space: Some(space),
        occupied,
        remote,
    })
}

/// The existing native event socket drives updates. State reads happen in this
/// worker, never inside the Wayland pointer or frame callbacks.
pub fn listen_snapshots(
    monitor: String,
    workspace_count: i32,
    send: impl Fn(Option<Snapshot>) + Send + 'static,
) {
    listen(move |event| {
        if matches!(event, Event::Unavailable) {
            send(None);
        } else {
            send(snapshot(&monitor, workspace_count));
        }
    });
}

fn parse_snapshot(raw: &str, monitor: &str, workspace_count: i32) -> Option<Snapshot> {
    // The replies arrive as back-to-back JSON documents.
    let mut documents = serde_json::Deserializer::from_str(raw).into_iter::<serde_json::Value>();
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

    Some(Snapshot {
        active,
        space: None,
        occupied,
        remote,
    })
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
                send(Event::OutputsChanged);
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
                        "monitoradded" | "monitoraddedv2" | "monitorremoved"
                        | "monitorremovedv2" | "configreloaded" => {
                            send(Event::OutputsChanged);
                        }
                        // Workspace events carry no monitor, so resolve the
                        // active workspace from the next monitor snapshot.
                        "workspace" | "workspacev2" | "createworkspace" | "createworkspacev2"
                        | "destroyworkspace" | "destroyworkspacev2" | "moveworkspace"
                        | "moveworkspacev2" | "focusedmon" | "focusedmonv2" | "openwindow"
                        | "closewindow" | "movewindow" | "movewindowv2" | "renameworkspace"
                        | "activespecial" | "activespecialv2" => send(Event::Changed),
                        "custom" if payload == "hyprspace:unavailable" => send(Event::Unavailable),
                        "custom" if payload.starts_with("hyprspace:") => send(Event::Changed),
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

    #[test]
    fn managed_outputs_match_descriptions_and_apply_exclusions_before_wildcards() {
        let monitors = serde_json::json!([
            {"name":"eDP-1","description":"Laptop display"},
            {"name":"HDMI-A-1","description":"Ancor Communications Inc VG248 J5LMQS024117"},
            {"name":"DP-2","description":"External display"}
        ]);
        let configured =
            serde_json::json!({"output":["*","!Ancor Communications Inc VG248 J5LMQS024117"]});
        let expected = HashSet::from(["eDP-1".to_owned(), "DP-2".to_owned()]);
        assert_eq!(eligible_monitors(&monitors, &configured), Some(expected));
        let configured = serde_json::json!({"output":"desc:External display"});
        assert_eq!(
            eligible_monitors(&monitors, &configured),
            Some(HashSet::from(["DP-2".to_owned()]))
        );
        assert_eq!(
            eligible_monitors(&monitors, &serde_json::json!({"output":[]})),
            Some(HashSet::new())
        );
    }
    use serde_json::json;

    #[test]
    fn only_full_bar_outputs_get_pills() {
        let monitors = json!([
            {"name": "eDP-1", "mirrorOf": "none"},
            {"name": "HDMI-A-1", "mirrorOf": "none"},
            {"name": "DP-2", "disabled": true},
            {"name": "DP-3", "mirrorOf": "eDP-1"}
        ]);
        let laptop: HashSet<String> = ["eDP-1".to_owned()].into_iter().collect();
        assert_eq!(
            eligible_monitors(&monitors, &json!({"output": ["eDP-1", "DP-2", "DP-3"]})),
            Some(laptop.clone())
        );
        assert_eq!(
            eligible_monitors(&monitors, &json!({"output": "eDP-1"})),
            Some(laptop)
        );
        let both = ["eDP-1".to_owned(), "HDMI-A-1".to_owned()]
            .into_iter()
            .collect();
        assert_eq!(eligible_monitors(&monitors, &json!({})), Some(both));
    }

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
        assert_eq!(
            snapshot.remote.keys().copied().collect::<HashSet<_>>(),
            [1, 2, 7].into_iter().collect()
        );
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

    #[test]
    fn spaces_use_local_numbers_and_only_populated_normal_destinations() {
        let state = json!({
            "schema":1,"type":"spaces","enabled":true,"active":"hobby-uuid",
            "monitors":[{"name":"DP-2","number":3},{"name":"eDP-1","number":1}],
            "workspaces":[
                {"number":1,"native_id":10001,"monitor":"eDP-1","occupied":true,"special":false},
                {"number":3,"native_id":10003,"monitor":"DP-2","occupied":true,"special":false},
                {"number":5,"native_id":10005,"monitor":"DP-2","occupied":false,"special":false},
                {"number":7,"native_id":-98,"monitor":"DP-2","occupied":true,"special":true},
                {"number":99,"native_id":10099,"monitor":"DP-2","occupied":true,"special":false}
            ]
        });
        let snapshot = parse_spaces_snapshot(&state, "eDP-1", 10).unwrap();
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.space.as_deref(), Some("hobby-uuid"));
        assert_eq!(snapshot.occupied, [1, 3].into_iter().collect());
        assert_eq!(snapshot.remote, [(3, 0)].into_iter().collect());
        assert!(!snapshot.occupied.contains(&10001));
        let remote = parse_spaces_snapshot(&state, "DP-2", 10).unwrap();
        assert_eq!(remote.active, 3);
        assert_eq!(remote.remote, [(1, 1)].into_iter().collect());
    }

    #[test]
    fn repeated_local_numbers_change_namespace_without_retaining_occupancy() {
        let mut state = json!({
            "schema":1,"type":"spaces","enabled":true,"active":"hobby",
            "monitors":[{"name":"eDP-1","number":1}],
            "workspaces":[{"number":3,"native_id":12345,"monitor":"eDP-1","occupied":true,"special":false}]
        });
        let hobby = parse_spaces_snapshot(&state, "eDP-1", 10).unwrap();
        state["active"] = json!("work");
        state["workspaces"] = json!([]);
        let work = parse_spaces_snapshot(&state, "eDP-1", 10).unwrap();
        assert_eq!(hobby.active, work.active);
        assert_ne!(hobby.space, work.space);
        assert_eq!(hobby.occupied, [3].into_iter().collect());
        assert!(work.occupied.is_empty());
        assert!(work.remote.is_empty());
    }

    #[test]
    fn unknown_or_missing_space_metadata_fails_closed() {
        let mut state = json!({"schema":1,"type":"spaces","enabled":true,"active":"hobby",
            "monitors":[{"name":"eDP-1","number":0}],"workspaces":[]});
        assert_eq!(
            parse_spaces_snapshot(&state, "eDP-1", 10).unwrap().active,
            0
        );
        assert!(parse_spaces_snapshot(&state, "missing", 10).is_none());
        state["monitors"][0]["number"] = json!(i64::MAX);
        assert_eq!(
            parse_spaces_snapshot(&state, "eDP-1", 10).unwrap().active,
            0
        );
        state["schema"] = json!(2);
        assert!(parse_spaces_snapshot(&state, "eDP-1", 10).is_none());
        state["schema"] = json!(1);
        state["active"] = json!("");
        assert!(parse_spaces_snapshot(&state, "eDP-1", 10).is_none());
    }
}
