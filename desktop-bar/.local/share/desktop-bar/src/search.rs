//! Search providers use the existing daemon without starting another interface.
use crate::{
    modules::Update,
    shell::{Entry, Reply},
};
use serde::{Deserialize, Serialize};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::{
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const PROVIDERS: &[&str] = &[
    "desktopapplications",
    "files",
    "calc",
    "runner",
    "symbols",
    "unicode",
    "websearch",
    "todo",
    "providerlist",
    "menus:repositories",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub providers: Vec<String>,
    pub query: String,
}
impl Request {
    pub fn launcher(provider: &str, query: &str) -> Self {
        let (provider, query) = if provider.is_empty() || provider == "apps" {
            [
                ("gh ", "menus:repositories"),
                ("/", "providerlist"),
                (".", "files"),
                (":", "symbols"),
                ("=", "calc"),
                ("@", "websearch"),
                (">", "runner"),
                ("$", "clipboard"),
            ]
            .into_iter()
            .find_map(|(prefix, provider)| {
                query
                    .strip_prefix(prefix)
                    .map(|query| (provider, query.trim_start()))
            })
            .unwrap_or(("desktopapplications,websearch", query))
        } else {
            (provider, query)
        };
        Self {
            providers: provider.split(',').map(str::to_owned).collect(),
            query: query.into(),
        }
    }
}

pub fn title(provider: &str) -> &str {
    match provider {
        "desktopapplications" => "Apps",
        "files" => "Files",
        "calc" => "Calculator",
        "runner" => "Commands",
        "symbols" => "Symbols",
        "unicode" => "Unicode",
        "websearch" => "Web search",
        "todo" => "Tasks",
        "providerlist" => "Search modes",
        "menus:repositories" => "Repositories",
        _ => "Search",
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct Item {
    identifier: String,
    text: String,
    subtext: String,
    icon: String,
    provider: String,
    actions: Vec<String>,
    state: Vec<String>,
}
#[derive(Deserialize)]
struct Response {
    item: Option<Item>,
}

pub fn action_label(action: &str) -> &str {
    match action {
        "start" | "activate" => "Open",
        "new_instance" => "Open new instance",
        "opendir" => "Open containing folder",
        "copypath" => "Copy path",
        "copyfile" => "Copy file link",
        "runterminal" => "Run in terminal",
        "run_cmd" => "Copy symbol",
        "search" => "Search",
        "open_url" => "Open URL",
        "save_next" => "Save and add another",
        "change_category" => "Change category",
        "refresh_index" => "Refresh files",
        "delete_all" => "Clear calculator history",
        "clear" => "Clear completed tasks",
        "create" => "New task",
        "done" => "Toggle completed",
        "active" => "Start task",
        "inactive" => "Pause task",
        "copy" => "Copy",
        "open" => "Open",
        "run" => "Run",
        "save" => "Save",
        "delete" => "Delete",
        _ => action,
    }
}

pub fn launcher_entries() -> Vec<Entry> {
    [
        (
            "Bluetooth controls",
            "Devices, pairing and radio",
            "󰂯",
            "panel",
            "bluetooth",
        ),
        (
            "Sound controls",
            "Playback, recording, outputs, inputs and profiles",
            "",
            "panel",
            "audio",
        ),
        (
            "Quick sound",
            "Speaker and microphone volume",
            "",
            "panel",
            "volume",
        ),
        (
            "Wi-Fi controls",
            "Networks, connection and radio",
            "󰖩",
            "panel",
            "wifi",
        ),
        (
            "Notifications",
            "Current notifications and history",
            "󰂚",
            "panel",
            "notifications",
        ),
        (
            "Keybindings",
            "Search desktop shortcuts",
            "󰌌",
            "panel",
            "keybindings",
        ),
        (
            "Clipboard history",
            "Copied text and images",
            "",
            "panel",
            "clipboard",
        ),
        (
            "Desktop controls",
            "Morphing Notch Bar dashboard",
            "󰒓",
            "panel",
            "controls",
        ),
        (
            "Desktop menu",
            "Settings, appearance and system actions",
            "󰍜",
            "menu",
            "main",
        ),
        (
            "Themes",
            "Desktop colors and palettes",
            "󰏘",
            "panel",
            "themes",
        ),
        (
            "Wallpapers",
            "Choose a background",
            "󰸉",
            "panel",
            "wallpapers",
        ),
        ("Bar style", "Clean and Motion", "󰕮", "panel", "styles"),
        ("Calendar", "Date and month view", "󰃭", "panel", "calendar"),
        (
            "Battery",
            "Charge, usage and remaining time",
            "󰁹",
            "panel",
            "battery",
        ),
        (
            "System status",
            "CPU, memory and storage",
            "󰍛",
            "panel",
            "system",
        ),
        ("Files", "Find and open files", "󰈔", "provider", "files"),
        (
            "Calculator",
            "Calculate and convert units",
            "󰃬",
            "provider",
            "calc",
        ),
        ("Commands", "Run a command", "󰆍", "provider", "runner"),
        (
            "Symbols and emoji",
            "Find and copy a symbol",
            "󰞅",
            "provider",
            "symbols",
        ),
        (
            "Unicode",
            "Find a Unicode character",
            "󰘎",
            "provider",
            "unicode",
        ),
        ("Web search", "Search the web", "󰖟", "provider", "websearch"),
        (
            "Tasks",
            "Tasks and scheduled reminders",
            "󰄱",
            "provider",
            "todo",
        ),
        (
            "Repositories",
            "Open a repository",
            "󰊤",
            "provider",
            "menus:repositories",
        ),
    ]
    .into_iter()
    .map(|(name, detail, icon, command, target)| Entry {
        id: format!("control/{target}"),
        name: name.into(),
        detail: detail.into(),
        icon: icon.into(),
        command: vec![command.into(), target.into()],
        ..Default::default()
    })
    .collect()
}

/// Text matched after the title: name, detail and keywords. Clipboard text
/// entries search their preview without keeping a second copy as keywords.
fn search_text<'a>(entry: &Entry, buffer: &'a mut String) -> &'a str {
    buffer.clear();
    buffer.push_str(&entry.name);
    buffer.push(' ');
    buffer.push_str(&entry.detail);
    buffer.push(' ');
    match &entry.clipboard {
        Some(item) if entry.keywords.is_empty() && entry.image.is_empty() => {
            buffer.extend(
                item.preview
                    .chars()
                    .take(crate::clipboard::SEARCH_CHARACTERS),
            );
        }
        _ => buffer.push_str(&entry.keywords),
    }
    buffer
}

pub fn ranked(entries: &[Entry], query: &str) -> Vec<usize> {
    let mut buffer = String::new();
    let mut scored: Vec<_> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let title = crate::fuzzy::score(query, &entry.name, false).map(|score| score + 1000);
            let detail = || crate::fuzzy::score(query, search_text(entry, &mut buffer), false);
            title.or_else(detail).map(|score| (index, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(index, _)| index).collect()
}

impl Item {
    fn entry(self, query: &str) -> Entry {
        let action = self.actions.first().cloned().unwrap_or_default();
        let command = if self.provider == "providerlist" {
            vec!["provider".into(), self.identifier.clone()]
        } else {
            vec![
                "search-activate".into(),
                self.provider.clone(),
                self.identifier.clone(),
                action,
                query.into(),
            ]
        };
        let image = std::path::Path::new(&self.icon)
            .is_absolute()
            .then_some(self.icon.clone())
            .unwrap_or_default();
        let icon = match self.provider.as_str() {
            "files" if self.icon == "folder" => "󰉋",
            "files" => "󰈔",
            "calc" => "󰃬",
            "runner" => "󰆍",
            "websearch" => "󰖟",
            "todo" if self.state.iter().any(|s| s == "done") => "󰄲",
            "todo" => "󰄱",
            "providerlist" => "󰍉",
            "menus:repositories" => "󰊤",
            _ => "",
        }
        .into();
        Entry {
            id: self.identifier,
            name: self.text,
            detail: self.subtext,
            image,
            icon,
            command,
            actions: self.actions,
            action_provider: self.provider,
            ..Default::default()
        }
    }
}

fn connect() -> io::Result<UnixStream> {
    let stream = UnixStream::connect(crate::theme::runtime().join("desktop-search/search.sock"))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}
fn send(stream: &mut UnixStream, kind: u8, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Search request is too large".into());
    }
    stream
        .write_all(&[kind, 1])
        .and_then(|_| stream.write_all(&(bytes.len() as u32).to_be_bytes()))
        .and_then(|_| stream.write_all(&bytes))
        .map_err(|e| e.to_string())
}
fn read_exact(
    stream: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> Result<(), String> {
    while !bytes.is_empty() {
        if cancelled() {
            return Err("Cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err("Search timed out".into());
        }
        let mut fd = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut fd, 1, 20) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.to_string());
        }
        if ready == 0 {
            continue;
        }
        match stream.read(bytes) {
            Ok(0) => return Err("Search service disconnected".into()),
            Ok(count) => bytes = &mut bytes[count..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
fn frame(
    stream: &mut UnixStream,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> Result<(u8, Vec<u8>), String> {
    let mut header = [0; 5];
    read_exact(stream, &mut header, deadline, cancelled)?;
    let length = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
    if length > 1024 * 1024 {
        return Err("Search response is too large".into());
    }
    let mut bytes = vec![0; length];
    read_exact(stream, &mut bytes, deadline, cancelled)?;
    Ok((header[0], bytes))
}

pub fn query(request: &Request, cancelled: &impl Fn() -> bool) -> Result<Vec<Entry>, String> {
    let mut stream = connect().map_err(|e| format!("Search service unavailable: {e}"))?;
    send(
        &mut stream,
        0,
        &serde_json::json!({"providers":request.providers,"query":request.query,"maxresults":256}),
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut entries = vec![];
    let mut total = 0;
    loop {
        let (kind, bytes) = frame(&mut stream, deadline, cancelled)?;
        total += bytes.len();
        if total > 8 * 1024 * 1024 {
            return Err("Search results exceed their limit".into());
        }
        match kind {
            0 | 1 => {
                let response: Response =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if let Some(item) = response.item {
                    if entries.len() == 256 {
                        return Err("Too many search results".into());
                    }
                    entries.push(item.entry(&request.query));
                }
            }
            254 => {}
            255 => {
                if request
                    .providers
                    .iter()
                    .any(|provider| provider == "desktopapplications")
                {
                    entries.extend(launcher_entries());
                    let order = ranked(&entries, &request.query);
                    entries = order
                        .into_iter()
                        .map(|index| entries[index].clone())
                        .collect();
                    if entries
                        .iter()
                        .any(|entry| entry.action_provider != "websearch")
                    {
                        entries.retain(|entry| entry.action_provider != "websearch");
                    }
                    entries.truncate(256);
                }
                return Ok(entries);
            }
            _ => return Err("Unexpected search response".into()),
        }
    }
}

pub fn activate(arguments: &[String]) -> Result<(), String> {
    let [provider, identifier, action, query] = arguments else {
        return Err("Invalid provider action".into());
    };
    let mut stream = connect().map_err(|e| e.to_string())?;
    let context = if matches!(
        provider.as_str(),
        "runner" | "websearch" | "menus:repositories" | "desktopapplications"
    ) || (provider == "files" && matches!(action.as_str(), "open" | "opendir"))
    {
        crate::modules::workspaces::launch_context()
    } else {
        String::new()
    };
    send(
        &mut stream,
        1,
        &serde_json::json!({"provider":provider,"identifier":identifier,"action":action,"query":query,"context":context}),
    )?;
    let (kind, bytes) = frame(
        &mut stream,
        Instant::now() + Duration::from_secs(5),
        &|| false,
    )?;
    activation_response(kind, &bytes)
}

fn activation_response(kind: u8, bytes: &[u8]) -> Result<(), String> {
    if kind != 2 {
        return Err("Unexpected action response".into());
    }
    if bytes.is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(error.as_str().unwrap_or("Provider action failed").into());
    }
    Ok(())
}

pub fn keep_open(provider: &str, action: &str) -> bool {
    provider == "todo" || matches!(action, "delete" | "delete_all" | "save" | "refresh_index")
}

enum Task {
    Query(Request),
    Filter(String, Vec<Entry>, bool),
}
struct Job {
    generation: u64,
    revision: u64,
    task: Task,
}
pub struct Service {
    pending: Arc<(Mutex<Option<Job>>, Condvar)>,
    revision: Arc<AtomicU64>,
}
impl Service {
    pub fn start(sender: Sender<Update>, generation: Arc<AtomicU64>) -> Self {
        let pending: Arc<(Mutex<Option<Job>>, Condvar)> = Arc::default();
        let revision = Arc::new(AtomicU64::new(0));
        let work = pending.clone();
        let latest = revision.clone();
        std::thread::Builder::new()
            .name("bar-search".into())
            .spawn(move || {
                loop {
                    let Some(job) = ({
                        let (lock, ready) = &*work;
                        let mut job = lock.lock().unwrap();
                        while job.is_none() {
                            job = ready.wait(job).unwrap();
                        }
                        // Cached matching runs immediately; expensive providers coalesce typing bursts.
                        while matches!(job.as_ref().map(|job| &job.task), Some(Task::Query(_))) {
                            let (next, timeout) =
                                ready.wait_timeout(job, Duration::from_millis(8)).unwrap();
                            job = next;
                            if timeout.timed_out() {
                                break;
                            }
                        }
                        job.take()
                    }) else {
                        continue;
                    };
                    let cancelled = || {
                        job.generation != generation.load(Ordering::Relaxed)
                            || job.revision != latest.load(Ordering::Relaxed)
                    };
                    if cancelled() {
                        continue;
                    }
                    let reply = match job.task {
                        Task::Query(request) => {
                            Reply::Search(job.generation, job.revision, query(&request, &cancelled))
                        }
                        Task::Filter(query, entries, web_fallback) => {
                            let matches = ranked(&entries, &query);
                            if matches.is_empty() && web_fallback {
                                Reply::Search(
                                    job.generation,
                                    job.revision,
                                    self::query(
                                        &Request {
                                            providers: vec!["websearch".into()],
                                            query,
                                        },
                                        &cancelled,
                                    ),
                                )
                            } else {
                                Reply::Filtered(job.generation, job.revision, matches)
                            }
                        }
                    };
                    if !cancelled() && sender.send(Update::Shell(reply)).is_err() {
                        break;
                    }
                }
            })
            .expect("search worker");
        Self { pending, revision }
    }
    pub fn request(&self, generation: u64, request: Request) -> u64 {
        let revision = self.revision.fetch_add(1, Ordering::Relaxed) + 1;
        *self.pending.0.lock().unwrap() = Some(Job {
            generation,
            revision,
            task: Task::Query(request),
        });
        self.pending.1.notify_one();
        revision
    }
    pub fn filter(
        &self,
        generation: u64,
        query: &str,
        entries: &[Entry],
        web_fallback: bool,
    ) -> u64 {
        let revision = self.revision.fetch_add(1, Ordering::Relaxed) + 1;
        *self.pending.0.lock().unwrap() = Some(Job {
            generation,
            revision,
            task: Task::Filter(query.into(), entries.to_vec(), web_fallback),
        });
        self.pending.1.notify_one();
        revision
    }
    pub fn cancel(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
        *self.pending.0.lock().unwrap() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clipboard_text_is_searched_from_its_bounded_preview() {
        let item = |preview: String| clipboard_history::Item {
            identifier: "1".into(),
            preview,
            ..Default::default()
        };
        let entries = vec![
            Entry {
                name: "first line".into(),
                clipboard: Some(item("first line\nneedle".into())),
                ..Default::default()
            },
            Entry {
                name: "image".into(),
                image: "/tmp/needle.png".into(),
                clipboard: Some(item("/tmp/needle.png".into())),
                ..Default::default()
            },
            Entry {
                name: "long".into(),
                clipboard: Some(item("x".repeat(20000) + "needle")),
                ..Default::default()
            },
        ];
        assert_eq!(ranked(&entries, "needle"), vec![0]);
    }
    #[test]
    fn prefixes_preserve_the_query_and_named_providers_do_not_reinterpret_it() {
        assert_eq!(
            Request::launcher("apps", "= 1 + 2"),
            Request {
                providers: vec!["calc".into()],
                query: "1 + 2".into()
            }
        );
        assert_eq!(
            Request::launcher("apps", "gh literal-name"),
            Request {
                providers: vec!["menus:repositories".into()],
                query: "literal-name".into()
            }
        );
        assert_eq!(Request::launcher("symbols", ":heart").query, ":heart");
    }
    #[test]
    fn activation_accepts_empty_success_frames_and_reports_backend_errors() {
        assert!(activation_response(2, &[]).is_ok());
        assert!(activation_response(2, b"{}").is_ok());
        assert_eq!(
            activation_response(2, br#"{"error":"Action failed"}"#).unwrap_err(),
            "Action failed"
        );
        assert!(activation_response(0, &[]).is_err());
    }
    #[test]
    fn provider_results_keep_backend_order_and_literal_activation_arguments() {
        let entry = Item {
            provider: "runner".into(),
            identifier: "generic".into(),
            actions: vec!["run".into(), "runterminal".into()],
            ..Default::default()
        }
        .entry("printf '%s' '$HOME;' ");
        assert_eq!(
            entry.command,
            [
                "search-activate",
                "runner",
                "generic",
                "run",
                "printf '%s' '$HOME;' "
            ]
        );
        assert_eq!(entry.actions.len(), 2);
    }
    #[test]
    fn framed_reads_reject_oversized_or_truncated_messages_and_can_be_cancelled() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        server.write_all(&[0, 255, 255, 255, 255]).unwrap();
        assert!(
            frame(
                &mut client,
                Instant::now() + Duration::from_secs(1),
                &|| false
            )
            .unwrap_err()
            .contains("too large")
        );
        assert_eq!(
            frame(
                &mut client,
                Instant::now() + Duration::from_secs(1),
                &|| true
            )
            .unwrap_err(),
            "Cancelled"
        );
        drop(server);
        assert!(
            frame(
                &mut client,
                Instant::now() + Duration::from_secs(1),
                &|| false
            )
            .is_err()
        );
    }
}
