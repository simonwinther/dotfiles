use super::Monitor;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn usable_serial(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    !s.is_empty()
        && ![
            "unknown",
            "none",
            "null",
            "n/a",
            "default",
            "123456",
            "123456789",
            "01010101",
        ]
        .contains(&s.as_str())
        && s.trim_start_matches("0x")
            .chars()
            .any(|c| c != '0' && !c.is_whitespace())
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Identity {
    pub make: String,
    pub model: String,
    pub serial: Option<String>,
    pub edid: Option<String>,
}
impl Identity {
    pub fn score(&self, m: &Monitor) -> Option<u32> {
        if self.make.is_empty()
            || self.model.is_empty()
            || self.make != m.make
            || self.model != m.model
        {
            return None;
        }
        let mut score = 10;
        if let Some(s) = &self.serial {
            if !usable_serial(s) || s != &m.serial {
                return None;
            }
            score += 100;
        }
        if let Some(h) = &self.edid {
            if m.edid.as_ref() != Some(h) {
                return None;
            }
            score += 50;
        }
        Some(score)
    }
}
impl Monitor {
    pub fn physical_key(&self) -> String {
        let detail = if usable_serial(&self.serial) {
            format!("serial:{}", self.serial.trim())
        } else if let Some(h) = &self.edid {
            format!("edid:{h}")
        } else {
            "model-only".into()
        };
        // Length encoded JSON prevents delimiter collisions; connector names never enter the saved key.
        serde_json::to_string(&(&self.make, &self.model, detail)).unwrap()
    }
}
pub fn setup_key(monitors: &[Monitor]) -> Option<String> {
    let mut keys: Vec<_> = monitors.iter().map(Monitor::physical_key).collect();
    keys.sort();
    if keys.windows(2).any(|p| p[0] == p[1]) {
        return None;
    }
    Some(format!(
        "v1:{:x}",
        Sha256::digest(serde_json::to_vec(&keys).unwrap())
    ))
}
pub fn read_edid(connector: &str) -> Option<String> {
    let mut hashes = vec![];
    for entry in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name
            .strip_prefix("card")
            .and_then(|s| s.split_once('-'))
            .is_some_and(|(_, suffix)| suffix == connector)
        {
            if std::fs::read_to_string(entry.path().join("status"))
                .ok()
                .as_deref()
                != Some("connected\n")
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path().join("edid")) else {
                continue;
            };
            if bytes.len() >= 128
                && bytes[..8] == [0, 255, 255, 255, 255, 255, 255, 0]
                && bytes
                    .as_chunks::<128>()
                    .0
                    .iter()
                    .all(|b| b.iter().fold(0u8, |a, v| a.wrapping_add(*v)) == 0)
            {
                hashes.push(format!("{:x}", Sha256::digest(bytes)));
            }
        }
    }
    // Connector names may collide across GPUs. Refuse to attach an uncertain EDID.
    if hashes.len() == 1 {
        hashes.pop()
    } else {
        None
    }
}
