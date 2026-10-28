use crate::schema::{MAX_MESSAGE, Project, Snapshot, Status, StoredSnapshot, clean, invalid};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn cache_path() -> io::Result<PathBuf> {
    let root = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| invalid("HOME or XDG_CACHE_HOME is required"))?;
    Ok(root.join("project-picker/projects.json"))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Cache {
    version: u32,
    active: Option<String>,
    contexts: BTreeMap<String, StoredSnapshot>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            version: 1,
            active: None,
            contexts: BTreeMap::new(),
        }
    }
}

impl Cache {
    fn current(&self) -> Option<&StoredSnapshot> {
        self.active.as_ref().and_then(|key| self.contexts.get(key))
    }

    fn validate(&self, now: u64) -> io::Result<()> {
        if self.version != 1 {
            return Err(invalid("Unsupported project cache"));
        }
        for context in self.contexts.values() {
            context.snapshot.validate(now)?;
            if context.synced_at == 0
                || i64::try_from(context.synced_at)
                    .ok()
                    .and_then(DateTime::from_timestamp_millis)
                    .is_none()
            {
                return Err(invalid("Invalid sync time"));
            }
        }
        Ok(())
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid("Expected a regular cache file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_MESSAGE as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(invalid("Project cache exceeds the size limit"));
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid("Unsupported project cache"))
}

pub fn read_cache(path: &Path) -> io::Result<Cache> {
    let cache: Cache = match read_json(path) {
        Ok(cache) => cache,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Cache::default(),
        Err(error) => return Err(error),
    };
    cache.validate(now())?;
    Ok(cache)
}

fn lock(path: &Path) -> io::Result<File> {
    let parent = path.parent().ok_or_else(|| invalid("Invalid cache path"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(parent.join("sync.lock"))?;
    if !lock.metadata()?.is_file() {
        return Err(invalid("Expected a regular cache lock"));
    }
    lock.lock()?;
    Ok(lock)
}

fn refuse_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            Err(invalid("Refusing to replace a link or special file"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn atomic_write(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_MESSAGE {
        return Err(invalid("Project cache exceeds the size limit"));
    }
    refuse_link(path)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| invalid("Invalid cache path"))?;
    // create_new prevents a pre-existing name or symlink from being followed.
    let (temporary, mut file) = loop {
        let temporary = parent.join(format!(
            ".project-picker.{}.{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        refuse_link(path)?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub fn save_status(status: &Status, path: &Path) -> io::Result<()> {
    let _lock = lock(path)?;
    atomic_write(&path.with_file_name("status.json"), status)
}

/// Returns None for a stale/duplicate snapshot, or the updated project count.
pub fn save_snapshot(mut snapshot: Snapshot, path: &Path) -> io::Result<Option<usize>> {
    snapshot.validate(now())?;
    snapshot.sort();
    let _lock = lock(path)?;
    let mut cache = read_cache(path)?;
    let key = snapshot.context_key();
    if cache
        .contexts
        .get(&key)
        .is_some_and(|old| old.snapshot.started_at >= snapshot.started_at)
    {
        return Ok(None);
    }
    let count = snapshot.projects.len();
    if cache
        .current()
        .is_none_or(|active| active.snapshot.started_at <= snapshot.started_at)
    {
        cache.active = Some(key.clone());
    }
    let synced_at = now();
    cache.contexts.insert(
        key,
        StoredSnapshot {
            snapshot,
            synced_at,
        },
    );
    refuse_link(&path.with_file_name("status.json"))?;
    atomic_write(path, &cache)?;
    let status = Status::from_message(&serde_json::json!({"code": "ok"}), synced_at)?;
    atomic_write(&path.with_file_name("status.json"), &status)?;
    Ok(Some(count))
}

#[derive(Debug, PartialEq)]
pub struct View {
    pub projects: Vec<Project>,
    pub notice: Option<String>,
    pub last_synced: String,
}

pub fn view(cache: Option<&Cache>, status: Option<&Status>, now: u64) -> View {
    let cache = cache.filter(|cache| cache.validate(now).is_ok());
    let mut result = View {
        projects: vec![Project::home()],
        notice: None,
        last_synced: "Not synced yet".into(),
    };
    let context = cache.and_then(Cache::current);
    if let Some(context) = context {
        let mut snapshot = context.snapshot.clone();
        snapshot.sort();
        result
            .projects
            .extend(snapshot.projects.into_iter().map(|mut project| {
                project.name = clean(&project.name);
                project
            }));
        if let Some(synced) = i64::try_from(context.synced_at)
            .ok()
            .and_then(DateTime::from_timestamp_millis)
        {
            result.last_synced = format!(
                "{} · Last synced {}",
                clean(&context.snapshot.workspace_name),
                synced.with_timezone(&Local).format("%H:%M:%S")
            );
        }
    }
    result.notice =
        status.and_then(|status| status.notice(context.map_or(0, |c| c.synced_at), now));
    result
}

pub fn load(path: &Path) -> View {
    let cache = read_cache(path);
    let status = read_json(&path.with_file_name("status.json")).ok();
    let mut result = view(cache.as_ref().ok(), status.as_ref(), now());
    if cache.is_err() {
        result.notice = Some("Saved list unreadable · Check the picker cache".into());
    }
    result
}
