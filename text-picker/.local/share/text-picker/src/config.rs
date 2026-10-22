use crate::Result;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum Mode {
    Snippets,
    Macros,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "snippets" => Ok(Self::Snippets),
            "macros" => Ok(Self::Macros),
            _ => Err("Choose snippets or macros".into()),
        }
    }

    pub fn filename(self) -> &'static str {
        match self {
            Self::Snippets => "snippets.json",
            Self::Macros => "macros.json",
        }
    }

    pub fn picker_class(self) -> &'static str {
        match self {
            Self::Snippets => "global.snip",
            Self::Macros => "macro.picker",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub label: String,
    pub text: String,
}

pub fn directory() -> Result<PathBuf> {
    let home = env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home).join(".config"))
        .join("text-picker"))
}

pub fn read(path: &Path) -> Result<Vec<Entry>> {
    let entries: Vec<Entry> = serde_json::from_slice(&fs::read(path)?)?;
    for (index, entry) in entries.iter().enumerate() {
        if entry.label.trim().is_empty() || entry.text.is_empty() {
            return Err(format!("Entry {} needs both a label and text", index + 1).into());
        }
        if entry.label.contains('\0')
            || entry.text.contains('\0')
            || entry.text.len() > MAX_TEXT_BYTES
        {
            return Err(
                format!("Entry {} contains unsupported or oversized text", index + 1).into(),
            );
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_preserves_unicode_newlines_and_shell_characters() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("snippets.json");
        let entries = vec![
            Entry {
                label: "Same label".into(),
                text: "æøå ' $HOME $(do-not-run)\nsecond\tline".into(),
            },
            Entry {
                label: "Same label".into(),
                text: "another value".into(),
            },
        ];
        fs::write(&file, serde_json::to_vec(&entries)?)?;
        assert_eq!(read(&file)?, entries);
        Ok(())
    }

    #[test]
    fn invalid_configuration_fails_before_opening_the_picker() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("snippets.json");
        fs::write(&file, r#"[{"label":"missing text"}]"#)?;
        assert!(read(&file).is_err());
        fs::write(&file, r#"[{"label":"empty","text":""}]"#)?;
        assert!(read(&file).is_err());
        Ok(())
    }
}
