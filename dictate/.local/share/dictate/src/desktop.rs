use crate::config::Config;
use anyhow::{Result, bail, ensure};
use serde_json::Value;
use std::{env, path::PathBuf, process::Stdio, sync::OnceLock, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

pub fn exists(name: &str) -> bool {
    env::split_paths(&env::var_os("PATH").unwrap_or_default()).any(|p| p.join(name).is_file())
}
pub async fn capture(command: &str, args: &[&str], ms: u64) -> Result<String> {
    let result = tokio::time::timeout(
        Duration::from_millis(ms),
        Command::new(command)
            .args(args)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(result.status.success(), "{command} failed");
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}
pub async fn hypr_json(command: &str) -> Result<Value> {
    Ok(serde_json::from_str(
        &capture("hyprctl", &[command, "-j"], 250).await?,
    )?)
}
pub async fn focused_monitor() -> Option<String> {
    let (window, monitors) = tokio::join!(hypr_json("activewindow"), hypr_json("monitors"));
    let window = window.ok();
    let monitors = monitors.ok()?;
    let monitors = monitors.as_array()?;
    let preferred = window.as_ref().and_then(|w| w.get("monitor"));
    monitors
        .iter()
        .find(|m| preferred.is_some_and(|id| m.get("id") == Some(id)))
        .or_else(|| monitors.iter().find(|m| m["focused"] == true))?
        .get("name")?
        .as_str()
        .map(str::to_owned)
}
static LUA: OnceLock<bool> = OnceLock::new();
pub async fn lua() -> bool {
    if let Some(v) = LUA.get() {
        return *v;
    }
    let yes = capture("hyprctl", &["status"], 250)
        .await
        .is_ok_and(|s| s.lines().any(|s| s.trim() == "configProvider: lua"));
    let _ = LUA.set(yes);
    yes
}
pub async fn paste(c: &Config) -> Result<()> {
    let method = c.get("PASTE_METHOD");
    if method == "none" {
        return Ok(());
    }
    let simple = match method {
        "wtype-ctrl-v" => Some(("wtype", vec!["-M", "ctrl", "-k", "v", "-m", "ctrl"])),
        "wtype-ctrl-shift-v" => Some((
            "wtype",
            vec![
                "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
            ],
        )),
        "ydotool-ctrl-v" => Some(("ydotool", vec!["key", "29:1", "47:1", "47:0", "29:0"])),
        "ydotool-ctrl-shift-v" => Some((
            "ydotool",
            vec!["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"],
        )),
        _ => None,
    };
    if let Some((cmd, args)) = simple {
        capture(cmd, &args, 2000).await?;
        return Ok(());
    }
    let window = active_window()
        .await
        .map_err(|e| anyhow::anyhow!("Automatic paste skipped: {e}"))?;
    let (mods, key) = match method {
        "hyprctl-super-v" => (
            if window.terminal {
                "CTRL SHIFT"
            } else {
                "CTRL"
            },
            "V",
        ),
        "hyprctl-ctrl-v" => ("CTRL", "V"),
        "hyprctl-ctrl-shift-v" => ("CTRL SHIFT", "V"),
        "hyprctl-shift-insert" => ("SHIFT", "Insert"),
        _ => bail!("Unknown PASTE_METHOD: {method}"),
    };
    send_shortcut(mods, key, &window.address).await
}

pub struct Window {
    pub address: String,
    pub terminal: bool,
}

pub async fn active_window() -> Result<Window> {
    let window = hypr_json("activewindow").await?;
    let class = window["class"].as_str().unwrap_or("");
    let address = window["address"].as_str().unwrap_or("");
    ensure!(
        !class.is_empty()
            && address.starts_with("0x")
            && u64::from_str_radix(&address[2..], 16).is_ok_and(|n| n > 0),
        "application focus is unknown"
    );
    ensure!(
        !(class == "Tk" && window["title"] == "Dictate") && class != "local.dictate.overlay",
        "overlay has focus"
    );
    let terminal = [
        "com.mitchellh.ghostty",
        "ghostty",
        "Alacritty",
        "alacritty",
        "kitty",
        "foot",
        "footclient",
        "org.wezfurlong.wezterm",
        "org.kde.konsole",
        "xterm",
        "URxvt",
        "st",
    ]
    .contains(&class)
        || class.contains("Ghostty");
    Ok(Window {
        address: address.into(),
        terminal,
    })
}

pub async fn send_shortcut(mods: &str, key: &str, address: &str) -> Result<()> {
    let target = format!("address:{address}");
    let result = if lua().await {
        let cmd = format!(
            "hl.dsp.send_shortcut({{ mods = {}, key = {}, window = {} }})",
            serde_json::to_string(mods)?,
            serde_json::to_string(key)?,
            serde_json::to_string(&target)?
        );
        capture("hyprctl", &["dispatch", &cmd], 2000).await?
    } else {
        capture(
            "hyprctl",
            &[
                "dispatch",
                "sendshortcut",
                &format!("{mods}, {key}, {target}"),
            ],
            2000,
        )
        .await?
    };
    ensure!(
        result
            .lines()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            == ["ok"],
        "Hyprland did not confirm the paste shortcut"
    );
    Ok(())
}
pub async fn copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let operation = async {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(text.as_bytes()).await?;
        drop(stdin);
        let status = child.wait().await?;
        ensure!(status.success(), "Clipboard copy failed");
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(Duration::from_secs(2), operation).await??;
    Ok(())
}
pub async fn read_clipboard(primary: bool) -> String {
    let mut args = vec!["--no-newline", "--type", "text"];
    if primary {
        args.insert(0, "--primary");
    }
    capture("wl-paste", &args, 500).await.unwrap_or_default()
}

/// Text that a spoken revision applies to, and the window it came from.
pub struct RevisionSource {
    pub window: Window,
    pub text: String,
}

/// Terminals use the clipboard, because Ctrl+A and Ctrl+C are shell keys
/// there and TUIs such as OpenCode copy their own mouse selections. Elsewhere,
/// copy the selection or else the whole field, leaving it selected so the
/// revised text can be pasted over it.
pub async fn revision_source(c: &Config) -> Result<RevisionSource> {
    let window = active_window()
        .await
        .map_err(|e| anyhow::anyhow!("Revision skipped: {e}"))?;
    if window.terminal {
        let text = read_clipboard(false).await;
        ensure!(
            !text.trim().is_empty(),
            "Copy the text to revise before using it in a terminal"
        );
        return Ok(RevisionSource { window, text });
    }
    let wait = Duration::from_millis(c.millis("REVISE_COPY_TIMEOUT_MS"));
    let text = match copy_from(&window, false, wait).await? {
        Some(selection) => selection,
        None => copy_from(&window, true, wait).await?.unwrap_or_default(),
    };
    Ok(RevisionSource { window, text })
}

async fn copy_from(window: &Window, select_all: bool, wait: Duration) -> Result<Option<String>> {
    let marker = format!("dictate-revision-{}", crate::storage::generation());
    copy(&marker).await?;
    if select_all {
        send_shortcut("CTRL", "A", &window.address).await?;
    }
    send_shortcut("CTRL", "C", &window.address).await?;
    let until = tokio::time::Instant::now() + wait;
    loop {
        let text = read_clipboard(false).await;
        if !text.is_empty() && text != marker {
            return Ok(Some(text));
        }
        if tokio::time::Instant::now() >= until {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// One short line describing the revision source for the overlay.
pub fn source_preview(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        return "(empty field)".into();
    }
    let mut preview: String = line.chars().take(48).collect();
    if preview.len() < line.len() || text.trim().lines().nth(1).is_some() {
        preview.push('…');
    }
    preview
}

pub async fn notify(title: &str, text: &str) {
    if exists("notify-send") {
        let _ = capture("notify-send", &["-a", "Dictate", title, text], 1500).await;
    }
}

/// Shared state is compatible with the browser dictation client.
pub async fn duck(c: &Config, start: bool) -> Result<()> {
    crate::volume::duck(c, start).await
}
pub fn overlay_path() -> PathBuf {
    std::env::current_exe()
        .unwrap_or_default()
        .with_file_name("dictate-overlay")
}

#[cfg(test)]
mod tests {
    use super::source_preview;

    #[test]
    fn source_preview_shows_first_line_and_marks_more_text() {
        assert_eq!(source_preview(""), "(empty field)");
        assert_eq!(source_preview("  Short line \n"), "Short line");
        assert_eq!(source_preview("\nFirst\nSecond"), "First…");
        assert_eq!(
            source_preview(&"a".repeat(60)),
            format!("{}…", "a".repeat(48))
        );
    }
}
