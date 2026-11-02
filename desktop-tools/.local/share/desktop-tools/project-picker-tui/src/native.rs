use crate::{
    cache,
    schema::{EXTENSION_ORIGIN, MAIN_PAGE, MAX_MESSAGE, Snapshot, Status, invalid},
};
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

const MAX_URL: usize = 8192;

/// Socket where the extension's persistent native host accepts open requests.
pub fn open_socket() -> io::Result<PathBuf> {
    let runtime =
        env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| invalid("XDG_RUNTIME_DIR is required"))?;
    Ok(PathBuf::from(runtime).join("project-picker/open.sock"))
}

/// Only ChatGPT pages may be opened in the reused browser window.
pub fn project_url(url: &str) -> bool {
    url.len() <= MAX_URL
        && (url == MAIN_PAGE || url.starts_with(&format!("{MAIN_PAGE}/")))
        && !url.chars().any(char::is_control)
}

fn read_frame(source: &mut impl Read) -> io::Result<Value> {
    let mut header = [0; 4];
    source.read_exact(&mut header)?;
    let size = u32::from_ne_bytes(header) as usize;
    if size == 0 || size > MAX_MESSAGE {
        return Err(invalid("Native message exceeds the size limit"));
    }
    let mut data = vec![0; size];
    source.read_exact(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}

fn write_frame(destination: &mut impl Write, value: &Value) -> io::Result<()> {
    let data = serde_json::to_vec(value)?;
    destination.write_all(&(data.len() as u32).to_ne_bytes())?;
    destination.write_all(&data)?;
    destination.flush()
}

/// Asks the extension to show `url` in an existing ChatGPT app window.
/// Returns false when no browser is listening or no window could be reused.
pub fn request_open(url: &str) -> bool {
    if !project_url(url) {
        return false;
    }
    let Ok(path) = open_socket() else {
        return false;
    };
    let Ok(mut stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(2500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    if stream.write_all(format!("{url}\n").as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream.take(8))
        .read_line(&mut reply)
        .is_ok_and(|_| reply == "ok\n")
}

/// Keeps the extension's native port open and relays open requests from
/// `project-picker --open` to it. Exits when the browser closes the port.
fn listen(path: &Path) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| invalid("Invalid socket path"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)?;
    // The newest browser connection takes over from a stale or older one.
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let (replies, received) = mpsc::channel();
    let socket = path.to_owned();
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        while let Ok(value) = read_frame(&mut input) {
            if replies.send(value).is_err() {
                break;
            }
        }
        // The browser closed the port; stop accepting requests for it.
        let _ = fs::remove_file(&socket);
        std::process::exit(0);
    });
    let mut output = io::stdout().lock();
    let mut next = 0_u64;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
        let mut url = String::new();
        let mut reader = BufReader::new((&stream).take(MAX_URL as u64 + 1));
        if reader.read_line(&mut url).is_err() || !project_url(url.trim_end_matches('\n')) {
            let _ = (&stream).write_all(b"no\n");
            continue;
        }
        while received.try_recv().is_ok() {}
        next += 1;
        write_frame(
            &mut output,
            &json!({"action": "open", "id": next, "url": url.trim_end_matches('\n')}),
        )?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut opened = false;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match received.recv_timeout(left) {
                Ok(reply) if reply["id"] == next => {
                    opened = reply["ok"] == true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = (&stream).write_all(if opened { b"ok\n" } else { b"no\n" });
    }
    Ok(())
}

fn receive(origin: Option<&OsStr>, path: &Path, source: &mut impl Read) -> io::Result<Value> {
    // Reject the origin before reading or allocating the browser's message.
    if origin != Some(OsStr::new(EXTENSION_ORIGIN)) {
        return Err(invalid("Unexpected extension origin"));
    }
    let mut header = [0; 4];
    source.read_exact(&mut header)?;
    let size = u32::from_ne_bytes(header) as usize;
    if size == 0 || size > MAX_MESSAGE {
        return Err(invalid("Native message exceeds the size limit"));
    }
    let mut data = vec![0; size];
    source.read_exact(&mut data)?;
    let value: Value = serde_json::from_slice(&data)?;
    if value.get("action").and_then(Value::as_str) == Some("listen") {
        listen(&open_socket()?)?;
        return Ok(json!({"ok": true}));
    }
    if value.get("action").and_then(Value::as_str) == Some("status") {
        let status = Status::from_message(&value, cache::now())?;
        cache::save_status(&status, path)?;
        Ok(json!({"ok": true}))
    } else {
        let snapshot: Snapshot = serde_json::from_value(value)?;
        Ok(match cache::save_snapshot(snapshot, path)? {
            Some(count) => json!({"ok": true, "updated": true, "count": count}),
            None => json!({"ok": true, "updated": false}),
        })
    }
}

/// One request and one framed reply, with no diagnostic text on stdout.
pub fn serve(
    origin: Option<&OsStr>,
    path: &Path,
    source: &mut impl Read,
    destination: &mut impl Write,
) -> io::Result<bool> {
    let result = receive(origin, path, source).unwrap_or_else(
        |_| json!({"ok": false, "error": "Could not save a complete project snapshot"}),
    );
    let data = serde_json::to_vec(&result)?;
    destination.write_all(&(data.len() as u32).to_ne_bytes())?;
    destination.write_all(&data)?;
    destination.flush()?;
    Ok(result["ok"] == true)
}
