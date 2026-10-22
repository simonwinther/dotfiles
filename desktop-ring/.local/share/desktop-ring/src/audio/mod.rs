use crate::core::Event;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioState {
    pub outputs: Vec<Device>,
    pub inputs: Vec<Device>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub identity: String,
    pub label: String,
    #[serde(default)]
    pub short_label: String,
    pub available: bool,
    pub active: bool,
    pub muted: bool,
    pub volume: u32,
}
async fn pactl(args: &[&str]) -> Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new("pactl")
            .args(args)
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("PipeWire request timed out")??;
    ensure!(
        output.status.success(),
        "PipeWire: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8(output.stdout)?)
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn parse_devices(rows: &Value, default: &str, input: bool) -> Vec<Device> {
    let Some(rows) = rows.as_array() else {
        return vec![];
    };
    let mut devices: Vec<_> = rows
        .iter()
        .filter(|v| {
            !input
                || (!text(v, "name").ends_with(".monitor")
                    && text(&v["properties"], "device.class") != "monitor"
                    && !text(v, "name").starts_with("alsa_input.hw_"))
        })
        .map(|v| {
            let name = text(v, "name");
            let props = &v["properties"];
            let persistent = [
                "device.serial",
                "api.bluez5.address",
                "device.bus-id",
                "device.bus_path",
            ]
            .iter()
            .find_map(|k| props.get(*k).and_then(Value::as_str))
            .unwrap_or(name);
            let identity =
                serde_json::to_string(&(persistent, text(props, "device.profile.name"), name))
                    .unwrap();
            let active_port = text(v, "active_port");
            let unavailable = v["ports"]
                .as_array()
                .and_then(|ports| ports.iter().find(|p| text(p, "name") == active_port))
                .is_some_and(|p| ["not available", "no"].contains(&text(p, "availability")));
            let volume = v["volume"]
                .as_object()
                .map(|channels| {
                    channels
                        .values()
                        .filter_map(|c| c["value"].as_f64())
                        .fold(0.0, f64::max)
                        / 65536.0
                        * 100.0
                })
                .unwrap_or(0.0)
                .round() as u32;
            Device {
                name: name.into(),
                identity,
                label: text(v, "description").into(),
                short_label: {
                    let full = text(v, "description");
                    let nick = text(props, "node.nick");
                    let profile = text(props, "device.profile.description");
                    if full.chars().count() > 32 && !nick.is_empty() && !profile.is_empty() {
                        format!("{nick}\n{profile}")
                    } else {
                        full.into()
                    }
                },
                available: !unavailable,
                active: name == default,
                muted: v["mute"].as_bool().unwrap_or(false),
                volume,
            }
        })
        .collect();
    devices.sort_by(|a, b| a.label.cmp(&b.label));
    devices
}
pub async fn discover() -> Result<AudioState> {
    let (info, sinks, sources) = tokio::try_join!(
        pactl(&["--format=json", "info"]),
        pactl(&["--format=json", "list", "sinks"]),
        pactl(&["--format=json", "list", "sources"])
    )?;
    let info: Value = serde_json::from_str(&info)?;
    Ok(AudioState {
        outputs: parse_devices(
            &serde_json::from_str(&sinks)?,
            text(&info, "default_sink_name"),
            false,
        ),
        inputs: parse_devices(
            &serde_json::from_str(&sources)?,
            text(&info, "default_source_name"),
            true,
        ),
        error: None,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AudioAction {
    Select { input: bool, name: String },
    Volume { input: bool, delta: i32 },
    Mute { input: bool },
}
pub async fn act(action: &AudioAction, state: &AudioState, move_streams: bool) -> Result<()> {
    let input = match action {
        AudioAction::Select { input, .. }
        | AudioAction::Volume { input, .. }
        | AudioAction::Mute { input } => *input,
    };
    let devices = if input { &state.inputs } else { &state.outputs };
    let kind = if input { "source" } else { "sink" };
    match action {
        AudioAction::Select { name, .. } => {
            let device = devices
                .iter()
                .find(|d| d.name == *name)
                .context("audio device disconnected")?;
            ensure!(device.available, "audio route is unavailable");
            pactl(&[&format!("set-default-{kind}"), name]).await?;
            if move_streams {
                let streams = if input {
                    "source-outputs"
                } else {
                    "sink-inputs"
                };
                let data: Value =
                    serde_json::from_str(&pactl(&["--format=json", "list", streams]).await?)?;
                for stream in data.as_array().into_iter().flatten() {
                    // WirePlumber's internal split-device streams must never be rerouted.
                    if stream["properties"]["media.role"].as_str() == Some("filter")
                        || stream["properties"]["node.virtual"].as_str() == Some("true")
                    {
                        continue;
                    }
                    if let Some(id) = stream["index"].as_u64() {
                        let cmd = if input {
                            "move-source-output"
                        } else {
                            "move-sink-input"
                        };
                        if let Err(e) = pactl(&[cmd, &id.to_string(), name]).await {
                            eprintln!("stream {id}: {e}");
                        }
                    }
                }
            }
        }
        AudioAction::Volume { delta, .. } => {
            let d = devices
                .iter()
                .find(|d| d.active)
                .context("no default audio device")?;
            let volume = (d.volume as i32 + delta).clamp(0, 100);
            pactl(&[
                &format!("set-{kind}-volume"),
                &d.name,
                &format!("{volume}%"),
            ])
            .await?;
        }
        AudioAction::Mute { .. } => {
            let d = devices
                .iter()
                .find(|d| d.active)
                .context("no default audio device")?;
            pactl(&[&format!("set-{kind}-mute"), &d.name, "toggle"]).await?;
        }
    }
    Ok(())
}
pub async fn watch(tx: mpsc::Sender<Event>) {
    let mut backoff = 1;
    loop {
        let result: Result<()> = async {
            let mut child = Command::new("pactl")
                .arg("subscribe")
                .env("LC_ALL", "C")
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
            tx.send(Event::Audio).await?;
            while let Some(line) = lines.next_line().await? {
                if [" on sink #", " on source #", " on server #", " on card #"]
                    .iter()
                    .any(|s| line.contains(s))
                {
                    tx.send(Event::Audio).await?;
                }
            }
            let status = child.wait().await?;
            anyhow::bail!("audio subscription ended: {status}")
        }
        .await;
        if tx.is_closed() {
            return;
        }
        eprintln!("audio: {result:?}; reconnect in {backoff}s");
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}
