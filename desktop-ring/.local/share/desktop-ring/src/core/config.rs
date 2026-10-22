use crate::monitors::{FixedProfile, MonitorPreference};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub monitors: bool,
    pub laptop: bool,
    pub audio: bool,
    pub bluetooth: bool,
    pub power: bool,
    pub move_streams: bool,
    pub internal_scale: f64,
    pub fixed_profiles: Vec<FixedProfile>,
    pub laptop_preferences: Vec<MonitorPreference>,
    pub theme: Theme,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            monitors: true,
            laptop: false,
            audio: true,
            bluetooth: true,
            power: true,
            move_streams: false,
            internal_scale: 1.0,
            fixed_profiles: vec![],
            laptop_preferences: vec![],
            theme: Theme::default(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub font: String,
    pub icon_font: String,
    pub radius: f64,
    pub animation_ms: u32,
    pub follow_desktop_palette: bool,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#1a1b26".into(),
            foreground: "#c0caf5".into(),
            accent: "#7aa2f7".into(),
            font: "Adwaita Sans".into(),
            icon_font: "Symbols Nerd Font".into(),
            radius: 214.0,
            animation_ms: 140,
            follow_desktop_palette: true,
        }
    }
}
pub fn config_path() -> PathBuf {
    std::env::var_os("DESKTOP_RING_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| base("XDG_CONFIG_HOME", ".config").join("desktop/ring.json"))
}
pub fn base(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").expect("HOME must be set")).join(fallback)
    })
}
pub fn load() -> anyhow::Result<Config> {
    let path = config_path();
    let c: Config = serde_json::from_slice(
        &std::fs::read(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?,
    )?;
    anyhow::ensure!(
        c.internal_scale.is_finite() && (0.5..=4.0).contains(&c.internal_scale),
        "invalid internal_scale"
    );
    anyhow::ensure!(
        c.theme.radius.is_finite() && (150.0..=400.0).contains(&c.theme.radius),
        "radius must be 150..400 logical pixels"
    );
    for p in &c.fixed_profiles {
        p.validate()?;
    }
    Ok(c)
}
