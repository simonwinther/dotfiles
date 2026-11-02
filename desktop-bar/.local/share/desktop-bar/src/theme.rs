use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub style: String,
    pub font: String,
    pub scale: f32,
    pub light: bool,
    pub palette: HashMap<String, String>,
    pub outputs: Outputs,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Outputs {
    pub full: Vec<String>,
    pub minimal: Vec<String>,
}

impl Default for Outputs {
    fn default() -> Self {
        Self {
            full: vec!["*".into()],
            minimal: vec![],
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            style: "native-clean".into(),
            font: "JetBrainsMono Nerd Font".into(),
            scale: 1.0,
            light: false,
            palette: HashMap::new(),
            outputs: Outputs::default(),
        }
    }
}

impl Theme {
    pub fn read() -> Self {
        let mut value: Self = std::fs::read(runtime().join("desktop/bar-theme.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if let Ok(scale) = std::env::var("DESKTOP_BAR_SCALE")
            && let Ok(scale) = scale.parse()
        {
            value.scale = scale;
        }
        if !value.scale.is_finite() {
            value.scale = 1.0;
        }
        value.scale = value.scale.clamp(0.5, 3.0);
        value
    }
    pub fn motion(&self) -> bool {
        self.style == "native-motion"
    }
    pub fn color(&self, key: &str, fallback: &str) -> Color {
        self.palette
            .get(key)
            .and_then(|v| Color::parse(v))
            .unwrap_or_else(|| Color::parse(fallback).unwrap())
    }
    pub fn text(&self) -> Color {
        self.color("foreground", if self.light { "#4c4f69" } else { "#f4f4f5" })
    }
    pub fn accent(&self) -> Color {
        self.color("accent", "#635bff")
    }
    pub fn blue(&self) -> Color {
        Color::hex(if self.light { "#1e66f5" } else { "#60a5fa" })
    }
    pub fn green(&self) -> Color {
        Color::hex(if self.light { "#40a02b" } else { "#4ade80" })
    }
    pub fn yellow(&self) -> Color {
        Color::hex(if self.light { "#df8e1d" } else { "#eab308" })
    }
    pub fn red(&self) -> Color {
        Color::hex(if self.light { "#d20f39" } else { "#f87171" })
    }
    pub fn dim(&self) -> Color {
        self.text().alpha(0.55)
    }
    pub fn pill(&self) -> Color {
        if self.light {
            Color::hex("#ffffff").alpha(0.70)
        } else {
            Color::hex("#18171f").alpha(0.82)
        }
    }
    pub fn pill_hover(&self) -> Color {
        if self.light {
            Color::hex("#ffffff").alpha(0.88)
        } else {
            Color::hex("#2d2a3a").alpha(0.90)
        }
    }
    pub fn outline(&self) -> Color {
        Color::hex("#ffffff").alpha(if self.light { 0.55 } else { 0.10 })
    }
    pub fn divider(&self) -> Color {
        if self.light {
            Color::hex("#6c6f85").alpha(0.17)
        } else {
            Color::hex("#ffffff").alpha(0.13)
        }
    }
    pub fn minimal(&self, name: &str) -> bool {
        self.outputs.minimal.iter().any(|p| p == name)
    }
    pub fn selected(&self, name: &str, description: &str) -> bool {
        self.minimal(name) || matches_output(&self.outputs.full, name, description)
    }
}

pub fn matches_output(patterns: &[String], name: &str, description: &str) -> bool {
    let matches =
        |p: &str| p == "*" || p == name || p.strip_prefix("desc:").unwrap_or(p) == description;
    if patterns
        .iter()
        .any(|p| p.strip_prefix('!').is_some_and(matches))
    {
        return false;
    }
    !patterns.is_empty()
        && (patterns.iter().all(|p| p.starts_with('!'))
            || patterns.iter().any(|p| !p.starts_with('!') && matches(p)))
}

pub fn runtime() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })))
}
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}
pub fn config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);
impl Color {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        if s.len() != 6 {
            return None;
        }
        let n = u32::from_str_radix(s, 16).ok()?;
        Some(Self(
            ((n >> 16) & 255) as f32 / 255.0,
            ((n >> 8) & 255) as f32 / 255.0,
            (n & 255) as f32 / 255.0,
            1.0,
        ))
    }
    pub fn hex(s: &str) -> Self {
        Self::parse(s).unwrap()
    }
    pub fn alpha(self, a: f32) -> Self {
        Self(self.0, self.1, self.2, self.3 * a.clamp(0.0, 1.0))
    }
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self(
            self.0 + (other.0 - self.0) * t,
            self.1 + (other.1 - self.1) * t,
            self.2 + (other.2 - self.2) * t,
            self.3 + (other.3 - self.3) * t,
        )
    }
    pub fn skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(self.0, self.1, self.2, self.3).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_palette_values_fall_back_and_exclusions_win() {
        assert!(Color::parse("#ff0000").is_some());
        assert!(Color::parse("#xyz").is_none());
        assert!(Color::parse("#fffffff").is_none());
        assert!(!matches_output(
            &["*".into(), "!eDP-1".into()],
            "eDP-1",
            "Laptop"
        ));
        assert!(matches_output(&["desc:Laptop".into()], "eDP-1", "Laptop"));
        assert!(!matches_output(&[], "eDP-1", "Laptop"));
    }
}
