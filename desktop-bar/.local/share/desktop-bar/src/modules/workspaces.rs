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

#[derive(Debug, Clone)]
pub enum Event {
    OutputsChanged,
    Changed,
    Unavailable,
    Urgent(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub active: i32,
    pub space: Option<String>,
    pub occupied: HashSet<i32>,
    pub remote: HashMap<i32, usize>,
    pub urgent: HashSet<i32>,
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

pub fn focused_output() -> Option<String> {
    let reply: serde_json::Value = serde_json::from_str(&request("j/monitors")?).ok()?;
    reply
        .as_array()?
        .iter()
        .find(|output| output["focused"].as_bool() == Some(true))?["name"]
        .as_str()
        .map(str::to_owned)
}

pub fn active_window() -> Option<String> {
    let reply: serde_json::Value = serde_json::from_str(&request("j/activewindow")?).ok()?;
    reply["address"].as_str().map(str::to_owned)
}

/// Private Hyprspace IPC uses message boundaries, unlike Hyprland's stream.
/// Replies are bounded so malformed or future responses cannot grow memory.
fn spaces_request(payload: &str) -> Option<serde_json::Value> {
    serde_json::from_slice(&spaces_reply(payload, IO_TIMEOUT, 1024 * 1024)?).ok()
}

pub fn launch_context() -> String {
    spaces_reply("capture", Duration::from_millis(300), 128)
        .and_then(|reply| String::from_utf8(reply).ok())
        .filter(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        .unwrap_or_default()
}

fn spaces_reply(payload: &str, duration: Duration, limit: usize) -> Option<Vec<u8>> {
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
        tv_sec: duration.as_secs() as _,
        tv_usec: duration.subsec_micros() as _,
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
    if size <= 0 || size as usize > limit {
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
    Some(reply)
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
        if let Some(reply) = request(&command)
            && reply.trim() != "ok"
        {
            eprintln!("workspace switch failed: {reply}");
        }
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
/// Snapshots for each monitor from one compositor query.
pub fn snapshots(
    monitors: &[String],
    workspace_count: i32,
    urgent: &HashSet<i64>,
) -> Vec<Option<Snapshot>> {
    let state =
        spaces_request("spaces").filter(|state| state["schema"] == 1 && state["type"] == "spaces");
    match state.as_ref().and_then(|state| state["enabled"].as_bool()) {
        Some(true) => {
            let state = state.as_ref().unwrap();
            monitors
                .iter()
                .map(|monitor| parse_spaces_snapshot(state, monitor, workspace_count, urgent))
                .collect()
        }
        Some(false) => {
            let raw = request("[[BATCH]]j/monitors ; j/workspaces");
            monitors
                .iter()
                .map(|monitor| {
                    let mut snapshot = parse_snapshot(raw.as_deref()?, monitor, workspace_count)?;
                    snapshot.urgent = urgent
                        .iter()
                        .copied()
                        .map(|n| n as i32)
                        .filter(|n| (1..=workspace_count).contains(n))
                        .collect();
                    Some(snapshot)
                })
                .collect()
        }
        None => monitors.iter().map(|_| None).collect(),
    }
}

fn urgent_native(addresses: &mut HashSet<String>) -> HashSet<i64> {
    if addresses.is_empty() {
        return HashSet::new();
    }
    let focused = request("j/activewindow")
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|c| c["address"].as_str().map(str::to_owned));
    let Some(clients) =
        request("j/clients").and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    else {
        return HashSet::new();
    };
    let Some(clients) = clients.as_array() else {
        return HashSet::new();
    };
    addresses.retain(|a| {
        focused.as_ref() != Some(a)
            && clients
                .iter()
                .any(|c| c["address"].as_str() == Some(a.as_str()))
    });
    clients
        .iter()
        .filter(|c| c["address"].as_str().is_some_and(|a| addresses.contains(a)))
        .filter_map(|c| c["workspace"]["id"].as_i64())
        .collect()
}

fn parse_spaces_snapshot(
    state: &serde_json::Value,
    monitor: &str,
    workspace_count: i32,
    native_urgent: &HashSet<i64>,
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
    let mut urgent = HashSet::new();
    // Workspaces belong to the active space in this top-level array. Entries
    // under `spaces` are summaries and do not contain workspace arrays.
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
        if entry["native_id"]
            .as_i64()
            .is_some_and(|id| native_urgent.contains(&id))
        {
            urgent.insert(number);
        }
        let owner = entry["monitor"].as_str().unwrap_or("");
        if owner != monitor {
            remote.insert(
                number,
                monitor_names
                    .iter()
                    .position(|name| *name == owner)
                    .unwrap_or(0),
            );
        }
    }
    Some(Snapshot {
        urgent,
        active,
        space: Some(space),
        occupied,
        remote,
    })
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
        .filter(|entry| entry["windows"].as_u64().is_some_and(|windows| windows > 0))
        .filter_map(|entry| entry["id"].as_i64())
        .map(|id| id as i32)
        .filter(|id| (1..=workspace_count).contains(id))
        .collect();

    let remote = workspaces
        .iter()
        .filter_map(|entry| {
            let id = entry["id"].as_i64()? as i32;
            let owner = entry["monitor"].as_str().unwrap_or("");
            if !occupied.contains(&id) || owner == monitor {
                return None;
            }
            let color = monitor_names
                .iter()
                .position(|name| *name == owner)
                .unwrap_or(0);
            Some((id, color))
        })
        .collect();

    Some(Snapshot {
        urgent: HashSet::new(),
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
                        "urgent" => send(Event::Urgent(format!(
                            "0x{}",
                            payload.trim_start_matches("0x")
                        ))),
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

/// One event socket and snapshot worker serve every output in this process.
pub fn start(
    sender: smithay_client_toolkit::reexports::calloop::channel::Sender<super::Update>,
) -> mpsc::Sender<Vec<String>> {
    enum Message {
        Outputs(Vec<String>),
        Event(Event),
    }
    let (tx, rx) = mpsc::channel();
    let events = tx.clone();
    listen(move |event| {
        let _ = events.send(Message::Event(event));
    });
    let (outputs, changes) = mpsc::channel();
    std::thread::spawn(move || {
        for names in changes {
            if tx.send(Message::Outputs(names)).is_err() {
                break;
            }
        }
    });
    std::thread::Builder::new()
        .name("bar-workspaces".into())
        .spawn(move || {
            let mut names = Vec::new();
            let mut urgent = HashSet::new();
            // Last snapshot sent per output. New output lists can come with
            // rebuilt bars, so they clear it and every output is sent again.
            let mut sent: HashMap<String, Option<Snapshot>> = HashMap::new();
            while let Ok(message) = rx.recv() {
                let mut unavailable = false;
                let mut apply = |message| match message {
                    Message::Outputs(new) => {
                        names = new;
                        sent.clear();
                    }
                    Message::Event(Event::Urgent(address)) => {
                        urgent.insert(address);
                    }
                    Message::Event(event) => unavailable = matches!(event, Event::Unavailable),
                };
                apply(message);
                for message in rx.try_iter() {
                    apply(message);
                }
                let native_urgent = urgent_native(&mut urgent);
                let states = if unavailable {
                    names.iter().map(|_| None).collect()
                } else {
                    snapshots(&names, 10, &native_urgent)
                };
                for (name, state) in names.iter().zip(states) {
                    if sent.get(name) == Some(&state) {
                        continue;
                    }
                    sent.insert(name.clone(), state.clone());
                    if sender
                        .send(super::Update::Workspaces(name.clone(), state))
                        .is_err()
                    {
                        return;
                    }
                }
            }
        })
        .expect("workspace worker");
    outputs
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
        let occupied: HashSet<i32> = [1, 3, 7, 10].into_iter().collect();

        for (monitor, active) in [("DVI-D-1", 1), ("HDMI-A-1", 2), ("DP-2", 3)] {
            let snapshot = parse_snapshot(&raw, monitor, 10).unwrap();
            assert_eq!(snapshot.active, active);
            assert_eq!(snapshot.occupied, occupied);
            assert!(!snapshot.remote.contains_key(&active));
        }
        let snapshot = parse_snapshot(&raw, "DP-2", 10).unwrap();
        assert_eq!(
            snapshot.remote.keys().copied().collect::<HashSet<_>>(),
            [1, 7].into_iter().collect()
        );
        assert_ne!(snapshot.remote[&1], snapshot.remote[&7]);
        assert!(!snapshot.remote.contains_key(&2));
        let hdmi = parse_snapshot(&raw, "HDMI-A-1", 10).unwrap();
        assert_eq!(snapshot.remote[&1], hdmi.remote[&1]);

        // Switching a different screen updates its label without moving the
        // indicator on DP-2, even when the previous workspace disappears.
        monitors[1]["activeWorkspace"]["id"] = json!(4);
        workspaces[1]["id"] = json!(4);
        let raw = format!("{monitors}\n{workspaces}");
        let snapshot = parse_snapshot(&raw, "DP-2", 10).unwrap();
        assert_eq!(snapshot.active, 3);
        assert_eq!(snapshot.occupied, [1, 3, 7, 10].into_iter().collect());
        assert!(!snapshot.remote.contains_key(&2));
        assert!(!snapshot.remote.contains_key(&4));

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
        let snapshot = parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new()).unwrap();
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.space.as_deref(), Some("hobby-uuid"));
        assert_eq!(snapshot.occupied, [1, 3].into_iter().collect());
        assert_eq!(snapshot.remote, [(3, 0)].into_iter().collect());
        assert!(!snapshot.occupied.contains(&10001));
        let remote = parse_spaces_snapshot(&state, "DP-2", 10, &HashSet::new()).unwrap();
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
        let hobby = parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new()).unwrap();
        state["active"] = json!("work");
        state["workspaces"] = json!([]);
        let work = parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new()).unwrap();
        assert_eq!(hobby.active, work.active);
        assert_ne!(hobby.space, work.space);
        assert_eq!(hobby.occupied, [3].into_iter().collect());
        assert!(work.occupied.is_empty());
        assert!(work.remote.is_empty());
    }

    #[test]
    fn space_summaries_do_not_hide_workspace_state_or_urgency() {
        let state = json!({
            "schema":1,"type":"spaces","enabled":true,"active":"thesis",
            "spaces":[
                {"id":"home","active":false,"occupied_workspaces":1,"windows":1},
                {"id":"thesis","active":true,"occupied_workspaces":3,"windows":3}
            ],
            "monitors":[{"name":"eDP-1","number":1}],
            "workspaces":[
                {"number":1,"native_id":-1338,"monitor":"eDP-1","occupied":true,"special":false},
                {"number":2,"native_id":-1339,"monitor":"eDP-1","occupied":true,"special":false},
                {"number":3,"native_id":-1337,"monitor":"eDP-1","occupied":true,"special":false},
                {"number":4,"native_id":0,"monitor":"","occupied":false,"special":false},
                {"number":0,"native_id":-98,"monitor":"eDP-1","occupied":true,"special":true}
            ]
        });
        let snapshot = parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new()).unwrap();
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.space.as_deref(), Some("thesis"));
        assert_eq!(snapshot.occupied, HashSet::from([1, 2, 3]));
        assert!(snapshot.urgent.is_empty());
        // Native IDs from other spaces and special workspaces do not leak
        // into the displayed space's local workspace numbers.
        let snapshot =
            parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::from([-1339, -98, -2000]))
                .unwrap();
        assert_eq!(snapshot.urgent, HashSet::from([2]));
    }

    #[test]
    fn unknown_or_missing_space_metadata_fails_closed() {
        let mut state = json!({"schema":1,"type":"spaces","enabled":true,"active":"hobby",
            "monitors":[{"name":"eDP-1","number":0}],"workspaces":[]});
        assert_eq!(
            parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new())
                .unwrap()
                .active,
            0
        );
        assert!(parse_spaces_snapshot(&state, "missing", 10, &HashSet::new()).is_none());
        state["monitors"][0]["number"] = json!(i64::MAX);
        assert_eq!(
            parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new())
                .unwrap()
                .active,
            0
        );
        state["schema"] = json!(2);
        assert!(parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new()).is_none());
        state["schema"] = json!(1);
        state["active"] = json!("");
        assert!(parse_spaces_snapshot(&state, "eDP-1", 10, &HashSet::new()).is_none());
    }
}
