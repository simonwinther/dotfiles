use crate::{home, process};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Settings {
    target: String,
    #[serde(default)]
    name: String,
}

fn settings() -> Result<Settings> {
    let config = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".config"));
    let bytes = fs::read(config.join("taildrop/iphone.json"))
        .context("Could not read your iPhone settings")?;
    ensure!(bytes.len() <= 65_536, "iPhone settings file is too large");
    let settings: Settings =
        serde_json::from_slice(&bytes).context("Could not read your iPhone settings")?;
    ensure!(
        !settings.target.is_empty() && !settings.target.contains('\0'),
        "The iPhone address is missing or invalid"
    );
    Ok(settings)
}

pub fn check() -> Result<()> {
    let settings = settings()?;
    let output = process::capture(
        Command::new("tailscale").args(["status", "--json"]),
        Some(Duration::from_secs(15)),
    )?;
    ensure!(
        output.status.success(),
        "Could not read Tailscale status: {}",
        process::detail(&output)
    );
    let state: Value =
        serde_json::from_slice(&output.stdout).context("Invalid Tailscale status")?;
    ensure!(
        state["BackendState"] == "Running",
        "Tailscale is not running"
    );
    let peer = state["Peer"]
        .as_object()
        .and_then(|peers| {
            peers.values().find(|peer| {
                peer["TailscaleIPs"]
                    .as_array()
                    .is_some_and(|ips| ips.iter().any(|ip| ip.as_str() == Some(&settings.target)))
            })
        })
        .context("The configured iPhone is not in your active tailnet")?;
    let name = peer["DNSName"]
        .as_str()
        .unwrap_or("")
        .split('.')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&settings.name);
    println!("iPhone: {name} ({})", settings.target);
    println!(
        "Online: {}",
        if peer["Online"].as_bool().unwrap_or(false) {
            "True"
        } else {
            "False"
        }
    );
    Ok(())
}

struct Progress(Child);

impl Progress {
    fn start(count: usize) -> Result<Self> {
        Ok(Self(
            Command::new("zenity")
                .args([
                    "--progress",
                    "--pulsate",
                    "--auto-close",
                    "--no-cancel",
                    "--title=Send to iPhone",
                ])
                .arg(format!("--text=Sending {count} file(s) to your iPhone..."))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("Could not show transfer progress")?,
        ))
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.0.stdin.take() {
            let _ = stdin.write_all(b"100\n");
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub fn send(mut selected: Vec<OsString>) -> Result<()> {
    let settings = settings()?;
    if selected.is_empty() {
        let output = process::capture(
            Command::new("zenity")
                .args([
                    "--file-selection",
                    "--multiple",
                    "--separator=\u{1f}",
                    "--title=Choose files to send to iPhone",
                    "--width=960",
                    "--height=720",
                ])
                .arg(format!("--filename={}/", home()?.display())),
            None,
        )?;
        if !output.status.success() {
            return Ok(());
        }
        let mut bytes = output.stdout;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        selected = bytes
            .split(|c| *c == 0x1f)
            .filter(|b| !b.is_empty())
            .map(|b| OsString::from_vec(b.to_vec()))
            .collect();
        if selected.is_empty() {
            return Ok(());
        }
    }
    let cwd = env::current_dir()?;
    let paths = selected.iter().map(|p| cwd.join(p)).collect::<Vec<_>>();
    let invalid = paths
        .iter()
        .filter(|p| !p.is_file())
        .take(5)
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>();
    ensure!(
        invalid.is_empty(),
        "Choose files to send. Folders cannot be sent with Taildrop.\n\n{}",
        invalid.join("\n")
    );
    let progress = Progress::start(paths.len())?;
    let result = process::capture(
        Command::new("tailscale")
            .args(["file", "cp", "--update-interval=0"])
            .args(&paths)
            .arg(format!("{}:", settings.target)),
        None,
    );
    drop(progress);
    let output = result?;
    ensure!(
        output.status.success(),
        "The transfer did not finish. Make sure Tailscale is connected on your iPhone, then try again.\n\n{}",
        process::detail(&output)
    );
    let _ = process::capture(Command::new("notify-send")
        .args(["--app-name=File Sharing", "--icon=phone", "Sent to iPhone"])
        .arg(format!("{} file(s) sent. Open the Tailscale notification on your iPhone to find them in Files.", paths.len())), Some(Duration::from_secs(3)));
    Ok(())
}

pub fn error_dialog(message: &str) {
    let _ = process::capture(
        Command::new("zenity")
            .args([
                "--error",
                "--no-markup",
                "--title=Send to iPhone",
                "--width=500",
            ])
            .arg(format!("--text={message}")),
        None,
    );
}

pub fn is_check(args: &[OsString]) -> bool {
    args == [OsStr::new("--check")]
}
