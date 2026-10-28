use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Local,
    Ssh(String),
}
impl Target {
    pub fn label(&self) -> &str {
        match self {
            Self::Local => "local",
            Self::Ssh(host) => host,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub hosts: Vec<String>,
}
pub fn validate(host: &str) -> Result<()> {
    crate::remote::validate_host(host)?;
    if host.is_empty()
        || host.len() > 255
        || host.starts_with('-')
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.@:-[]%".contains(c))
    {
        bail!("Use an SSH alias or user@host, without spaces or SSH options");
    }
    Ok(())
}
pub fn config_path() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .context("HOME or XDG_CONFIG_HOME must be set")?;
    Ok(root.join("cluster/hosts.json"))
}
impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            let legacy = path
                .parent()
                .and_then(Path::parent)
                .context("Invalid host config path")?
                .join("mytop/clusters.json");
            if legacy.is_file() {
                let config = Self::load_from(&legacy)?;
                config.save_to(&path)?;
                return Ok(config);
            }
        }
        Self::load_from(&path)
    }
    fn load_from(path: &Path) -> Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("Read {}", path.display())),
        };
        let config: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("Invalid host list in {}", path.display()))?;
        for (i, host) in config.hosts.iter().enumerate() {
            validate(host)?;
            if config.hosts[..i].contains(host) {
                bail!("Duplicate saved host: {host}");
            }
        }
        Ok(config)
    }
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path()?)
    }
    fn save_to(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("Host list has no parent directory")?;
        if !parent.exists() {
            fs::create_dir_all(parent)?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let temporary = parent.join(format!(".clusters-{}-{stamp}", std::process::id()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            serde_json::to_writer_pretty(&mut file, self)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result.with_context(|| format!("Save {}", path.display()))
    }
    pub fn add(&mut self, host: &str) -> Result<()> {
        validate(host)?;
        if self.hosts.iter().any(|h| h == host) {
            bail!("Host already saved: {host}");
        }
        self.hosts.push(host.into());
        Ok(())
    }
    pub fn remove(&mut self, host: &str) -> Result<()> {
        let i = self
            .hosts
            .iter()
            .position(|h| h == host)
            .context("Host is not saved")?;
        self.hosts.remove(i);
        Ok(())
    }
}

#[derive(Clone, Debug, clap::Subcommand)]
pub enum HostCommand {
    /// Show saved SSH aliases
    List,
    /// Save an SSH alias without connecting
    Add { host: String },
    /// Remove a saved alias without changing the server
    Remove { host: String },
    /// Install the cluster CLI and mytop compatibility command on a server
    Install { host: String },
}
pub fn manage(action: HostCommand) -> Result<()> {
    let mut config = Config::load()?;
    match action {
        HostCommand::List => {
            for host in config.hosts {
                println!("{host}");
            }
        }
        HostCommand::Add { host } => {
            config.add(&host)?;
            config.save()?;
            println!("Added {host}");
        }
        HostCommand::Remove { host } => {
            config.remove(&host)?;
            config.save()?;
            println!("Removed {host}");
        }
        HostCommand::Install { host } => {
            validate(&host)?;
            let result = crate::remote::remote(&host, "install-cli", serde_json::json!({}))?;
            println!(
                "Installed cluster and mytop on {host}: {}",
                result["path"].as_str().unwrap_or("")
            );
            if !config.hosts.contains(&host) {
                config.add(&host)?;
                config.save()?;
            }
        }
    }
    Ok(())
}
pub fn local_available() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            fs::metadata(dir.join("scontrol"))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

#[derive(Default)]
pub struct Picker {
    pub open: bool,
    pub selected: usize,
    pub input: Option<String>,
    pub error: Option<String>,
    pub hosts: Config,
    pub local: bool,
}
impl Picker {
    pub fn choices(&self) -> Vec<Target> {
        let mut targets = Vec::new();
        if self.local {
            targets.push(Target::Local);
        }
        targets.extend(self.hosts.hosts.iter().cloned().map(Target::Ssh));
        targets
    }
    pub fn reload(&mut self) {
        match Config::load() {
            Ok(config) => {
                self.hosts = config;
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        self.selected = self.selected.min(self.choices().len().saturating_sub(1));
    }
    pub fn add(&mut self, host: &str) -> Result<()> {
        let mut config = Config::load()?;
        config.add(host)?;
        config.save()?;
        self.hosts = config;
        self.selected = self.choices().len().saturating_sub(1);
        Ok(())
    }
    pub fn remove(&mut self) -> Result<()> {
        let Some(Target::Ssh(host)) = self.choices().get(self.selected).cloned() else {
            bail!("The local Slurm connection cannot be removed");
        };
        let mut config = Config::load()?;
        config.remove(&host)?;
        config.save()?;
        self.hosts = config;
        self.selected = self.selected.min(self.choices().len().saturating_sub(1));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_aliases_and_rejects_options_and_shell_input() {
        for host in ["hendrix", "mostafa", "user@host.example", "user@[::1]"] {
            assert!(validate(host).is_ok());
        }
        for host in [
            "",
            "-oProxyCommand=bad",
            "a b",
            "host\nother",
            "$(bad)",
            "a;bad",
        ] {
            assert!(validate(host).is_err());
        }
    }
    #[test]
    fn adding_removing_and_picker_order() {
        let mut p = Picker {
            local: true,
            ..Default::default()
        };
        p.hosts.add("hendrix").unwrap();
        p.hosts.add("mostafa").unwrap();
        assert!(p.hosts.add("hendrix").is_err());
        assert_eq!(
            p.choices(),
            vec![
                Target::Local,
                Target::Ssh("hendrix".into()),
                Target::Ssh("mostafa".into())
            ]
        );
        p.hosts.remove("hendrix").unwrap();
        assert_eq!(p.hosts.hosts, ["mostafa"]);
        assert!(p.hosts.remove("missing").is_err());
    }
    #[test]
    fn host_list_round_trip_permissions_and_invalid_input() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("mytop-hosts-{}-{stamp}", std::process::id()));
        let path = root.join("mytop/clusters.json");
        assert!(Config::load_from(&path).unwrap().hosts.is_empty());
        let c = Config {
            hosts: vec!["hendrix".into(), "mostafa".into()],
        };
        c.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path).unwrap().hosts, c.hosts);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&path, r#"{"hosts":["a","a"]}"#).unwrap();
        assert!(Config::load_from(&path).is_err());
        fs::write(&path, "broken json").unwrap();
        assert!(Config::load_from(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
