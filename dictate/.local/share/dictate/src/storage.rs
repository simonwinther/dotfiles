use crate::config::{self, Config};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, RawFd},
        unix::{
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
            io::FromRawFd,
        },
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn generation() -> String {
    format!(
        "{:016x}{:08x}{:08x}",
        now_ms(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
static COUNTER: AtomicU64 = AtomicU64::new(1);
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let m = fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && m.uid() == unsafe { libc::getuid() },
        "Unsafe private directory: {}",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
fn safe_file(m: &fs::Metadata, path: &Path) -> Result<()> {
    ensure!(
        m.is_file() && m.uid() == unsafe { libc::getuid() } && m.nlink() == 1,
        "Unsafe file: {}",
        path.display()
    );
    Ok(())
}
pub fn read_file(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    safe_file(&f.metadata()?, path)?;
    Ok(f)
}
pub fn private_file(path: &Path, append: bool) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .append(append)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    safe_file(&f.metadata()?, path)?;
    f.set_permissions(fs::Permissions::from_mode(0o600))?;
    if !append {
        f.set_len(0)?;
    }
    Ok(f)
}
pub fn write_private(path: &Path, bytes: impl AsRef<[u8]>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Ok(m) = fs::symlink_metadata(path) {
        safe_file(&m, path)?;
    }
    let tmp = path.with_file_name(format!(
        ".{}-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        generation()
    ));
    let result = (|| -> Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        f.write_all(bytes.as_ref())?;
        f.sync_all()?;
        if let Ok(m) = fs::symlink_metadata(path) {
            safe_file(&m, path)?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    let _ = fs::remove_file(tmp);
    result
}
pub fn json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_private(path, serde_json::to_vec(value)?)
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let f = read_file(path).ok()?;
    if f.metadata().ok()?.len() > 8 * 1024 * 1024 {
        return None;
    }
    serde_json::from_reader(f.take(8 * 1024 * 1024 + 1)).ok()
}
pub fn log(c: &Config, line: &str) {
    if let Ok(mut f) = private_file(&c.path("LAST_LOG_FILE"), true) {
        let _ = writeln!(f, "{} {line}", now_ms());
    }
}
pub struct Lock(pub File);
impl Lock {
    pub fn acquire() -> Result<Self> {
        private_dir(&config::runtime_dir())?;
        // Never truncate an existing lock: all invocations lock the same inode.
        let f = private_file(&config::runtime_dir().join("session.lock"), true)?;
        ensure!(
            unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Dictate is already active"
        );
        Ok(Self(f))
    }
    /// The parent explicitly inherited this descriptor across exec.
    ///
    /// # Safety
    /// `fd` must be an open, exclusively owned descriptor for the locked session file.
    pub unsafe fn inherited(fd: RawFd) -> Self {
        Self(unsafe { File::from_raw_fd(fd) })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identity {
    pub pid: u32,
    pub start_ticks: String,
    pub boot_id: String,
}
impl Identity {
    pub fn get(pid: u32) -> Option<Self> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = stat
            .rsplit_once(") ")?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        if matches!(*rest.first()?, "Z" | "X") {
            return None;
        }
        Some(Self {
            pid,
            start_ticks: rest.get(19)?.to_string(),
            boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .ok()?
                .trim()
                .into(),
        })
    }
    pub fn alive(&self) -> bool {
        Self::get(self.pid)
            .is_some_and(|x| x.start_ticks == self.start_ticks && x.boot_id == self.boot_id)
    }
    pub fn signal(&self, sig: i32) {
        if self.alive() {
            unsafe {
                libc::kill(self.pid as i32, sig);
            }
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub owner: Identity,
    pub recorder: Option<Identity>,
    pub mode: String,
    pub phase: String,
    pub start_ms: u64,
    pub audio_file: PathBuf,
    pub settings: BTreeMap<String, String>,
    pub recording_ms: u64,
    #[serde(default)]
    pub transcript: Option<String>,
    #[serde(default)]
    pub retain_source: bool,
}
impl Session {
    pub fn path() -> PathBuf {
        config::runtime_dir().join("native-session.json")
    }
    pub fn save(&self) -> Result<()> {
        json(&Self::path(), self)
    }
    pub fn load() -> Option<Self> {
        read_json(&Self::path())
    }
    pub fn active() -> Option<Self> {
        Self::load().filter(|s| s.owner.alive())
    }
}
pub fn legacy_active() -> Option<(String, Identity)> {
    for (file, mode) in [("live-session.json", "live"), ("session.json", "batch")] {
        let Some(v) = read_json::<serde_json::Value>(&config::runtime_dir().join(file)) else {
            continue;
        };
        let Some(owner) = v
            .get("owner")
            .and_then(|x| serde_json::from_value::<Identity>(x.clone()).ok())
        else {
            continue;
        };
        if owner.alive() {
            return Some((mode.into(), owner));
        }
    }
    None
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Recovery {
    pub settings: BTreeMap<String, String>,
    pub recording_ms: u64,
    pub transcript: Option<String>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub saved_ms: u64,
    #[serde(skip)]
    pub generation: String,
    #[serde(skip)]
    pub audio_file: PathBuf,
}
fn recovery_dir() -> PathBuf {
    config::state_dir().join("recovery")
}
fn valid_generation(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|c| c.is_ascii_hexdigit())
}
impl Recovery {
    pub fn load() -> Option<Self> {
        let pointer = read_json::<serde_json::Value>(&recovery_dir().join("latest.json"))?;
        let id = pointer.get("generation")?.as_str()?;
        if !valid_generation(id) {
            return None;
        }
        let mut r: Self = read_json(&recovery_dir().join(id).join("metadata.json"))?;
        r.audio_file = recovery_dir().join(id).join("recording.wav");
        read_file(&r.audio_file).ok()?;
        r.generation = id.into();
        Some(r)
    }
    pub fn save(
        source: &Path,
        settings: BTreeMap<String, String>,
        recording_ms: u64,
        transcript: Option<String>,
        reason: &str,
        pcm_rate: Option<u32>,
    ) -> Result<Self> {
        let previous = Self::load();
        let id = generation();
        let root = recovery_dir();
        private_dir(&root)?;
        let dir = root.join(&id);
        private_dir(&dir)?;
        let mut result = Self {
            settings,
            recording_ms,
            transcript,
            reason: reason.into(),
            saved_ms: now_ms(),
            generation: id.clone(),
            audio_file: dir.join("recording.wav"),
        };
        let publish = (|| -> Result<()> {
            if let Some(rate) = pcm_rate {
                crate::audio::wrap_pcm(source, &result.audio_file, rate, 1, "s16")?;
            } else {
                let mut src = read_file(source)?;
                let mut dst = private_file(&result.audio_file, false)?;
                std::io::copy(&mut src, &mut dst)?;
                dst.sync_all()?;
            }
            json(&dir.join("metadata.json"), &result)?;
            File::open(&dir)?.sync_all()?;
            json(
                &root.join("latest.json"),
                &serde_json::json!({"generation":id}),
            )?;
            File::open(&root)?.sync_all()?;
            Ok(())
        })();
        if let Err(e) = publish {
            let published = read_json::<serde_json::Value>(&root.join("latest.json"))
                .is_some_and(|v| v["generation"] == id);
            if !published {
                let _ = fs::remove_dir_all(&dir);
            }
            return Err(e).context("Could not archive recording; original audio retained");
        }
        if let Some(old) = previous {
            let _ = fs::remove_dir_all(root.join(old.generation));
        }
        result.saved_ms = now_ms();
        Ok(result)
    }
    pub fn discard(id: Option<&str>) -> Result<bool> {
        let Some(current) = Self::load() else {
            return Ok(false);
        };
        if id.is_some_and(|id| id != current.generation) {
            return Ok(false);
        }
        fs::remove_file(recovery_dir().join("latest.json"))?;
        fs::remove_dir_all(recovery_dir().join(current.generation))?;
        Ok(true)
    }
}
pub fn recover_stale(c: &Config) -> Result<bool> {
    if let Some(stale) = Session::load().filter(|s| !s.owner.alive()) {
        if let Some(recorder) = &stale.recorder {
            recorder.signal(libc::SIGKILL);
        }
        if stale.audio_file.is_file()
            && fs::metadata(&stale.audio_file)?.len() > if stale.mode == "live" { 0 } else { 44 }
        {
            let pcm = stale.audio_file.extension().is_some_and(|s| s == "pcm");
            let duration = if pcm {
                fs::metadata(&stale.audio_file)?.len() / 2 * 1000 / 24000
            } else {
                // A killed worker cannot finalize its growing WAV header.
                let rate = stale
                    .settings
                    .get("RECORD_RATE")
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(16000);
                let channels = stale
                    .settings
                    .get("RECORD_CHANNELS")
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(1);
                let format = stale
                    .settings
                    .get("RECORD_FORMAT")
                    .map(String::as_str)
                    .unwrap_or("s16");
                let bytes = fs::metadata(&stale.audio_file)?.len().saturating_sub(44);
                use std::io::{Seek, SeekFrom};
                let mut file = OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&stale.audio_file)?;
                safe_file(&file.metadata()?, &stale.audio_file)?;
                file.seek(SeekFrom::Start(0))?;
                file.write_all(&crate::audio::wav_header(bytes, rate, channels, format)?)?;
                file.sync_all()?;
                bytes * 1000
                    / (rate as u64 * channels as u64 * crate::audio::sample_bytes(format) as u64)
            };
            Recovery::save(
                &stale.audio_file,
                stale.settings,
                duration,
                stale.transcript,
                "Interrupted Rust session",
                pcm.then_some(24000),
            )?;
            log(c, "event=stale_session_recovered");
        }
        fs::remove_file(Session::path())?;
        return Ok(true);
    }
    Ok(false)
}
