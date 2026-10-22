use crate::command::{self, Deadline};
use anyhow::{Context, Result, bail, ensure};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Backend {
    Wpctl,
    Pactl,
}

impl Backend {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "wpctl" => Ok(Self::Wpctl),
            "pactl" => Ok(Self::Pactl),
            _ => bail!("Unknown saved audio backend: {value}"),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Wpctl => "wpctl",
            Self::Pactl => "pactl",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Audio {
    pub backend: Backend,
    pub sink: String,
    pub identity: String,
    pub volume: f64,
    pub muted: bool,
}

pub fn volume(value: &str) -> Result<f64> {
    let value = value.parse::<f64>().context("Invalid audio volume")?;
    ensure!(value.is_finite() && value >= 0., "Invalid audio volume");
    Ok(value)
}

fn wpctl_sink(sink: &str, expected: &str, deadline: &Deadline) -> Result<(String, String)> {
    let output = command::capture("wpctl", &["inspect", sink], deadline)?;
    let id = output
        .lines()
        .find_map(|s| s.strip_prefix("id ")?.split(',').next())
        .context("No audio sink ID")?
        .trim()
        .to_owned();
    let identity = output
        .lines()
        .find_map(|s| {
            let (key, value) = s.split_once('=')?;
            (key.trim().trim_start_matches('*').trim() == "object.serial")
                .then(|| value.trim().trim_matches('"').to_owned())
        })
        .context("No stable audio sink identity")?;
    ensure!(
        !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()) && !identity.is_empty(),
        "Invalid audio sink identity"
    );
    ensure!(
        expected.is_empty() || identity == expected,
        "Audio sink identity changed; saved volume retained"
    );
    Ok((id, identity))
}

impl Audio {
    pub fn current(deadline: &Deadline) -> Result<Self> {
        if command::exists("wpctl") {
            let (sink, identity) = wpctl_sink("@DEFAULT_AUDIO_SINK@", "", deadline)?;
            let (volume, muted) = Self::read(&Backend::Wpctl, &sink, deadline)?;
            Ok(Self {
                backend: Backend::Wpctl,
                sink,
                identity,
                volume,
                muted,
            })
        } else if command::exists("pactl") {
            let sink = command::capture("pactl", &["get-default-sink"], deadline)?;
            ensure!(
                !sink.is_empty() && !sink.starts_with('@'),
                "No default audio sink"
            );
            let (volume, muted) = Self::read(&Backend::Pactl, &sink, deadline)?;
            Ok(Self {
                backend: Backend::Pactl,
                sink,
                identity: String::new(),
                volume,
                muted,
            })
        } else {
            bail!("Install wpctl (WirePlumber) or pactl to control audio");
        }
    }

    fn read(backend: &Backend, sink: &str, deadline: &Deadline) -> Result<(f64, bool)> {
        match backend {
            Backend::Wpctl => {
                let output = command::capture("wpctl", &["get-volume", sink], deadline)?;
                let value = output
                    .split_whitespace()
                    .nth(1)
                    .context("No audio volume")?;
                Ok((volume(value)?, output.contains("[MUTED]")))
            }
            Backend::Pactl => {
                let output = command::capture("pactl", &["get-sink-volume", sink], deadline)?;
                let value = output
                    .split_whitespace()
                    .find_map(|s| s.strip_suffix('%'))
                    .context("No PulseAudio volume")?;
                let volume = volume(value)? / 100.;
                let output = command::capture("pactl", &["get-sink-mute", sink], deadline)?;
                let muted = match output.split_whitespace().last() {
                    Some("yes") => true,
                    Some("no") => false,
                    _ => bail!("Invalid audio mute status"),
                };
                Ok((volume, muted))
            }
        }
    }

    // Old Bash state had no sink fields. Resolve it once, preserving saved volume.
    pub fn bind_legacy(&mut self, deadline: &Deadline) -> Result<()> {
        if self.sink.is_empty() {
            match self.backend {
                Backend::Wpctl => {
                    (self.sink, self.identity) = wpctl_sink("@DEFAULT_AUDIO_SINK@", "", deadline)?;
                }
                Backend::Pactl => {
                    self.sink = command::capture("pactl", &["get-default-sink"], deadline)?;
                }
            }
        }
        ensure!(
            !self.sink.is_empty() && !self.sink.starts_with('@'),
            "Invalid saved audio sink"
        );
        ensure!(
            self.backend != Backend::Wpctl || !self.identity.is_empty(),
            "Missing saved sink identity"
        );
        Ok(())
    }

    pub fn check_sink(&self, deadline: &Deadline) -> Result<()> {
        if self.backend == Backend::Wpctl {
            wpctl_sink(&self.sink, &self.identity, deadline)?;
        }
        Ok(())
    }

    pub fn set_volume(&self, value: f64, deadline: &Deadline) -> Result<()> {
        let value = match self.backend {
            Backend::Wpctl => format!("{value:.4}"),
            Backend::Pactl => format!("{:.3}%", value * 100.),
        };
        let action = if self.backend == Backend::Wpctl {
            "set-volume"
        } else {
            "set-sink-volume"
        };
        command::capture(self.backend.name(), &[action, &self.sink, &value], deadline)?;
        Ok(())
    }

    pub fn set_mute(&self, deadline: &Deadline) -> Result<()> {
        let action = if self.backend == Backend::Wpctl {
            "set-mute"
        } else {
            "set-sink-mute"
        };
        command::capture(
            self.backend.name(),
            &[action, &self.sink, if self.muted { "1" } else { "0" }],
            deadline,
        )?;
        Ok(())
    }

    pub fn fade(
        &self,
        from: f64,
        to: f64,
        millis: u64,
        steps: u64,
        deadline: &Deadline,
    ) -> Result<()> {
        ensure!(to.is_finite() && to >= 0., "Invalid target volume");
        let steps = if millis == 0 { 1 } else { steps.clamp(1, 100) };
        let began = Instant::now();
        for step in 1..=steps {
            self.set_volume(from + (to - from) * step as f64 / steps as f64, deadline)?;
            if step < steps {
                let target = began + Duration::from_millis(millis * step / steps);
                if let Some(delay) = target.checked_duration_since(Instant::now()) {
                    deadline.sleep(delay)?;
                }
            }
        }
        Ok(())
    }

    pub fn restore(&self, millis: u64, steps: u64, deadline: &Deadline) -> Result<()> {
        self.check_sink(deadline)?;
        let (current, _) = Self::read(&self.backend, &self.sink, deadline)?;
        self.fade(current, self.volume, millis, steps, deadline)?;
        self.set_mute(deadline)
    }
}
