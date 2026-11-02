use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub max_items: usize,
    pub max_payload_mib: u64,
    pub repository_owner: String,
    pub terminal: Vec<String>,
    pub todo_categories: Vec<String>,
    pub websearch: Vec<SearchEngine>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchEngine {
    pub name: String,
    pub url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_items: 100,
            max_payload_mib: 64,
            repository_owner: String::new(),
            terminal: vec!["desktopctl".into(), "terminal".into()],
            todo_categories: vec![],
            websearch: vec![SearchEngine {
                name: "Google".into(),
                url: "https://www.google.com/search?q=%TERM%".into(),
            }],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub state: PathBuf,
    pub socket: PathBuf,
    pub compatibility_socket: PathBuf,
    pub home: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn environment() -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
        let xdg = |name, default: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(default))
        };
        let runtime = PathBuf::from(
            std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?,
        );
        Ok(Self {
            config: xdg("XDG_CONFIG_HOME", ".config"),
            state: xdg("XDG_STATE_HOME", ".local/state").join("desktop-search"),
            socket: runtime.join("desktop-search/search.sock"),
            compatibility_socket: runtime.join("elephant/elephant.sock"),
            data: xdg("XDG_DATA_HOME", ".local/share"),
            cache: xdg("XDG_CACHE_HOME", ".cache"),
            home,
        })
    }
}

pub fn private_directory(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    ensure!(
        !path.symlink_metadata()?.file_type().is_symlink(),
        "Private directory is a symbolic link"
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = std::env::var_os("DESKTOP_SEARCH_CONFIG_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.config.join("desktop-search/config.toml"));
        let config: Self = if file.exists() {
            ensure!(
                file.metadata()?.len() <= 1024 * 1024,
                "Configuration is too large"
            );
            toml::from_str(&std::fs::read_to_string(file)?)?
        } else {
            Self::default()
        };
        ensure!(
            (1..=100_000).contains(&config.max_items),
            "History limit must be 1–100000"
        );
        ensure!(
            (1..=256).contains(&config.max_payload_mib),
            "Payload limit must be 1–256 MiB"
        );
        ensure!(config.websearch.len() <= 100, "Too many search engines");
        ensure!(
            !config.terminal.is_empty()
                && config.terminal.len() <= 32
                && !config.terminal[0].is_empty(),
            "Terminal requires a command"
        );
        for engine in &config.websearch {
            ensure!(
                engine.url.starts_with("https://") || engine.url.starts_with("http://"),
                "Search engine requires an HTTP URL"
            );
            ensure!(
                engine.url.contains("%TERM%"),
                "Search URL must contain %TERM%"
            );
        }
        Ok(config)
    }
}
