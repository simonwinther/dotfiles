use crate::Result;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub config_home: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        let home = PathBuf::from(env::var_os("HOME").ok_or("HOME is not set")?);
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let config = env::var_os("BACKGROUND_GRID_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| config_home.join("background-grid"));
        let cache = env::var_os("XDG_CACHE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
            .join("background-grid");
        Ok(Self {
            home,
            config,
            config_home,
            cache,
        })
    }

    pub fn current(&self) -> PathBuf {
        env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".local/state"))
            .join("desktop/background")
    }

    pub fn expand(&self, path: &Path) -> PathBuf {
        if path == Path::new("~") {
            self.home.clone()
        } else if let Ok(relative) = path.strip_prefix("~/") {
            self.home.join(relative)
        } else {
            path.to_path_buf()
        }
    }

    pub fn folders(&self) -> Result<Vec<PathBuf>> {
        let library = self.config.join("folders.json");
        if library.exists() {
            // Stow can link the individual files rather than the directory.
            // Resolve folders.json itself so relative library paths keep working.
            let source = fs::canonicalize(&library)?;
            let parent = source.parent().ok_or("Invalid folders.json path")?;
            let names: Vec<PathBuf> = serde_json::from_slice(&fs::read(&source)?)?;
            let configured: Vec<_> = names
                .into_iter()
                .map(|path| parent.join(self.expand(&path)))
                .collect();
            if configured.iter().any(|path| path.is_dir()) {
                return Ok(configured);
            }
        }
        let data = env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".local/share"));
        let mut folders = vec![data.join("wallpapers")];
        if let Ok(settings) = fs::read_to_string(self.config_home.join("waypaper/config.ini")) {
            folders.extend(
                waypaper_folders(&settings)
                    .iter()
                    .map(|path| self.expand(path)),
            );
        }
        Ok(folders)
    }
}

fn waypaper_folders(settings: &str) -> Vec<PathBuf> {
    let mut section = false;
    let mut reading = false;
    let mut result = Vec::new();
    for line in settings.lines() {
        let text = line.trim();
        if text.is_empty() || text.starts_with(['#', ';']) {
            continue;
        }
        if text.starts_with('[') {
            section = text == "[Settings]";
            reading = false;
        } else if section {
            if reading && line.starts_with(char::is_whitespace) {
                result.push(PathBuf::from(text));
            } else {
                reading = false;
                if let Some((key, value)) = text.split_once(['=', ':'])
                    && key.trim().eq_ignore_ascii_case("folder")
                {
                    reading = true;
                    if !value.trim().is_empty() {
                        result.push(PathBuf::from(value.trim()));
                    }
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn relative_library_uses_stow_source_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let home = temp.path().join("home");
        let source = temp.path().join("dotfiles/background-grid");
        let images = temp.path().join("dotfiles/pictures");
        let config = home.join(".config/background-grid");
        fs::create_dir_all(&source)?;
        fs::create_dir_all(&images)?;
        fs::create_dir_all(&config)?;
        fs::write(source.join("folders.json"), "[\"../pictures\"]")?;
        symlink(source.join("folders.json"), config.join("folders.json"))?;
        let paths = Paths {
            home: home.clone(),
            config,
            config_home: home.join(".config"),
            cache: home.join(".cache/background-grid"),
        };
        assert_eq!(fs::canonicalize(&paths.folders()?[0])?, images);
        Ok(())
    }

    #[test]
    fn waypaper_accepts_multiple_folders_and_ignores_other_settings() {
        let settings = "[Other]\nfolder=/wrong\n[Settings]\nfolder = ~/Pictures\n  /extra wallpapers\n# comment\nfill = fill\n";
        assert_eq!(
            waypaper_folders(settings),
            vec![
                PathBuf::from("~/Pictures"),
                PathBuf::from("/extra wallpapers")
            ]
        );
    }
}
