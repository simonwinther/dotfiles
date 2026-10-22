mod identity;
pub mod ipc;
mod layout;
pub use identity::{Identity, setup_key, usable_serial};
pub use layout::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Monitor {
    pub name: String,
    pub make: String,
    pub model: String,
    pub serial: String,
    pub description: String,
    pub edid: Option<String>,
    pub width: u32,
    pub height: u32,
    #[serde(rename = "refreshRate")]
    pub refresh: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub transform: u8,
    pub disabled: bool,
    pub focused: bool,
    #[serde(rename = "availableModes")]
    pub modes: Vec<String>,
}
impl Monitor {
    pub fn internal(&self) -> bool {
        self.name.starts_with("eDP-")
            || self.name.starts_with("LVDS-")
            || self.name.starts_with("DSI-")
    }
    pub fn enrich(&mut self) {
        self.edid = identity::read_edid(&self.name);
    }
}
