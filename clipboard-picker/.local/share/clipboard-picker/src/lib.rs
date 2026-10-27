use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Item {
    pub identifier: String,
    pub text: String,
    pub subtext: String,
    pub preview: String,
    pub preview_type: String,
    pub state: Vec<String>,
    pub actions: Vec<String>,
}

impl Item {
    pub fn image(&self) -> bool {
        self.preview_type == "file"
            && image_mime(Path::new(&self.preview)).is_some()
            && Path::new(&self.preview).is_file()
    }

    pub fn pinned(&self) -> bool {
        self.state.iter().any(|s| s == "pinned")
    }

    pub fn matches(&self, query: &str, images: bool) -> bool {
        (!images || self.image())
            && (query.is_empty()
                || format!("{} {}", self.text, self.subtext)
                    .to_lowercase()
                    .contains(&query.to_lowercase()))
    }

    pub fn payload(&self) -> Result<Payload> {
        if self.image() {
            let path = Path::new(&self.preview);
            ensure!(
                path.metadata()?.len() <= 64 * 1024 * 1024,
                "Image exceeds 64 MiB"
            );
            Ok(Payload {
                mime: image_mime(path).unwrap().into(),
                bytes: std::fs::read(path).context("Read saved image")?,
            })
        } else if self.preview_type == "file" {
            let uri = if self.preview.starts_with("file://") {
                self.preview.clone()
            } else {
                file_uri(Path::new(&self.preview))?
            };
            Ok(Payload {
                mime: "text/uri-list".into(),
                bytes: format!("{uri}\r\n").into_bytes(),
            })
        } else if self.preview_type == "text" {
            Ok(Payload {
                mime: "text/plain;charset=utf-8".into(),
                bytes: self.preview.as_bytes().to_vec(),
            })
        } else {
            bail!("This file entry must be copied through its provider")
        }
    }
}

pub fn image_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        _ => None,
    }
}

fn socket() -> Result<UnixStream> {
    let path = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .context("XDG_RUNTIME_DIR is not set")?
        .join("elephant/elephant.sock");
    let stream = UnixStream::connect(path).context("Connect to Elephant clipboard history")?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    Ok(stream)
}

fn request(stream: &mut impl Write, kind: u8, data: &Value) -> Result<()> {
    let body = serde_json::to_vec(data)?;
    stream.write_all(&[kind, 1])?; // Elephant's JSON wire format.
    stream.write_all(&(body.len() as u32).to_be_bytes())?;
    stream.write_all(&body)?;
    Ok(())
}

fn read_items(stream: &mut impl Read) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    loop {
        let mut header = [0; 5];
        stream
            .read_exact(&mut header)
            .context("Read history response")?;
        let size = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
        ensure!(size <= 16 * 1024 * 1024, "History response exceeds 16 MiB");
        let mut body = vec![0; size];
        stream.read_exact(&mut body)?;
        match header[0] {
            255 => break,    // Done.
            254 => continue, // Empty.
            0 | 1 => {
                let value: Value = serde_json::from_slice(&body)?;
                if let Some(item) = value.get("item") {
                    let item: Item = serde_json::from_value(item.clone())?;
                    if !item.identifier.is_empty() && items.len() < 1000 {
                        items.push(item);
                    }
                }
            }
            _ => bail!("Unsupported history response"),
        }
    }
    // The provider sends entries with higher scores first; use stable sorting
    // so pinned entries stay ahead without changing recency within each group.
    items.sort_by_key(|item| !item.pinned());
    Ok(items)
}

pub fn history() -> Result<Vec<Item>> {
    let mut stream = socket()?;
    request(
        &mut stream,
        0,
        &json!({"providers":["clipboard"],"query":"","maxresults":1000}),
    )?;
    read_items(&mut stream)
}

pub fn activate(item: &Item, action: &str) -> Result<()> {
    ensure!(
        item.actions.iter().any(|a| a == action),
        "Action is unavailable"
    );
    request(
        &mut socket()?,
        1,
        &json!({
            "provider":"clipboard", "identifier":item.identifier, "action":action,
            "query":"", "arguments":""
        }),
    )
}

#[derive(Clone, Debug)]
pub struct Payload {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub fn prepare_batch(items: &[Item]) -> Result<Vec<Payload>> {
    let mut total = 0;
    let mut payloads = Vec::new();
    for item in items {
        let payload = item.payload()?;
        total += payload.bytes.len();
        ensure!(
            total <= 256 * 1024 * 1024,
            "Selection exceeds 256 MiB; paste a smaller batch"
        );
        payloads.push(payload);
    }
    Ok(payloads)
}

pub fn copy(payload: &Payload) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", &payload.mime])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Start wl-copy")?;
    child
        .stdin
        .take()
        .context("Open clipboard input")?
        .write_all(&payload.bytes)?;
    ensure!(child.wait()?.success(), "Could not acquire the clipboard");
    Ok(())
}

pub fn active_window() -> Result<Value> {
    let output = Command::new("hyprctl")
        .args(["-j", "activewindow"])
        .output()?;
    ensure!(output.status.success(), "Could not read the active window");
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub trait Desktop {
    fn active(&mut self) -> Result<Value>;
    fn copy(&mut self, payload: &Payload) -> Result<()>;
    fn paste(&mut self, address: &str, terminal: bool) -> Result<()>;
    fn wait(&mut self, duration: Duration);
}

pub struct Hyprland;
impl Desktop for Hyprland {
    fn active(&mut self) -> Result<Value> {
        active_window()
    }
    fn copy(&mut self, payload: &Payload) -> Result<()> {
        copy(payload)
    }
    fn paste(&mut self, address: &str, terminal: bool) -> Result<()> {
        let mods = if terminal { "CTRL SHIFT" } else { "CTRL" };
        let selector = serde_json::to_string(&format!("address:{address}"))?;
        let code = format!(
            "hl.dispatch(hl.dsp.send_shortcut({{ mods = \"{mods}\", key = \"V\", window = {selector} }}))"
        );
        let result = Command::new("hyprctl").args(["eval", &code]).output()?;
        ensure!(
            result.status.success() && !String::from_utf8_lossy(&result.stdout).contains("error"),
            "Could not deliver paste shortcut"
        );
        Ok(())
    }
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub fn terminal(class: &str) -> bool {
    matches!(
        class.to_lowercase().as_str(),
        "com.mitchellh.ghostty"
            | "ghostty"
            | "kitty"
            | "alacritty"
            | "foot"
            | "footclient"
            | "org.wezfurlong.wezterm"
            | "wezterm"
            | "org.gnome.terminal"
            | "gnome-terminal-server"
            | "konsole"
            | "xterm"
            | "urxvt"
            | "st-256color"
    )
}

pub fn paste_batch(
    desktop: &mut impl Desktop,
    target: &str,
    payloads: &[Payload],
    delay: Duration,
) -> Result<()> {
    ensure!(!target.is_empty(), "No destination window; use Copy only");
    ensure!(!payloads.is_empty(), "Nothing selected");
    ensure!(
        (Duration::from_millis(200)..=Duration::from_secs(10)).contains(&delay),
        "Paste delay must be 200–10000 ms"
    );
    // Give the layer surface time to relinquish keyboard focus.
    desktop.wait(Duration::from_millis(180));
    for (index, payload) in payloads.iter().enumerate() {
        let window = desktop.active()?;
        ensure!(
            window["address"].as_str() == Some(target),
            "Paste stopped before item {}: the destination lost focus",
            index + 1
        );
        desktop
            .copy(payload)
            .with_context(|| format!("Copy item {}", index + 1))?;
        let window = desktop.active()?;
        ensure!(
            window["address"].as_str() == Some(target),
            "Paste stopped: destination changed; this item remains copied"
        );
        desktop.paste(target, terminal(window["class"].as_str().unwrap_or("")))?;
        // Most receivers read clipboard bytes asynchronously. Keep ownership
        // stable during their import and check focus again before the next item.
        desktop.wait(delay);
    }
    Ok(())
}

pub fn file_uri(path: &Path) -> Result<String> {
    use std::os::unix::ffi::OsStrExt;
    ensure!(path.is_absolute(), "File path must be absolute");
    let mut result = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(byte) {
            result.push(*byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(result)
}

pub fn export_files(items: &[Item]) -> Result<Payload> {
    ensure!(
        !items.is_empty() && items.iter().all(Item::image),
        "Select only images to copy files"
    );
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .context("Neither XDG_CACHE_HOME nor HOME is set")?;
    export_files_in(items, &cache.join("clipboard-picker/exports"))
}

fn export_files_in(items: &[Item], root: &Path) -> Result<Payload> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    std::fs::create_dir_all(root)?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    let dir = root.join(format!("{stamp}-{}", std::process::id()));
    std::fs::create_dir(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let result = (|| {
        let mut uris = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let payload = item.payload()?;
            let extension = Path::new(&item.preview).extension().unwrap();
            let dest = dir.join(format!(
                "image-{:03}.{}",
                index + 1,
                extension.to_string_lossy()
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&dest)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file.write_all(&payload.bytes)?;
            uris.push(file_uri(&dest)?);
        }
        Ok(Payload {
            mime: "text/uri-list".into(),
            bytes: format!("{}\r\n", uris.join("\r\n")).into_bytes(),
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(kind: u8, body: Value) -> Vec<u8> {
        let data = serde_json::to_vec(&body).unwrap();
        let mut out = vec![kind];
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
        out
    }
    #[test]
    fn history_keeps_full_text_and_stable_pin_order() {
        let mut data = frame(
            0,
            json!({"item":{"identifier":"new", "preview":"a\nb <literal>","preview_type":"text"}}),
        );
        data.extend(frame(
            0,
            json!({"item":{"identifier":"pin","state":["pinned"]}}),
        ));
        data.extend(frame(0, json!({"item":{"identifier":"old"}})));
        data.extend(frame(255, json!({})));
        let items = read_items(&mut &data[..]).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|i| i.identifier.as_str())
                .collect::<Vec<_>>(),
            ["pin", "new", "old"]
        );
        assert_eq!(items[1].payload().unwrap().bytes, b"a\nb <literal>");
    }
    #[test]
    fn corrupt_history_is_bounded_and_rejected() {
        assert!(read_items(&mut &[0, 255, 255, 255, 255][..]).is_err());
        assert!(read_items(&mut &[0, 0, 0, 0, 5, b'{'][..]).is_err());
        assert!(read_items(&mut &frame(9, json!({}))[..]).is_err());
    }
    #[test]
    fn file_uris_escape_clipboard_delimiters() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b\n#æ.png")).unwrap(),
            "file:///tmp/a%20b%0A%23%C3%A6.png"
        );
    }

    #[test]
    fn exported_images_survive_history_eviction_with_private_permissions() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "clipboard-export-test-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let original = root.join("original.png");
        let bytes = b"\x89PNG\r\n\x1a\nsynthetic image bytes";
        std::fs::write(&original, bytes).unwrap();
        let item = Item {
            preview: original.to_string_lossy().into(),
            preview_type: "file".into(),
            ..Default::default()
        };
        let copied = export_files_in(&[item], &root.join("exports")).unwrap();
        assert_eq!(copied.mime, "text/uri-list");
        let uri = String::from_utf8(copied.bytes).unwrap();
        let exported = PathBuf::from(uri.trim().strip_prefix("file://").unwrap());
        std::fs::remove_file(original).unwrap();
        assert_eq!(std::fs::read(&exported).unwrap(), bytes);
        assert_eq!(
            exported.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            exported
                .parent()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[derive(Default)]
    struct Fake {
        events: Vec<String>,
        reads: usize,
        change_at: usize,
        fail_copy: bool,
    }
    impl Desktop for Fake {
        fn active(&mut self) -> Result<Value> {
            self.reads += 1;
            Ok(
                json!({"address": if self.change_at == self.reads { "other" } else { "original" },"class":"ghostty"}),
            )
        }
        fn copy(&mut self, p: &Payload) -> Result<()> {
            ensure!(!self.fail_copy, "copy failed");
            self.events
                .push(String::from_utf8(p.bytes.clone()).unwrap());
            Ok(())
        }
        fn paste(&mut self, target: &str, is_terminal: bool) -> Result<()> {
            assert_eq!(target, "original");
            assert!(is_terminal);
            self.events.push("paste".into());
            Ok(())
        }
        fn wait(&mut self, _: Duration) {}
    }
    fn payloads() -> Vec<Payload> {
        ["first", "second"]
            .map(|s| Payload {
                mime: "image/png".into(),
                bytes: s.as_bytes().to_vec(),
            })
            .into()
    }
    #[test]
    fn batch_preserves_selection_order_and_stops_on_focus_change() {
        let mut d = Fake::default();
        paste_batch(&mut d, "original", &payloads(), Duration::from_millis(800)).unwrap();
        assert_eq!(d.events, ["first", "paste", "second", "paste"]);
        let mut d = Fake {
            change_at: 3,
            ..Default::default()
        };
        assert!(paste_batch(&mut d, "original", &payloads(), Duration::from_millis(800)).is_err());
        assert_eq!(d.events, ["first", "paste"]);
        let mut d = Fake {
            change_at: 2,
            ..Default::default()
        };
        assert!(paste_batch(&mut d, "original", &payloads(), Duration::from_millis(800)).is_err());
        assert_eq!(d.events, ["first"]);
    }
    #[test]
    fn failed_copy_or_missing_target_never_sends_keys() {
        let mut d = Fake {
            fail_copy: true,
            ..Default::default()
        };
        assert!(paste_batch(&mut d, "original", &payloads(), Duration::from_secs(1)).is_err());
        assert!(d.events.is_empty());
        assert!(paste_batch(&mut d, "", &payloads(), Duration::from_secs(1)).is_err());
        assert_eq!(d.reads, 1);
    }
}
