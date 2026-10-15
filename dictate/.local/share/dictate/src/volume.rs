use crate::{
    config::{self, Config},
    desktop, storage,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    os::fd::AsRawFd,
    path::PathBuf,
    time::Duration,
};
use tokio::time::{Instant, sleep, sleep_until};

fn root() -> PathBuf {
    config::runtime_dir()
        .parent()
        .unwrap()
        .join("dictation-volume-duck")
}
async fn lock() -> Result<File> {
    storage::private_dir(&root())?;
    let file = storage::private_file(&root().join("lock"), true)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(file);
        }
        ensure!(Instant::now() < deadline, "Volume state is busy");
        sleep(Duration::from_millis(20)).await;
    }
}
fn load() -> BTreeMap<String, String> {
    fs::read_to_string(root().join("state.env"))
        .map(|s| config::env_values(&s))
        .unwrap_or_default()
}
fn get<'a>(state: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    state.get(key).map(String::as_str).unwrap_or("")
}
fn save(state: &BTreeMap<String, String>) -> Result<()> {
    let text = state
        .iter()
        .map(|(k, v)| format!("{k}={}\n", shell_words::quote(v)))
        .collect::<String>();
    storage::write_private(&root().join("state.env"), text)
}
async fn probe(backend: &str, sink: &str, identity: &str) -> Result<BTreeMap<String, String>> {
    if (backend.is_empty() || backend == "wpctl") && desktop::exists("wpctl") {
        let output = desktop::capture(
            "wpctl",
            &[
                "inspect",
                if sink.is_empty() {
                    "@DEFAULT_AUDIO_SINK@"
                } else {
                    sink
                },
            ],
            750,
        )
        .await?;
        let resolved = output
            .lines()
            .find_map(|s| s.strip_prefix("id ")?.split(',').next())
            .context("No default audio sink")?
            .trim();
        let serial = output
            .lines()
            .find_map(|s| {
                let (key, value) = s.trim().split_once('=')?;
                if key.trim().trim_start_matches('*').trim() == "object.serial" {
                    Some(value.trim().trim_matches('"'))
                } else {
                    None
                }
            })
            .context("No stable audio sink identity")?;
        ensure!(
            identity.is_empty() || identity == serial,
            "Audio sink identity changed; saved volume retained"
        );
        let volume = desktop::capture("wpctl", &["get-volume", resolved], 750).await?;
        let value = volume
            .split_whitespace()
            .nth(1)
            .context("No audio volume")?
            .parse::<f64>()?;
        ensure!(value.is_finite() && value >= 0., "Invalid audio volume");
        return Ok([
            ("ORIGINAL_BACKEND", "wpctl".into()),
            ("ORIGINAL_SINK", resolved.into()),
            ("ORIGINAL_SINK_IDENTITY", serial.into()),
            ("ORIGINAL_VOLUME", value.to_string()),
            (
                "ORIGINAL_MUTED",
                if volume.contains("[MUTED]") { "1" } else { "0" }.into(),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect());
    }
    ensure!(
        (backend.is_empty() || backend == "pactl") && desktop::exists("pactl"),
        "No supported volume backend"
    );
    let sink = if sink.is_empty() {
        desktop::capture("pactl", &["get-default-sink"], 750)
            .await?
            .trim()
            .to_owned()
    } else {
        sink.to_owned()
    };
    ensure!(
        !sink.is_empty() && !sink.starts_with('@'),
        "No stable PulseAudio sink"
    );
    let volume = desktop::capture("pactl", &["get-sink-volume", &sink], 750).await?;
    let value = volume
        .split_whitespace()
        .find_map(|w| w.strip_suffix('%')?.parse::<f64>().ok())
        .context("No PulseAudio volume")?
        / 100.;
    let mute = desktop::capture("pactl", &["get-sink-mute", &sink], 750).await?;
    Ok([
        ("ORIGINAL_BACKEND", "pactl".into()),
        ("ORIGINAL_SINK", sink),
        ("ORIGINAL_SINK_IDENTITY", String::new()),
        ("ORIGINAL_VOLUME", value.to_string()),
        (
            "ORIGINAL_MUTED",
            if mute.contains("yes") { "1" } else { "0" }.into(),
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect())
}
async fn set(backend: &str, sink: &str, volume: f64) -> Result<()> {
    let value = if backend == "pactl" {
        format!("{:.3}%", volume * 100.)
    } else {
        format!("{volume:.4}")
    };
    desktop::capture(
        backend,
        &[
            if backend == "pactl" {
                "set-sink-volume"
            } else {
                "set-volume"
            },
            sink,
            &value,
        ],
        750,
    )
    .await?;
    Ok(())
}
async fn fade(
    backend: &str,
    sink: &str,
    start: f64,
    end: f64,
    millis: u64,
    steps: u64,
) -> Result<()> {
    let steps = if millis == 0 { 1 } else { steps.clamp(1, 100) };
    let began = Instant::now();
    for i in 1..=steps {
        set(
            backend,
            sink,
            start + (end - start) * i as f64 / steps as f64,
        )
        .await?;
        if i < steps {
            sleep_until(began + Duration::from_millis(millis * i / steps)).await;
        }
    }
    Ok(())
}
async fn restore(state: &BTreeMap<String, String>, c: &Config) -> Result<()> {
    let backend = get(state, "ORIGINAL_BACKEND");
    let sink = get(state, "ORIGINAL_SINK");
    let identity = get(state, "ORIGINAL_SINK_IDENTITY");
    ensure!(
        !sink.is_empty() && (backend != "wpctl" || !identity.is_empty()),
        "Saved audio state has no stable sink identity"
    );
    let current = probe(backend, sink, identity).await?;
    let current_volume = get(&current, "ORIGINAL_VOLUME").parse::<f64>()?;
    let original = get(state, "ORIGINAL_VOLUME").parse::<f64>()?;
    ensure!(
        original.is_finite() && original >= 0.,
        "Invalid saved audio volume"
    );
    fade(
        backend,
        sink,
        current_volume,
        original,
        c.millis("VOLUME_DUCK_FADE_OUT_MS"),
        c.millis("VOLUME_DUCK_FADE_STEPS"),
    )
    .await?;
    desktop::capture(
        backend,
        &[
            if backend == "pactl" {
                "set-sink-mute"
            } else {
                "set-mute"
            },
            sink,
            get(state, "ORIGINAL_MUTED"),
        ],
        750,
    )
    .await?;
    Ok(())
}
pub async fn duck(c: &Config, start: bool) -> Result<()> {
    if start && !c.yes("DUCK_VOLUME") {
        return Ok(());
    }
    if !start && !root().join("state.env").is_file() {
        return Ok(());
    }
    let _guard = lock().await?;
    let mut state = load();
    let mut clients = get(&state, "CLIENTS")
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if start {
        if !clients.is_empty() {
            if !clients.iter().any(|v| v == "dictate") {
                clients.push("dictate".into());
                state.insert("CLIENTS".into(), clients.join(" "));
                save(&state)?;
            }
            return Ok(());
        }
        if !get(&state, "ORIGINAL_BACKEND").is_empty() {
            restore(&state, c).await?;
        }
        state = probe("", "", "").await?;
        state.insert("CLIENTS".into(), "dictate".into());
        save(&state)?;
        let backend = get(&state, "ORIGINAL_BACKEND");
        let sink = get(&state, "ORIGINAL_SINK");
        let original = get(&state, "ORIGINAL_VOLUME").parse::<f64>()?;
        if let Err(error) = fade(
            backend,
            sink,
            original,
            original * c.number("VOLUME_DUCK_FACTOR"),
            c.millis("VOLUME_DUCK_FADE_IN_MS"),
            c.millis("VOLUME_DUCK_FADE_STEPS"),
        )
        .await
        {
            state.insert("CLIENTS".into(), String::new());
            save(&state)?;
            if restore(&state, c).await.is_ok() {
                let _ = fs::remove_file(root().join("state.env"));
            }
            return Err(error);
        }
    } else {
        if !clients.is_empty() && !clients.iter().any(|v| v == "dictate") {
            return Ok(());
        }
        clients.retain(|v| v != "dictate");
        state.insert("CLIENTS".into(), clients.join(" "));
        save(&state)?;
        if clients.is_empty() && !get(&state, "ORIGINAL_BACKEND").is_empty() {
            restore(&state, c).await?;
            fs::remove_file(root().join("state.env"))?;
        }
    }
    Ok(())
}
