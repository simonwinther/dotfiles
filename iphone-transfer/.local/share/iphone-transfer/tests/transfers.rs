use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStringExt,
            fs::{PermissionsExt, symlink},
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tempfile::TempDir;

const BINARY: &str = env!("CARGO_BIN_EXE_iphone-transfer-native");

#[derive(Clone)]
struct Item {
    name: String,
    bytes: Vec<u8>,
    size: u64,
    header_size: Option<u64>,
    chunked: bool,
    get_status: u16,
    delete_status: u16,
}

impl Item {
    fn new(name: &str, bytes: &[u8]) -> Self {
        Self {
            name: name.into(),
            bytes: bytes.to_vec(),
            size: bytes.len() as u64,
            header_size: Some(bytes.len() as u64),
            chunked: false,
            get_status: 200,
            delete_status: 204,
        }
    }
}

struct Inbox {
    items: Vec<Item>,
    requests: Vec<(String, String)>,
    listing: Option<Vec<u8>>,
    verify_saved: Option<PathBuf>,
}

struct Server {
    socket: PathBuf,
    state: Arc<Mutex<Inbox>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn escape(name: &str) -> String {
    let mut result = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

impl Server {
    fn new(desktop: &Desktop, items: Vec<Item>) -> Self {
        let socket = desktop.path("tailscaled.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(Inbox {
            items,
            requests: vec![],
            listing: None,
            verify_saved: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&state);
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => Self::respond(stream, &shared),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("Mock API accept: {e}"),
                }
            }
        });
        Self {
            socket,
            state,
            stop,
            worker: Some(worker),
        }
    }

    fn respond(stream: UnixStream, state: &Arc<Mutex<Inbox>>) {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let parts = line.split_whitespace().collect::<Vec<_>>();
        let (method, path) = (parts[0].to_owned(), parts[1].to_owned());
        let mut host = false;
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if line
                .to_ascii_lowercase()
                .starts_with("host: local-tailscaled.sock")
            {
                host = true;
            }
        }
        assert!(host, "Use the local API host header");
        let mut stream = reader.into_inner();
        let mut state = state.lock().unwrap();
        state.requests.push((method.clone(), path.clone()));
        let (status, body, header, chunked) = if path.starts_with("/localapi/v0/files/?") {
            assert_eq!(method, "GET");
            let bytes = state.listing.clone().unwrap_or_else(|| {
                serde_json::to_vec(
                    &state
                        .items
                        .iter()
                        .map(|i| json!({"Name": i.name,"Size":i.size}))
                        .collect::<Vec<_>>(),
                )
                .unwrap()
            });
            (200, bytes.clone(), Some(bytes.len() as u64), false)
        } else {
            let index = state
                .items
                .iter()
                .position(|i| path == format!("/localapi/v0/files/{}", escape(&i.name)))
                .expect("Requested existing item");
            if method == "DELETE" {
                if let Some(saved) = &state.verify_saved {
                    assert_eq!(
                        fs::read(saved).unwrap(),
                        state.items[index].bytes,
                        "Publish exact data before DELETE"
                    );
                }
                let status = state.items[index].delete_status;
                if status == 204 {
                    state.items.remove(index);
                }
                (status, vec![], Some(0), false)
            } else {
                assert_eq!(method, "GET");
                let item = &state.items[index];
                (
                    item.get_status,
                    item.bytes.clone(),
                    item.header_size,
                    item.chunked,
                )
            }
        };
        drop(state);
        write!(stream, "HTTP/1.1 {status} Mock\r\nConnection: close\r\n").unwrap();
        if chunked {
            write!(stream, "Transfer-Encoding: chunked\r\n").unwrap();
        } else if let Some(size) = header {
            write!(stream, "Content-Length: {size}\r\n").unwrap();
        }
        write!(stream, "\r\n").unwrap();
        if chunked {
            for chunk in body.chunks(7) {
                write!(stream, "{:X}\r\n", chunk.len()).unwrap();
                if stream.write_all(chunk).is_err() {
                    return;
                }
                if stream.write_all(b"\r\n").is_err() {
                    return;
                }
            }
            let _ = stream.write_all(b"0\r\n\r\n");
        } else {
            let _ = stream.write_all(&body);
        }
    }

    fn run(&self, desktop: &Desktop) -> Output {
        desktop
            .command()
            .args(["receive", "--once", "--directory"])
            .arg(desktop.path("received"))
            .arg("--socket")
            .arg(&self.socket)
            .output()
            .unwrap()
    }

    fn requests(&self) -> Vec<(String, String)> {
        self.state.lock().unwrap().requests.clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if !thread::panicking() {
            self.worker.take().unwrap().join().unwrap();
        }
    }
}

struct Desktop {
    dir: TempDir,
}

impl Desktop {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let this = Self { dir };
        for directory in ["home", "config/taildrop", "tools", "runtime"] {
            fs::create_dir_all(this.path(directory)).unwrap();
        }
        this.write(
            "config/taildrop/iphone.json",
            br#"{"name":"test-phone","target":"100.64.0.10"}"#,
        );
        this.write("status.json", serde_json::to_vec(&json!({"BackendState":"Running","Peer":{"test":{"TailscaleIPs":["100.64.0.10"],"DNSName":"fixture.tailnet.example.","Online":false}}})).unwrap().as_slice());
        this.script(
            "file",
            "#!/bin/sh\nprintf '%s\\n' \"${IPHONE_TEST_MIME:-application/octet-stream}\"\n",
        );
        this.script(
            "tailscale",
            r#"#!/bin/sh
printf '%s\0' "$@" > "$IPHONE_TEST_ROOT/tailscale-args"
if [ "$1" = status ]; then /bin/cat "$IPHONE_TEST_ROOT/status.json"; exit 0; fi
if [ -f "$IPHONE_TEST_ROOT/send-fails" ]; then printf 'fixture connection failed\n' >&2; exit 1; fi
exit 0
"#,
        );
        this.script(
            "zenity",
            r#"#!/bin/sh
case "$1" in
--file-selection)
  printf '%s\0' "$@" > "$IPHONE_TEST_ROOT/picker-args"
  if [ -f "$IPHONE_TEST_ROOT/picker-cancel" ]; then exit 1; fi
  /bin/cat "$IPHONE_TEST_ROOT/picker-output" ;;
--progress)
  printf '%s\0' "$@" > "$IPHONE_TEST_ROOT/progress-args"
  while IFS= read -r progress; do :; done ;;
--error) printf '%s\0' "$@" > "$IPHONE_TEST_ROOT/error-args" ;;
*) exit 2 ;;
esac
"#,
        );
        this.script(
            "notify-send",
            "#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$IPHONE_TEST_ROOT/notification\"\n",
        );
        this.script(
            "nautilus",
            "#!/bin/sh\nprintf '%s\\0' \"$@\" > \"$IPHONE_TEST_ROOT/open-args\"\n",
        );
        this
    }

    fn path(&self, path: &str) -> PathBuf {
        self.dir.path().join(path)
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        fs::write(self.path(path), bytes).unwrap();
    }
    fn script(&self, name: &str, contents: &str) {
        let file = self.path("tools").join(name);
        fs::write(&file, contents).unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn command(&self) -> Command {
        let mut c = Command::new(BINARY);
        c.env_clear()
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_RUNTIME_DIR", self.path("runtime"))
            .env("PATH", self.path("tools"))
            .env("IPHONE_TEST_ROOT", self.dir.path())
            .current_dir(self.dir.path());
        c
    }
    fn ok(&self, args: &[&str]) -> Output {
        let out = self.command().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
    fn arguments(&self, path: &str) -> Vec<Vec<u8>> {
        let bytes = fs::read(self.path(path)).unwrap();
        bytes
            .split(|c| *c == 0)
            .take_while(|part| !part.is_empty())
            .map(Vec::from)
            .collect()
    }
    fn partials(&self) -> Vec<PathBuf> {
        fs::read_dir(self.path("received/.incoming"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|s| s == "part"))
            .collect()
    }
}

#[test]
fn receive_publishes_exact_bytes_and_only_then_acknowledges() {
    let desktop = Desktop::new();
    let bytes = b"%PDF-1.7\n\0binary\xffpayload\n";
    let server = Server::new(&desktop, vec![Item::new("report.pdf", bytes)]);
    server.state.lock().unwrap().verify_saved = Some(desktop.path("received/Documents/report.pdf"));
    let result = server.run(&desktop);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(desktop.path("received/Documents/report.pdf")).unwrap(),
        bytes
    );
    assert!(server.state.lock().unwrap().items.is_empty());
    assert_eq!(
        server
            .requests()
            .iter()
            .map(|(m, _)| m.as_str())
            .collect::<Vec<_>>(),
        ["GET", "GET", "DELETE"]
    );
    assert!(desktop.partials().is_empty());
    assert_eq!(
        fs::metadata(desktop.path("received/Documents/report.pdf"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(desktop.path("received/Documents"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn files_keep_their_existing_five_categories() {
    let desktop = Desktop::new();
    let items = [
        "photo.HEIC",
        "movie.mov",
        "song.mp3",
        "notes.md",
        "slides.key",
        "unknown.bin",
    ];
    let server = Server::new(
        &desktop,
        items.iter().map(|n| Item::new(n, b"test data")).collect(),
    );
    let result = server.run(&desktop);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    for path in [
        "Pictures/photo.HEIC",
        "Videos/movie.mov",
        "Audio/song.mp3",
        "Documents/notes.md",
        "Documents/slides.key",
        "Files/unknown.bin",
    ] {
        assert_eq!(
            fs::read(desktop.path("received").join(path)).unwrap(),
            b"test data"
        );
    }
}

#[test]
fn unknown_extensions_use_content_mime_detection() {
    let desktop = Desktop::new();
    let server = Server::new(
        &desktop,
        vec![Item::new("image.unrecognized", b"native image bytes")],
    );
    let result = desktop
        .command()
        .env("IPHONE_TEST_MIME", "image/png")
        .args(["receive", "--once", "--directory"])
        .arg(desktop.path("received"))
        .arg("--socket")
        .arg(&server.socket)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(
        desktop
            .path("received/Pictures/image.unrecognized")
            .exists()
    );
}

#[test]
fn collisions_and_existing_symlinks_are_never_overwritten() {
    let desktop = Desktop::new();
    fs::create_dir_all(desktop.path("received/Pictures")).unwrap();
    desktop.write("received/Pictures/photo.jpg", b"original");
    desktop.write("outside", b"protected");
    symlink(
        desktop.path("outside"),
        desktop.path("received/Pictures/photo (1).jpg"),
    )
    .unwrap();
    let server = Server::new(&desktop, vec![Item::new("photo.jpg", b"incoming")]);
    assert!(server.run(&desktop).status.success());
    assert_eq!(
        fs::read(desktop.path("received/Pictures/photo.jpg")).unwrap(),
        b"original"
    );
    assert_eq!(
        fs::read(desktop.path("received/Pictures/photo (2).jpg")).unwrap(),
        b"incoming"
    );
    assert_eq!(fs::read(desktop.path("outside")).unwrap(), b"protected");
}

#[test]
fn truncated_and_mismatched_files_remain_in_the_inbox() {
    for short_stream in [true, false] {
        let desktop = Desktop::new();
        let mut item = Item::new("broken.pdf", b"short");
        item.size = 15;
        if short_stream {
            item.header_size = Some(15);
        }
        let server = Server::new(&desktop, vec![item]);
        assert!(!server.run(&desktop).status.success());
        assert!(!desktop.path("received/Documents/broken.pdf").exists());
        assert!(desktop.partials().is_empty());
        assert!(!server.requests().iter().any(|(m, _)| m == "DELETE"));
        assert_eq!(server.state.lock().unwrap().items.len(), 1);
    }
}

#[test]
fn chunked_and_empty_downloads_are_complete_before_publication() {
    let desktop = Desktop::new();
    let bytes = b"chunked file with\0binary bytes and several HTTP chunks";
    let mut item = Item::new("chunks.pdf", bytes);
    item.chunked = true;
    item.header_size = None;
    let server = Server::new(&desktop, vec![item, Item::new("empty.txt", b"")]);
    let result = server.run(&desktop);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(desktop.path("received/Documents/chunks.pdf")).unwrap(),
        bytes
    );
    assert_eq!(
        fs::read(desktop.path("received/Documents/empty.txt")).unwrap(),
        b""
    );
}

#[test]
fn failed_acknowledgement_keeps_both_completed_copies() {
    let desktop = Desktop::new();
    let mut item = Item::new("retained.pdf", b"complete document");
    item.delete_status = 500;
    let server = Server::new(&desktop, vec![item]);
    assert!(!server.run(&desktop).status.success());
    assert_eq!(
        fs::read(desktop.path("received/Documents/retained.pdf")).unwrap(),
        b"complete document"
    );
    assert_eq!(server.state.lock().unwrap().items.len(), 1);
    assert!(desktop.partials().is_empty());
}

#[test]
fn large_downloads_retain_every_byte_across_streaming_buffers() {
    let desktop = Desktop::new();
    let bytes = (0..8 * 1024 * 1024 + 29)
        .map(|i| (i % 251) as u8)
        .collect::<Vec<_>>();
    let server = Server::new(&desktop, vec![Item::new("large.mov", &bytes)]);
    assert!(server.run(&desktop).status.success());
    assert_eq!(
        fs::read(desktop.path("received/Videos/large.mov")).unwrap(),
        bytes
    );
    assert!(server.state.lock().unwrap().items.is_empty());
}

#[test]
fn unsafe_names_are_rejected_while_other_files_can_finish() {
    let desktop = Desktop::new();
    let server = Server::new(
        &desktop,
        ["../outside.pdf", "/absolute", ".", "..", "", "bad\0file"]
            .iter()
            .map(|s| Item::new(s, b"unsafe"))
            .chain([Item::new("safe.pdf", b"safe")])
            .collect(),
    );
    assert!(!server.run(&desktop).status.success());
    assert_eq!(
        fs::read(desktop.path("received/Documents/safe.pdf")).unwrap(),
        b"safe"
    );
    assert!(!desktop.path("outside.pdf").exists());
    assert_eq!(server.requests().len(), 3);
    assert_eq!(server.state.lock().unwrap().items.len(), 6);
}

#[test]
fn unicode_and_reserved_filename_characters_are_url_encoded_once() {
    let desktop = Desktop::new();
    let name = "report ?#% é🦀.pdf";
    let server = Server::new(&desktop, vec![Item::new(name, b"document")]);
    assert!(server.run(&desktop).status.success());
    assert_eq!(
        fs::read(desktop.path("received/Documents").join(name)).unwrap(),
        b"document"
    );
    assert_eq!(
        server.requests()[1].1,
        "/localapi/v0/files/report%20%3F%23%25%20%C3%A9%F0%9F%A6%80.pdf"
    );
}

#[test]
fn list_only_reads_the_inbox_and_ignores_proxy_environment() {
    let desktop = Desktop::new();
    let server = Server::new(&desktop, vec![Item::new("pending.pdf", b"untouched")]);
    let output = desktop
        .command()
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .args(["receive", "--list", "--socket"])
        .arg(&server.socket)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()[0]["Name"],
        "pending.pdf"
    );
    assert!(!desktop.path("received").exists());
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn invalid_listing_and_api_failure_do_not_acknowledge_anything() {
    let desktop = Desktop::new();
    let server = Server::new(&desktop, vec![Item::new("pending.pdf", b"untouched")]);
    server.state.lock().unwrap().listing = Some(b"invalid json".to_vec());
    assert!(!server.run(&desktop).status.success());
    assert_eq!(server.requests().len(), 1);
    server.state.lock().unwrap().listing = None;
    server.state.lock().unwrap().items[0].get_status = 403;
    assert!(!server.run(&desktop).status.success());
    assert!(!server.requests().iter().any(|(m, _)| m == "DELETE"));
    assert!(desktop.partials().is_empty());
}

#[test]
fn receiver_lock_protects_active_partial_files() {
    let desktop = Desktop::new();
    fs::create_dir_all(desktop.path("received/.incoming")).unwrap();
    desktop.write("received/.incoming/receive-active.part", b"partial");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(desktop.path("received/.incoming/receiver.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let server = Server::new(&desktop, vec![]);
    let result = server.run(&desktop);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Another receiver"));
    assert!(
        desktop
            .path("received/.incoming/receive-active.part")
            .exists()
    );
    assert!(server.requests().is_empty());
}

#[test]
fn stale_cleanup_and_directory_checks_preserve_unrelated_files() {
    let desktop = Desktop::new();
    fs::create_dir_all(desktop.path("received/.incoming")).unwrap();
    desktop.write("received/.incoming/receive-old.part", b"stale");
    desktop.write("received/.incoming/personal-note", b"keep");
    desktop.write("outside", b"keep");
    symlink(
        desktop.path("outside"),
        desktop.path("received/.incoming/receive-link.part"),
    )
    .unwrap();
    let server = Server::new(&desktop, vec![]);
    assert!(server.run(&desktop).status.success());
    assert!(!desktop.path("received/.incoming/receive-old.part").exists());
    assert!(desktop.path("received/.incoming/personal-note").exists());
    assert!(
        desktop
            .path("received/.incoming/receive-link.part")
            .is_symlink()
    );
    assert_eq!(fs::read(desktop.path("outside")).unwrap(), b"keep");
    fs::remove_dir(desktop.path("received/Pictures")).unwrap();
    fs::create_dir(desktop.path("protected")).unwrap();
    symlink(desktop.path("protected"), desktop.path("received/Pictures")).unwrap();
    assert!(!server.run(&desktop).status.success());
}

#[test]
fn send_keeps_raw_paths_and_shell_characters_as_literal_arguments() {
    let desktop = Desktop::new();
    let first = "report ' $(touch injected);🦀\n.pdf";
    desktop.write(first, b"one");
    let raw = std::ffi::OsString::from_vec(b"raw-\xff.txt".to_vec());
    fs::write(desktop.dir.path().join(&raw), b"two").unwrap();
    let result = desktop
        .command()
        .args(["send", "--"])
        .arg(first)
        .arg(&raw)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let args = desktop.arguments("tailscale-args");
    use std::os::unix::ffi::OsStrExt;
    assert_eq!(
        args,
        [
            b"file".to_vec(),
            b"cp".to_vec(),
            b"--update-interval=0".to_vec(),
            desktop
                .dir
                .path()
                .join(first)
                .as_os_str()
                .as_bytes()
                .to_vec(),
            desktop.dir.path().join(raw).as_os_str().as_bytes().to_vec(),
            b"100.64.0.10:".to_vec()
        ]
    );
    assert!(!desktop.path("injected").exists());
    assert!(desktop.path("notification").exists());
}

#[test]
fn chooser_supports_newlines_and_cancellation_without_a_transfer() {
    let desktop = Desktop::new();
    let first = desktop.path("line\nbreak.pdf");
    let second = desktop.path("other.pdf");
    fs::write(&first, b"one").unwrap();
    fs::write(&second, b"two").unwrap();
    desktop.write(
        "picker-output",
        format!("{}\u{1f}{}\n", first.display(), second.display()).as_bytes(),
    );
    desktop.ok(&["send"]);
    assert_eq!(desktop.arguments("tailscale-args").len(), 6);
    fs::remove_file(desktop.path("tailscale-args")).unwrap();
    fs::remove_file(desktop.path("notification")).unwrap();
    desktop.write("picker-cancel", b"");
    desktop.ok(&["send"]);
    assert!(!desktop.path("tailscale-args").exists());
    assert!(!desktop.path("notification").exists());
}

#[test]
fn invalid_send_paths_and_failed_transfers_never_report_success() {
    let desktop = Desktop::new();
    let invalid = desktop.command().args(["send", "home"]).output().unwrap();
    assert!(!invalid.status.success());
    assert!(!desktop.path("tailscale-args").exists());
    assert!(!desktop.path("notification").exists());
    desktop.write("real.pdf", b"data");
    desktop.write("send-fails", b"");
    let failed = desktop
        .command()
        .args(["send", "real.pdf"])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("fixture connection failed"));
    assert!(!desktop.path("notification").exists());
    assert!(
        desktop
            .arguments("error-args")
            .contains(&b"--no-markup".to_vec())
    );
}

#[test]
fn check_uses_the_existing_phone_settings_without_sending_files() {
    let desktop = Desktop::new();
    let status = desktop.ok(&["send", "--check"]);
    assert!(
        String::from_utf8_lossy(&status.stdout)
            .contains("iPhone: fixture (100.64.0.10)\nOnline: False")
    );
    assert_eq!(
        desktop.arguments("tailscale-args"),
        [b"status".to_vec(), b"--json".to_vec()]
    );
    desktop.write("status.json", br#"{"BackendState":"Running","Peer":{}}"#);
    assert!(
        !desktop
            .command()
            .args(["send", "--check"])
            .output()
            .unwrap()
            .status
            .success()
    );
    desktop.write("status.json", br#"{"BackendState":"Stopped"}"#);
    assert!(
        !desktop
            .command()
            .args(["send", "--check"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!desktop.path("progress-args").exists());
}

#[test]
fn desktop_folder_action_resolves_home_without_machine_specific_paths() {
    let desktop = Desktop::new();
    desktop.ok(&["open"]);
    assert_eq!(
        desktop.arguments("open-args"),
        [desktop.path("home/From iPhone").into_os_string().into_vec()]
    );
}
