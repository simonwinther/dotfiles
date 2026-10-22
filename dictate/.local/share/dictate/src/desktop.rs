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
    let window = hypr_json("activewindow").await?;
    let class = window["class"].as_str().unwrap_or("");
    let address = window["address"].as_str().unwrap_or("");
    ensure!(
        !class.is_empty()
            && address.starts_with("0x")
            && u64::from_str_radix(&address[2..], 16).is_ok_and(|n| n > 0),
        "Automatic paste skipped: application focus is unknown"
    );
    ensure!(
        !(class == "Tk" && window["title"] == "Dictate") && class != "local.dictate.overlay",
        "Automatic paste skipped: overlay has focus"
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
    let (mods, key) = match method {
        "hyprctl-super-v" => (if terminal { "CTRL SHIFT" } else { "CTRL" }, "V"),
        "hyprctl-ctrl-v" => ("CTRL", "V"),
        "hyprctl-ctrl-shift-v" => ("CTRL SHIFT", "V"),
        "hyprctl-shift-insert" => ("SHIFT", "Insert"),
        _ => bail!("Unknown PASTE_METHOD: {method}"),
    };
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
