use crate::{core::config::base, monitors::Position};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub layouts: BTreeMap<String, Position>,
}
impl Preferences {
    pub fn path() -> PathBuf {
        base("XDG_STATE_HOME", ".local/state").join("desktop-ring/preferences.json")
    }
    pub fn load() -> anyhow::Result<Self> {
        match std::fs::read(Self::path()) {
            Ok(data) => {
                let p: Self = serde_json::from_slice(&data)?;
                anyhow::ensure!(
                    p.version <= 1,
                    "preferences were written by a newer version"
                );
                Ok(p)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let temp = dir.join(format!(".preferences-{}.tmp", std::process::id()));
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        f.write_all(&serde_json::to_vec_pretty(self)?)?;
        f.sync_all()?;
        std::fs::rename(temp, &path)?;
        std::fs::File::open(dir)?.sync_all()?;
        Ok(())
    }
}
