use crate::{
    audio::{self, Audio, Backend},
    command::Deadline,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const MAX_STATE: u64 = 65_536;

fn private_dir(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e).with_context(|| format!("Create {}", path.display())),
    }
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && m.uid() == unsafe { libc::getuid() },
        "Unsafe volume directory: {}",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn safe_file(file: &File) -> Result<()> {
    let m = file.metadata()?;
    ensure!(
        m.is_file() && m.uid() == unsafe { libc::getuid() } && m.nlink() == 1,
        "Unsafe volume state file"
    );
    Ok(())
}

pub struct Store {
    root: PathBuf,
    _lock: File,
}

impl Store {
    pub fn lock(deadline: &Deadline) -> Result<Self> {
        let base = match env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
            Some(path) => PathBuf::from(path),
            None => {
                let path = PathBuf::from(format!("/tmp/dictate-{}", unsafe { libc::getuid() }));
                private_dir(&path)?;
                path
            }
        };
        let root = base.join("dictation-volume-duck");
        private_dir(&root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(root.join("lock"))?;
        safe_file(&file)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let until = Instant::now() + Duration::from_secs(2);
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            ensure!(
                matches!(error.raw_os_error(), Some(libc::EWOULDBLOCK | libc::EINTR)),
                "Volume lock failed: {error}"
            );
            ensure!(Instant::now() < until, "Volume state is busy; try again");
            deadline.sleep(Duration::from_millis(10))?;
        }
        Ok(Self { root, _lock: file })
    }

    fn path(&self) -> PathBuf {
        self.root.join("state.env")
    }

    pub fn load(&self) -> Result<Option<State>> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.path())
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("Read saved volume"),
        };
        safe_file(&file)?;
        let mut text = String::new();
        file.take(MAX_STATE + 1).read_to_string(&mut text)?;
        ensure!(text.len() as u64 <= MAX_STATE, "Volume state is too large");
        State::parse(&text).map(Some)
    }

    pub fn save(&self, state: &State) -> Result<()> {
        let text = state.encode();
        ensure!(text.len() as u64 <= MAX_STATE, "Too many volume clients");
        // Never follow or replace a link at the destination.
        if let Ok(m) = fs::symlink_metadata(self.path()) {
            ensure!(
                m.is_file() && m.uid() == unsafe { libc::getuid() } && m.nlink() == 1,
                "Unsafe volume state file"
            );
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        file.write_all(text.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(self.path())?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        match fs::remove_file(self.path()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("Remove restored volume state"),
        }
    }
}

pub struct State {
    pub audio: Audio,
    pub clients: Vec<String>,
}

impl State {
    fn parse(text: &str) -> Result<Self> {
        let mut values = BTreeMap::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            // Decode both Bash printf %q and the native dictate app's quoted values.
            // No shell ever reads or executes the saved file.
            let words = shell_words::split(line).context("Invalid saved volume quoting")?;
            ensure!(words.len() == 1, "Invalid saved volume assignment");
            let (key, value) = words[0]
                .split_once('=')
                .context("Invalid saved volume assignment")?;
            ensure!(
                values.insert(key.to_owned(), value.to_owned()).is_none(),
                "Duplicate saved volume field"
            );
        }
        let get = |key| values.get(key).map(String::as_str).unwrap_or("");
        let muted = match get("ORIGINAL_MUTED") {
            "0" => false,
            "1" => true,
            _ => bail!("Invalid saved mute state"),
        };
        let clients = get("CLIENTS")
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        ensure!(
            clients
                .iter()
                .all(|c| sanitize_client(c) == *c && c.len() <= 256),
            "Invalid saved client"
        );
        Ok(Self {
            audio: Audio {
                backend: Backend::parse(get("ORIGINAL_BACKEND"))?,
                sink: get("ORIGINAL_SINK").to_owned(),
                identity: get("ORIGINAL_SINK_IDENTITY").to_owned(),
                volume: audio::volume(get("ORIGINAL_VOLUME"))?,
                muted,
            },
            clients,
        })
    }

    fn encode(&self) -> String {
        [
            ("ORIGINAL_BACKEND", self.audio.backend.name().to_owned()),
            ("ORIGINAL_SINK", self.audio.sink.clone()),
            ("ORIGINAL_SINK_IDENTITY", self.audio.identity.clone()),
            ("ORIGINAL_VOLUME", self.audio.volume.to_string()),
            (
                "ORIGINAL_MUTED",
                if self.audio.muted { "1" } else { "0" }.into(),
            ),
            ("CLIENTS", self.clients.join(" ")),
        ]
        .into_iter()
        .map(|(k, v)| format!("{k}={}\n", shell_words::quote(&v)))
        .collect()
    }

    pub fn status(&self) -> String {
        if self.clients.is_empty() {
            "idle\n".into()
        } else {
            format!(
                "ducked clients={} original_backend={} original_volume={} original_muted={}\n",
                self.clients.join(" "),
                self.audio.backend.name(),
                self.audio.volume,
                u8::from(self.audio.muted)
            )
        }
    }
}

pub fn sanitize_client(client: &str) -> String {
    let client = if client.is_empty() { "default" } else { client };
    client
        .bytes()
        .map(|c| {
            if c.is_ascii_alphanumeric() || b"_.:-".contains(&c) {
                c as char
            } else {
                '_'
            }
        })
        .collect()
}
