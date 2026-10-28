//! A disposable SQLite cache of file metadata. A scope is replaced only after
//! its filesystem scan succeeds; readers always see a complete old or new set.
use crate::{
    Options, Result, SortOrder, absolute, detached, executable_path, home, scan_args, scan_files,
    sorted_paths,
};
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, TransactionBehavior, config::DbConfig, params,
};
mod events;
use events::{Refresh, Watch};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const VERSION: i64 = 1;
const POLICY: i64 = 1;
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct InvalidCache(String);

impl std::fmt::Display for InvalidCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for InvalidCache {}

fn recoverable(error: &(dyn std::error::Error + 'static)) -> bool {
    error.is::<InvalidCache>()
        || error
            .downcast_ref::<rusqlite::Error>()
            .is_some_and(|error| {
                matches!(
                    error.sqlite_error_code(),
                    Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
                )
            })
}

// Only a SQL error in our fixed schema statements indicates a bad layout.
// Busy, read-only, full-disk and IO errors must leave the cache in place.
fn schema_error(error: rusqlite::Error) -> Box<dyn std::error::Error> {
    if error
        .sqlite_error()
        .is_some_and(|error| error.extended_code == rusqlite::ffi::SQLITE_ERROR)
    {
        Box::new(InvalidCache(format!("Invalid file index schema: {error}")))
    } else {
        Box::new(error)
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

fn cache_lock(file: &File, operation: libc::c_int) -> io::Result<()> {
    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if error.kind() != io::ErrorKind::WouldBlock {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "File index is in use; cannot acquire cache recovery lock",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn quarantine(path: &Path) -> Result<PathBuf> {
    // Keep journals with their database, and move the database last so a new
    // database can never pick up an old journal after an interrupted recovery.
    let mut files = Vec::new();
    for suffix in ["-wal", "-shm", "-journal", ""] {
        let source = with_suffix(path, suffix);
        match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.is_file() => files.push(source),
            Err(error) if !suffix.is_empty() && error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => {
                return Err(
                    format!("Index cache is not a regular file: {}", source.display()).into(),
                );
            }
        }
    }
    let mut attempt = 0;
    let directory = loop {
        let directory = with_suffix(
            path,
            &format!(
                ".quarantine-{}-{}-{attempt}",
                timestamp(),
                std::process::id()
            ),
        );
        match fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => break directory,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(error) => return Err(error.into()),
        }
    };
    let mut moved = Vec::new();
    for source in files {
        let target = directory.join(source.file_name().unwrap());
        if let Err(error) = fs::rename(&source, &target) {
            // A failed quarantine must not make a replacement database. Undo
            // partial moves when possible; otherwise retain them for inspection.
            for (source, target) in moved.iter().rev() {
                fs::rename(target, source).map_err(|restore| {
                    format!("Could not quarantine file index ({error}) or restore it ({restore}); preserved at {}", directory.display())
                })?;
            }
            fs::remove_dir(&directory)?;
            return Err(error.into());
        }
        moved.push((source, target));
    }
    Ok(directory)
}

fn database_path() -> Result<PathBuf> {
    absolute(
        &env::var_os("FILE_PICKER_INDEX")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home().join(".cache"))
                    .join("file-picker/index.sqlite3")
            }),
    )
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

struct Scope {
    id: i64,
}

#[derive(Clone, Copy)]
struct State {
    revision: i64,
    updated: Option<i64>,
}

struct Index {
    connection: Connection,
    path: PathBuf,
    // Fields drop in declaration order: close SQLite before releasing this.
    _cache_lock: File,
}

impl Index {
    fn open(path: &Path) -> Result<Self> {
        let parent = path.parent().ok_or("Index needs a parent directory")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        // Serialize opening/recovery separately from the shared lifetime lock.
        // A recovered connection must not block other waiting openers.
        let gate = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(with_suffix(path, ".open.lock"))?;
        cache_lock(&gate, libc::LOCK_EX)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(with_suffix(path, ".lock"))?;
        cache_lock(&lock, libc::LOCK_SH)?;
        let connection = match Self::connect(path) {
            Ok(connection) => connection,
            Err(error) if recoverable(error.as_ref()) => {
                // Never rename a database that another picker is still using.
                // Release the shared lock before competing for recovery.
                cache_lock(&lock, libc::LOCK_UN)?;
                cache_lock(&lock, libc::LOCK_EX)?;
                // Another opener may already have recovered it while we waited.
                let connection = match Self::connect(path) {
                    Ok(connection) => connection,
                    Err(error) if recoverable(error.as_ref()) => {
                        let directory = quarantine(path)?;
                        eprintln!(
                            "file-picker: Rebuilding file index ({error}); quarantined at {}",
                            directory.display()
                        );
                        Self::connect(path)?
                    }
                    Err(error) => return Err(error),
                };
                cache_lock(&lock, libc::LOCK_SH)?;
                connection
            }
            Err(error) => return Err(error),
        };
        Ok(Self {
            connection,
            path: path.to_owned(),
            _cache_lock: lock,
        })
    }

    fn connect(path: &Path) -> Result<Connection> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        // Reject truncated/non-database files before SQLite can initialize or
        // overwrite their shared-memory sidecar. An empty new file is valid;
        // every nonempty SQLite database needs its complete 100-byte header.
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Index cache is not a regular file",
            )
            .into());
        }
        if metadata.len() != 0 && metadata.len() < 100 {
            return Err(InvalidCache("Index file is shorter than a SQLite header".into()).into());
        }
        let mut connection = Connection::open(path)?;
        // A failed probe must leave any uncheckpointed WAL with the old cache
        // rather than merging/removing it when this connection closes.
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
        connection.busy_timeout(LOCK_TIMEOUT)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != 0 && version != VERSION {
            return Err(InvalidCache(format!("Unsupported file index version: {version}")).into());
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "temp_store", "MEMORY")?;
        if version == 0 {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction
                .execute_batch(
                    "
                CREATE TABLE IF NOT EXISTS scopes (
                    id INTEGER PRIMARY KEY,
                    root BLOB NOT NULL,
                    target BLOB NOT NULL,
                    all_files INTEGER NOT NULL,
                    pdf INTEGER NOT NULL,
                    policy INTEGER NOT NULL,
                    revision INTEGER NOT NULL DEFAULT 0,
                    updated INTEGER,
                    UNIQUE(root, target, all_files, pdf, policy)
                );
                CREATE TABLE IF NOT EXISTS files (
                    scope INTEGER NOT NULL REFERENCES scopes(id) ON DELETE CASCADE,
                    path BLOB NOT NULL,
                    changed_sec INTEGER NOT NULL,
                    changed_nsec INTEGER NOT NULL,
                    PRIMARY KEY(scope, path)
                ) WITHOUT ROWID;
                CREATE INDEX IF NOT EXISTS files_recent
                    ON files(scope, changed_sec DESC, changed_nsec DESC, path);
            ",
                )
                .map_err(schema_error)?;
            Self::validate_schema(&transaction)?;
            transaction.pragma_update(None, "user_version", VERSION)?;
            transaction.commit()?;
        } else {
            Self::validate_schema(&connection)?;
        }
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
        Ok(connection)
    }

    fn validate_schema(connection: &Connection) -> Result<()> {
        // Check the layout without walking every cached row on a warm open.
        connection
            .prepare(
                "SELECT id,root,target,all_files,pdf,policy,revision,updated FROM scopes LIMIT 0",
            )
            .map_err(schema_error)?;
        connection
            .prepare("SELECT scope,path,changed_sec,changed_nsec FROM files LIMIT 0")
            .map_err(schema_error)?;
        Ok(())
    }

    fn scope(&self, options: &Options) -> Result<Scope> {
        let root = options.root.as_os_str().as_bytes();
        // A retargeted directory symlink must not reuse its former target's list.
        let target = options.root.canonicalize()?;
        let values = params![
            root,
            target.as_os_str().as_bytes(),
            options.all,
            options.pdf,
            POLICY
        ];
        // The common read path takes no write lock.
        if let Some(id) = self.connection.query_row(
            "SELECT id FROM scopes WHERE root=?1 AND target=?2 AND all_files=?3 AND pdf=?4 AND policy=?5",
            values, |row| row.get(0),
        ).optional()? { return Ok(Scope { id }); }
        self.connection.execute(
            "INSERT INTO scopes(root,target,all_files,pdf,policy) VALUES(?1,?2,?3,?4,?5) ON CONFLICT DO NOTHING",
            values,
        )?;
        let id = self.connection.query_row(
            "SELECT id FROM scopes WHERE root=?1 AND target=?2 AND all_files=?3 AND pdf=?4 AND policy=?5",
            values, |row| row.get(0),
        )?;
        Ok(Scope { id })
    }

    fn state(&self, scope: &Scope) -> Result<State> {
        Ok(self.connection.query_row(
            "SELECT revision, updated FROM scopes WHERE id=?1",
            [scope.id],
            |row| {
                Ok(State {
                    revision: row.get(0)?,
                    updated: row.get(1)?,
                })
            },
        )?)
    }

    fn committed_state(&self, scope: &Scope) -> Result<Option<State>> {
        // A WAL write notification can precede SQLite publishing its commit.
        // Briefly reserving the writer lock fences the read after that commit.
        // Never wait here: keep the old snapshot usable while a writer works.
        self.connection.busy_timeout(Duration::ZERO)?;
        let result = (|| {
            let transaction = match rusqlite::Transaction::new_unchecked(
                &self.connection,
                TransactionBehavior::Immediate,
            ) {
                Ok(transaction) => transaction,
                Err(error)
                    if matches!(
                        error.sqlite_error_code(),
                        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            };
            let state = self.state(scope)?;
            transaction.rollback()?;
            Ok(Some(state))
        })();
        self.connection.busy_timeout(LOCK_TIMEOUT)?;
        result
    }

    fn lock(&self, scope: &Scope, wait: bool) -> Result<Option<File>> {
        let mut name = self.path.as_os_str().to_owned();
        name.push(format!(".scope-{}.lock", scope.id));
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(PathBuf::from(name))?;
        let flags = libc::LOCK_EX | if wait { 0 } else { libc::LOCK_NB };
        if unsafe { libc::flock(file.as_raw_fd(), flags) } != 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            return Err(error.into());
        }
        Ok(Some(file))
    }

    fn query(
        &self,
        scope: &Scope,
        options: &Options,
        mut visit: impl FnMut(Vec<u8>) -> Result<()>,
    ) -> Result<()> {
        let order = match options.sort {
            SortOrder::Name | SortOrder::None => "path",
            SortOrder::Recent => "changed_sec DESC, changed_nsec DESC, path",
        };
        let sql = if options.recent {
            format!(
                "SELECT path FROM (SELECT path,changed_sec,changed_nsec FROM files WHERE scope=?1
                ORDER BY changed_sec DESC,changed_nsec DESC,path LIMIT 30000) ORDER BY {order}"
            )
        } else {
            format!("SELECT path FROM files WHERE scope=?1 ORDER BY {order}")
        };
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query([scope.id])?;
        while let Some(row) = rows.next()? {
            visit(row.get(0)?)?;
        }
        Ok(())
    }

    fn own_file(&self, path: &Path) -> bool {
        path.parent() == self.path.parent()
            && path.file_name().is_some_and(|name| {
                let name = name.as_bytes();
                let database = self.path.file_name().unwrap().as_bytes();
                name == database
                    || name.strip_prefix(database).is_some_and(|suffix| {
                        matches!(
                            suffix,
                            b"-wal" | b"-shm" | b"-journal" | b".lock" | b".open.lock"
                        ) || suffix.starts_with(b".scope-")
                            || suffix.starts_with(b".quarantine-")
                    })
            })
            || path.parent().is_some_and(|parent| {
                parent.parent() == self.path.parent()
                    && parent.file_name().is_some_and(|name| {
                        name.as_bytes()
                            .strip_prefix(self.path.file_name().unwrap().as_bytes())
                            .is_some_and(|suffix| suffix.starts_with(b".quarantine-"))
                    })
            })
    }

    fn refresh(
        &mut self,
        scope: &Scope,
        options: &Options,
        wait: bool,
    ) -> Result<Option<serde_json::Value>> {
        let Some(_lock) = self.lock(scope, wait)? else {
            return Ok(None);
        };
        let started = Instant::now();
        // Perform IO before starting a write transaction. A failed scan cannot
        // prune an existing cache, and other scopes can update concurrently.
        let mut files = scan_files(options)?;
        files.retain(|(_, path)| !self.own_file(path));
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            "CREATE TEMP TABLE fresh (
            path BLOB PRIMARY KEY, changed_sec INTEGER NOT NULL, changed_nsec INTEGER NOT NULL
        ) WITHOUT ROWID;",
        )?;
        {
            let mut insert = transaction.prepare("INSERT INTO fresh VALUES(?1,?2,?3)")?;
            for (changed, path) in &files {
                insert.execute(params![
                    path.as_os_str().as_bytes(),
                    changed.div_euclid(1_000_000_000) as i64,
                    changed.rem_euclid(1_000_000_000) as i64
                ])?;
            }
        }
        let added: i64 = transaction.query_row(
            "SELECT count(*) FROM fresh n WHERE NOT EXISTS(SELECT 1 FROM files o WHERE o.scope=?1 AND o.path=n.path)",
            [scope.id], |row| row.get(0),
        )?;
        let updated: i64 = transaction.query_row(
            "SELECT count(*) FROM fresh n JOIN files o ON o.path=n.path AND o.scope=?1
                WHERE o.changed_sec!=n.changed_sec OR o.changed_nsec!=n.changed_nsec",
            [scope.id],
            |row| row.get(0),
        )?;
        let removed = transaction.execute(
            "DELETE FROM files WHERE scope=?1 AND NOT EXISTS(SELECT 1 FROM fresh n WHERE n.path=files.path)", [scope.id],
        )?;
        transaction.execute("INSERT INTO files(scope,path,changed_sec,changed_nsec)
            SELECT ?1,path,changed_sec,changed_nsec FROM fresh WHERE true
            ON CONFLICT(scope,path) DO UPDATE SET changed_sec=excluded.changed_sec, changed_nsec=excluded.changed_nsec
            WHERE files.changed_sec!=excluded.changed_sec OR files.changed_nsec!=excluded.changed_nsec", [scope.id])?;
        transaction.execute(
            "UPDATE scopes SET updated=?2,
            revision=revision + CASE WHEN updated IS NULL OR ?3 THEN 1 ELSE 0 END WHERE id=?1",
            params![scope.id, timestamp(), added + updated + removed as i64 != 0],
        )?;
        transaction.execute_batch("DROP TABLE fresh")?;
        transaction.commit()?;
        Ok(Some(
            serde_json::json!({"files": files.len(), "added": added, "updated": updated,
            "removed": removed, "elapsed_ms": started.elapsed().as_millis()}),
        ))
    }

    fn forget(&mut self, paths: &[PathBuf]) -> Result<usize> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut removed = 0;
        for path in paths {
            let path = absolute(path)?;
            let bytes = path.as_os_str().as_bytes();
            transaction.execute(
                "UPDATE scopes SET revision=revision+1
                WHERE id IN (SELECT scope FROM files WHERE path=?1)",
                [bytes],
            )?;
            removed += transaction.execute("DELETE FROM files WHERE path=?1", [bytes])?;
        }
        transaction.commit()?;
        Ok(removed)
    }

    fn clear_scope(&mut self, options: &Options) -> Result<usize> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let values = params![
            options.root.as_os_str().as_bytes(),
            options.all,
            options.pdf
        ];
        // Also clear old targets of a moved directory/symlink, even if the root
        // no longer exists. This operation never touches filesystem files.
        let removed = transaction.execute(
            "DELETE FROM files WHERE scope IN
            (SELECT id FROM scopes WHERE root=?1 AND all_files=?2 AND pdf=?3)",
            values,
        )?;
        transaction.execute(
            "UPDATE scopes SET updated=NULL,revision=revision+1
            WHERE root=?1 AND all_files=?2 AND pdf=?3",
            values,
        )?;
        transaction.commit()?;
        Ok(removed)
    }
}

pub fn list(options: &Options) -> Result<()> {
    let index = Index::open(&database_path()?)?;
    let scope = index.scope(options)?;
    let mut writer = io::BufWriter::new(io::stdout().lock());
    index.query(&scope, options, |path| {
        writer.write_all(&path)?;
        writer.write_all(b"\0")?;
        Ok(())
    })?;
    writer.flush()?;
    Ok(())
}

pub fn update(options: &Options, wait: bool) -> Result<()> {
    let mut index = Index::open(&database_path()?)?;
    let scope = index.scope(options)?;
    let report = index
        .refresh(&scope, options, wait)?
        .unwrap_or_else(|| serde_json::json!({"busy": true}));
    println!("{report}");
    Ok(())
}

pub fn remove(paths: &[PathBuf]) -> Result<()> {
    let removed = Index::open(&database_path()?)?.forget(paths)?;
    println!("{}", serde_json::json!({"removed": removed}));
    Ok(())
}

pub fn clear(options: &Options) -> Result<()> {
    let removed = Index::open(&database_path()?)?.clear_scope(options)?;
    println!("{}", serde_json::json!({"removed": removed}));
    Ok(())
}

pub fn status() -> Result<()> {
    let index = Index::open(&database_path()?)?;
    let mut statement = index.connection.prepare("SELECT s.root,s.all_files,s.pdf,s.updated,count(f.path)
        FROM scopes s LEFT JOIN files f ON f.scope=s.id GROUP BY s.id ORDER BY s.root,s.all_files,s.pdf")?;
    let mut rows = statement.query([])?;
    let mut scopes = Vec::new();
    while let Some(row) = rows.next()? {
        let path: Vec<u8> = row.get(0)?;
        scopes.push(serde_json::json!({"root": String::from_utf8_lossy(&path), "all": row.get::<_,bool>(1)?,
            "pdf": row.get::<_,bool>(2)?, "updated_unix_ms": row.get::<_,Option<i64>>(3)?, "files": row.get::<_,i64>(4)?}));
    }
    println!(
        "{}",
        serde_json::json!({"database": index.path.to_string_lossy(), "scopes": scopes})
    );
    Ok(())
}

// NUL-framed protocol: a header followed by zero or more absolute paths and an
// empty record. Unix paths cannot contain NUL, including non-UTF-8 filenames.
fn snapshot(
    index: &Index,
    scope: &Scope,
    options: &Options,
    writer: &mut impl Write,
    header: &[u8],
) -> Result<()> {
    writer.write_all(header)?;
    writer.write_all(b"\0")?;
    index.query(scope, options, |path| {
        writer.write_all(&path)?;
        writer.write_all(b"\0")?;
        Ok(())
    })?;
    writer.write_all(b"\0")?;
    writer.flush()?;
    Ok(())
}

fn message(writer: &mut impl Write, header: &[u8]) -> Result<()> {
    writer.write_all(header)?;
    writer.write_all(b"\0")?;
    writer.flush()?;
    Ok(())
}

fn warning(writer: &mut impl Write, error: &str) -> Result<()> {
    writer.write_all(b"warning\0")?;
    writer.write_all(error.replace('\0', "�").as_bytes())?;
    writer.write_all(b"\0")?;
    writer.flush()?;
    Ok(())
}

fn start_update(options: &Options) -> Result<Refresh> {
    let mut command = Command::new(executable_path()?);
    scan_args(&mut command, options, "--index-update-if-idle");
    // A brief index refresh survives a fast selection/Escape. It has no window
    // and the scope lock coalesces overlapping refreshes from both pickers.
    Ok(Refresh::new(
        detached(&mut command).stderr(Stdio::piped()).spawn()?,
    )?)
}

fn stream_index(
    options: &Options,
    writer: &mut impl Write,
    output: std::os::fd::RawFd,
) -> Result<()> {
    let index = Index::open(&database_path()?)?;
    let mut watch = Watch::new(&index.path);
    if index.connection.is_readonly("main")? {
        watch.disable();
    }
    let scope = index.scope(options)?;
    let mut state = index.state(&scope)?;
    // Start before streaming the cache: a fast first-page selection can close
    // the reader while the remainder of the cached snapshot is still in flight.
    let mut update = Some(start_update(options)?);
    if state.updated.is_some() {
        snapshot(&index, &scope, options, writer, b"cached")?;
    }
    message(writer, b"updating")?;
    let mut next = Instant::now() + REFRESH_INTERVAL;
    let mut dirty = false;
    loop {
        let current = if dirty && watch.available() {
            index.committed_state(&scope)?
        } else {
            Some(index.state(&scope)?)
        };
        let writer_busy = current.is_none();
        if let Some(current) = current {
            dirty = false;
            if current.revision != state.revision {
                snapshot(&index, &scope, options, writer, b"fresh")?;
                state = current;
            }
        }
        if let Some(child) = &mut update
            && child.finished()?
        {
            if let Some(error) = child.take_error() {
                warning(writer, &error)?;
            }
            // Another already-running refresher may have won the scope lock.
            // Check it only after our child exits, avoiding a launch-time race.
            if index.lock(&scope, false)?.is_some() {
                // Read the last revision before reporting a completed refresh.
                let final_state = index.state(&scope)?;
                if final_state.revision != state.revision {
                    snapshot(&index, &scope, options, writer, b"fresh")?;
                    state = final_state;
                }
                message(writer, b"ready")?;
                update = None;
                next = Instant::now() + REFRESH_INTERVAL;
            }
        }
        if update.is_none() && Instant::now() >= next {
            message(writer, b"updating")?;
            update = Some(start_update(options)?);
        }
        // No read transaction remains open while sleeping. Active refreshes use
        // pidfds rather than a timer; the normal idle deadline is the next scan.
        let deadline = if update.is_some() {
            Instant::now() + REFRESH_INTERVAL
        } else {
            next
        };
        dirty |= watch.wait(output, update.as_ref(), deadline, writer_busy)?;
    }
}

pub fn stream(options: &Options) -> Result<()> {
    let mut writer = io::BufWriter::with_capacity(64 * 1024, io::stdout().lock());
    let output = writer.get_ref().as_raw_fd();
    if let Err(error) = stream_index(options, &mut writer, output) {
        if error
            .downcast_ref::<io::Error>()
            .is_some_and(|error| matches!(error.kind(), io::ErrorKind::BrokenPipe))
        {
            return Ok(());
        }
        // A disabled, corrupt or unwritable cache must not prevent file search.
        warning(
            &mut writer,
            &format!("Index unavailable; using a live scan: {error}"),
        )?;
        message(&mut writer, b"updating")?;
        let paths = sorted_paths(options)?;
        writer.write_all(b"fresh\0")?;
        for path in paths {
            writer.write_all(path.as_os_str().as_bytes())?;
            writer.write_all(b"\0")?;
        }
        writer.write_all(b"\0")?;
        message(&mut writer, b"ready")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::PermissionsExt},
        sync::{Arc, Barrier},
    };
    use tempfile::TempDir;

    struct Fixture {
        temp: TempDir,
        options: Options,
        index: Index,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = TempDir::new().unwrap();
            let root = temp.path().join("files");
            fs::create_dir(&root).unwrap();
            let options =
                Options::parse([OsString::from("--root"), root.into_os_string()]).unwrap();
            let index = Index::open(&temp.path().join("cache/index.sqlite3")).unwrap();
            Self {
                temp,
                options,
                index,
            }
        }
        fn write(&self, name: &str) -> PathBuf {
            let path = self.options.root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fixture").unwrap();
            path
        }
        fn refresh(&mut self) -> serde_json::Value {
            let scope = self.index.scope(&self.options).unwrap();
            self.index
                .refresh(&scope, &self.options, true)
                .unwrap()
                .unwrap()
        }
        fn paths(&self) -> Vec<PathBuf> {
            let scope = self.index.scope(&self.options).unwrap();
            let mut paths = Vec::new();
            self.index
                .query(&scope, &self.options, |bytes| {
                    paths.push(PathBuf::from(OsString::from_vec(bytes)));
                    Ok(())
                })
                .unwrap();
            paths
        }
    }

    fn quarantines(path: &Path) -> Vec<PathBuf> {
        let prefix = with_suffix(path, ".quarantine-");
        fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|entry| {
                entry
                    .as_os_str()
                    .as_bytes()
                    .starts_with(prefix.as_os_str().as_bytes())
            })
            .collect()
    }

    #[test]
    fn notified_reads_wait_for_the_writer_to_publish_its_commit_without_blocking() {
        let mut fixture = Fixture::new();
        fixture.write("cached.txt");
        fixture.refresh();
        let scope = fixture.index.scope(&fixture.options).unwrap();
        let before = fixture.index.state(&scope).unwrap().revision;
        let mut writer = Connection::open(&fixture.index.path).unwrap();
        let transaction = writer
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        transaction
            .execute("UPDATE scopes SET revision=revision+1", [])
            .unwrap();
        let began = Instant::now();
        assert!(fixture.index.committed_state(&scope).unwrap().is_none());
        assert!(began.elapsed() < Duration::from_secs(1));
        assert_eq!(fixture.index.state(&scope).unwrap().revision, before);
        let timeout: i64 = fixture
            .index
            .connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        assert_eq!(timeout, LOCK_TIMEOUT.as_millis() as i64);
        transaction.commit().unwrap();
        assert_eq!(
            fixture
                .index
                .committed_state(&scope)
                .unwrap()
                .unwrap()
                .revision,
            before + 1
        );
    }

    fn version(index: &Index) -> i64 {
        index
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn version_zero_migration_and_current_reopen_preserve_cached_scopes() {
        for old_version in [0, VERSION] {
            let mut fixture = Fixture::new();
            let file = fixture.write("keep.pdf");
            fixture.refresh();
            fixture.options.pdf = true;
            fixture.refresh();
            let scope = fixture.index.scope(&fixture.options).unwrap();
            let state = fixture.index.state(&scope).unwrap();
            let path = fixture.index.path.clone();
            fixture
                .index
                .connection
                .pragma_update(None, "user_version", old_version)
                .unwrap();
            drop(fixture.index);
            fixture.index = Index::open(&path).unwrap();
            assert_eq!(version(&fixture.index), VERSION);
            assert_eq!(fixture.paths().as_slice(), std::slice::from_ref(&file));
            let migrated = fixture.index.state(&scope).unwrap();
            assert_eq!(migrated.revision, state.revision);
            assert_eq!(migrated.updated, state.updated);
            fixture.options.pdf = false;
            assert_eq!(fixture.paths(), [file]);
            assert!(quarantines(&path).is_empty());
        }
    }

    #[test]
    fn unsupported_versions_are_quarantined_once_and_rebuilt_from_files() {
        for unsupported in [-1, VERSION + 1] {
            let mut fixture = Fixture::new();
            let stale = fixture.write("stale.txt");
            fixture.refresh();
            fs::remove_file(stale).unwrap();
            let fresh = fixture.write("fresh.txt");
            let path = fixture.index.path.clone();
            fixture
                .index
                .connection
                .pragma_update(None, "user_version", unsupported)
                .unwrap();
            drop(fixture.index);
            fixture.index = Index::open(&path).unwrap();
            assert_eq!(version(&fixture.index), VERSION);
            assert!(fixture.paths().is_empty());
            let scope = fixture.index.scope(&fixture.options).unwrap();
            assert!(fixture.index.state(&scope).unwrap().updated.is_none());
            assert_eq!(fixture.refresh()["added"], 1);
            assert_eq!(fixture.paths(), [fresh]);
            let saved = quarantines(&path);
            assert_eq!(saved.len(), 1);
            let old = Connection::open(saved[0].join(path.file_name().unwrap())).unwrap();
            assert_eq!(
                old.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                unsupported
            );
            assert_eq!(
                old.query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            drop(old);
            drop(fixture.index);
            fixture.index = Index::open(&path).unwrap();
            assert_eq!(fixture.paths().len(), 1);
            assert_eq!(quarantines(&path), saved);
        }
    }

    #[test]
    fn non_database_cache_and_sidecars_are_preserved_and_never_catalogued() {
        let temp = TempDir::new().unwrap();
        let path = temp
            .path()
            .join(OsString::from_vec(b"index-\xff.sqlite3".to_vec()));
        let contents = b"not a sqlite database";
        fs::write(&path, contents).unwrap();
        for suffix in ["-wal", "-shm", "-journal"] {
            fs::write(with_suffix(&path, suffix), suffix).unwrap();
        }
        let mut index = Index::open(&path).unwrap();
        let saved = quarantines(&path);
        assert_eq!(saved.len(), 1);
        assert_eq!(
            fs::metadata(&saved[0]).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::read(saved[0].join(path.file_name().unwrap())).unwrap(),
            contents
        );
        for suffix in ["-wal", "-shm", "-journal"] {
            let original = with_suffix(&path, suffix);
            assert_eq!(
                fs::read(saved[0].join(original.file_name().unwrap())).unwrap(),
                suffix.as_bytes()
            );
        }
        let file = temp.path().join("ordinary.txt");
        fs::write(&file, "fixture").unwrap();
        let options = Options::parse([
            OsString::from("--all"),
            OsString::from("--root"),
            temp.path().as_os_str().to_owned(),
        ])
        .unwrap();
        let scope = index.scope(&options).unwrap();
        assert_eq!(
            index.refresh(&scope, &options, true).unwrap().unwrap()["files"],
            1
        );
        let mut paths = Vec::new();
        index
            .query(&scope, &options, |bytes| {
                paths.push(bytes);
                Ok(())
            })
            .unwrap();
        assert_eq!(paths, [file.as_os_str().as_bytes()]);
    }

    #[test]
    fn unsupported_version_in_uncheckpointed_wal_is_quarantined_with_its_rows() {
        let mut fixture = Fixture::new();
        fixture.write("keep.txt");
        fixture.refresh();
        fixture
            .index
            .connection
            .pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        fixture
            .index
            .connection
            .set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
        let path = fixture.index.path.clone();
        drop(fixture.index);
        let database = fs::read(&path).unwrap();
        let wal = fs::read(with_suffix(&path, "-wal")).unwrap();
        let index = Index::open(&path).unwrap();
        assert_eq!(version(&index), VERSION);
        let saved = quarantines(&path);
        assert_eq!(saved.len(), 1);
        let old_path = saved[0].join(path.file_name().unwrap());
        assert_eq!(fs::read(&old_path).unwrap(), database);
        assert_eq!(fs::read(with_suffix(&old_path, "-wal")).unwrap(), wal);
        assert!(with_suffix(&old_path, "-shm").is_file());
        let old = Connection::open(old_path).unwrap();
        assert_eq!(
            old.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            VERSION + 1
        );
        assert_eq!(
            old.query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn corrupt_sqlite_schema_page_is_quarantined_and_recreated() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        drop(Index::open(&path).unwrap());
        let mut bytes = fs::read(&path).unwrap();
        bytes[100] = 0xff; // Invalid b-tree page type, after the SQLite header.
        fs::write(&path, &bytes).unwrap();
        let error = Index::connect(&path).err().unwrap();
        assert_eq!(
            error
                .downcast_ref::<rusqlite::Error>()
                .unwrap()
                .sqlite_error_code(),
            Some(ErrorCode::DatabaseCorrupt)
        );
        let index = Index::open(&path).unwrap();
        assert_eq!(version(&index), VERSION);
        let saved = quarantines(&path);
        assert_eq!(saved.len(), 1);
        assert_eq!(
            fs::read(saved[0].join(path.file_name().unwrap())).unwrap(),
            bytes
        );
        assert_eq!(
            index
                .connection
                .pragma_query_value(None, "integrity_check", |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }

    #[test]
    fn incomplete_schema_is_rebuilt_without_marking_it_migrated() {
        for old_version in [0, VERSION] {
            let temp = TempDir::new().unwrap();
            let path = temp.path().join("index.sqlite3");
            let old = Connection::open(&path).unwrap();
            old.execute_batch("CREATE TABLE scopes (id INTEGER PRIMARY KEY)")
                .unwrap();
            old.pragma_update(None, "user_version", old_version)
                .unwrap();
            drop(old);
            let index = Index::open(&path).unwrap();
            assert_eq!(version(&index), VERSION);
            let saved = quarantines(&path);
            assert_eq!(saved.len(), 1);
            let old = Connection::open(saved[0].join(path.file_name().unwrap())).unwrap();
            assert_eq!(
                old.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                old_version
            );
        }
    }

    #[test]
    fn concurrent_openers_share_one_recovered_database() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        fs::write(&path, b"broken").unwrap();
        let start = Arc::new(Barrier::new(4));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                let start = start.clone();
                thread::spawn(move || {
                    start.wait();
                    Index::open(&path).unwrap()
                })
            })
            .collect();
        let indices: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert_eq!(quarantines(&path).len(), 1);
        for index in indices {
            assert_eq!(version(&index), VERSION);
        }
    }

    #[test]
    fn recovery_waits_for_an_existing_picker_connection_to_close() {
        let fixture = Fixture::new();
        let path = fixture.index.path.clone();
        fixture
            .index
            .connection
            .pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        let error = Index::open(&path).err().unwrap();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(quarantines(&path).is_empty());
        drop(fixture.index);
        assert_eq!(version(&Index::open(&path).unwrap()), VERSION);
        assert_eq!(quarantines(&path).len(), 1);
    }

    #[test]
    fn busy_or_unusable_cache_paths_are_not_quarantined() {
        let fixture = Fixture::new();
        let path = fixture.index.path.clone();
        drop(fixture.index);
        let writer = Connection::open(&path).unwrap();
        writer
            .pragma_update(None, "journal_mode", "DELETE")
            .unwrap();
        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let error = Index::open(&path).err().unwrap();
        assert_eq!(
            error
                .downcast_ref::<rusqlite::Error>()
                .unwrap()
                .sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy)
        );
        assert!(quarantines(&path).is_empty());
        writer.execute_batch("ROLLBACK").unwrap();
        assert_eq!(version(&Index::open(&path).unwrap()), VERSION);
        let directory = fixture.temp.path().join("directory.sqlite3");
        fs::create_dir(&directory).unwrap();
        assert!(Index::open(&directory).is_err());
        assert!(directory.is_dir());
        assert!(quarantines(&directory).is_empty());
    }

    #[test]
    fn failed_quarantine_preserves_the_original_cache() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("index.sqlite3");
        fs::write(&path, b"broken").unwrap();
        let sidecar = with_suffix(&path, "-wal");
        fs::create_dir(&sidecar).unwrap();
        assert!(Index::open(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        assert!(sidecar.is_dir());
        assert!(quarantines(&path).is_empty());
    }

    #[test]
    fn refresh_adds_updates_renames_and_prunes_deleted_files_atomically() {
        let mut fixture = Fixture::new();
        let first = fixture.write("first.txt");
        let old = fixture.write("old.txt");
        assert_eq!(fixture.refresh()["added"], 2);
        let renamed = fixture.options.root.join("renamed.txt");
        fs::rename(&old, &renamed).unwrap();
        fs::write(&first, "changed content").unwrap();
        let report = fixture.refresh();
        assert_eq!(report["added"], 1);
        assert_eq!(report["updated"], 1);
        assert_eq!(report["removed"], 1);
        assert_eq!(
            fixture
                .paths()
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            [first.clone(), renamed].into_iter().collect()
        );
        fs::remove_file(first).unwrap();
        assert_eq!(fixture.refresh()["removed"], 1);
    }

    #[test]
    fn warm_reads_need_no_filesystem_scan_and_preserve_non_utf8_paths() {
        let mut fixture = Fixture::new();
        let path = fixture
            .options
            .root
            .join(OsString::from_vec(b"raw-\xff\n' file.pdf".to_vec()));
        fs::write(&path, "fixture").unwrap();
        fixture.refresh();
        fs::remove_file(&path).unwrap();
        assert_eq!(fixture.paths(), [path]); // The cached read performs no stat.
        fixture.refresh();
        assert!(fixture.paths().is_empty());
    }

    #[test]
    fn failed_scan_and_uncommitted_transaction_keep_previous_snapshot() {
        let mut fixture = Fixture::new();
        let path = fixture.write("keep.pdf");
        fixture.refresh();
        let scope = fixture.index.scope(&fixture.options).unwrap();
        let moved = fixture.temp.path().join("moved");
        fs::rename(&fixture.options.root, &moved).unwrap();
        assert!(
            fixture
                .index
                .refresh(&scope, &fixture.options, true)
                .is_err()
        );
        fs::rename(&moved, &fixture.options.root).unwrap();
        assert_eq!(fixture.paths().as_slice(), std::slice::from_ref(&path));
        {
            let transaction = fixture.index.connection.transaction().unwrap();
            transaction.execute("DELETE FROM files", []).unwrap();
        }
        assert_eq!(fixture.paths(), [path]);
    }

    #[test]
    fn scopes_keep_pdf_hidden_and_dependency_filters_separate() {
        let mut fixture = Fixture::new();
        let pdf = fixture.write("paper.pdf");
        let text = fixture.write("notes.txt");
        let hidden = fixture.write(".hidden.txt");
        let dependency = fixture.write("node_modules/package/internal.txt");
        fixture.refresh();
        assert_eq!(fixture.paths().len(), 2);
        fixture.options.pdf = true;
        fixture.refresh();
        assert_eq!(fixture.paths().as_slice(), std::slice::from_ref(&pdf));
        fixture.options.pdf = false;
        fixture.options.all = true;
        fixture.refresh();
        assert_eq!(
            fixture
                .paths()
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            [pdf, text, hidden, dependency].into_iter().collect()
        );
        fixture.options.all = false;
        assert_eq!(fixture.paths().len(), 2);
    }

    #[test]
    fn manual_remove_and_clear_only_change_cached_rows() {
        let mut fixture = Fixture::new();
        let pdf = fixture.write("paper.pdf");
        let text = fixture.write("notes.txt");
        fixture.refresh();
        fixture.options.pdf = true;
        fixture.refresh();
        assert_eq!(fixture.index.forget(std::slice::from_ref(&pdf)).unwrap(), 2);
        assert!(pdf.exists());
        assert!(fixture.paths().is_empty());
        fixture.options.pdf = false;
        assert_eq!(fixture.paths().as_slice(), std::slice::from_ref(&text));
        assert_eq!(fixture.index.clear_scope(&fixture.options).unwrap(), 1);
        assert!(fixture.paths().is_empty());
        assert!(text.exists());
        assert_eq!(fixture.refresh()["added"], 2);
    }

    #[test]
    fn readers_remain_available_during_writes_and_refreshes_are_coalesced() {
        let mut fixture = Fixture::new();
        let file = fixture.write("keep.txt");
        fixture.refresh();
        let scope = fixture.index.scope(&fixture.options).unwrap();
        let reader = Index::open(&fixture.index.path).unwrap();
        let _lock = fixture.index.lock(&scope, true).unwrap().unwrap();
        assert!(reader.lock(&scope, false).unwrap().is_none());
        let transaction = fixture
            .index
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        transaction.execute("DELETE FROM files", []).unwrap();
        let mut paths = Vec::new();
        reader
            .query(&scope, &fixture.options, |path| {
                paths.push(PathBuf::from(OsString::from_vec(path)));
                Ok(())
            })
            .unwrap();
        assert_eq!(paths, [file]);
    }

    #[test]
    fn unchanged_scan_keeps_revision_and_index_never_catalogs_itself() {
        let mut fixture = Fixture::new();
        fixture.options.root = fixture.temp.path().to_owned();
        fixture.options.all = true;
        fixture.write("ordinary.txt");
        fixture.refresh();
        let scope = fixture.index.scope(&fixture.options).unwrap();
        let revision = fixture.index.state(&scope).unwrap().revision;
        assert_eq!(fixture.refresh()["added"], 0);
        assert_eq!(fixture.index.state(&scope).unwrap().revision, revision);
        assert!(
            fixture
                .paths()
                .iter()
                .all(|path| !fixture.index.own_file(path))
        );
        assert_eq!(fixture.paths().len(), 1);
    }

    #[test]
    fn cached_recent_set_and_name_order_match_live_sorting() {
        let mut fixture = Fixture::new();
        fixture.write("aaa.pdf");
        thread::sleep(Duration::from_millis(10));
        fixture.write("zzz.pdf");
        fixture.refresh();
        for recent in [false, true] {
            fixture.options.recent = recent;
            for sort in [SortOrder::Name, SortOrder::Recent] {
                fixture.options.sort = sort;
                assert_eq!(fixture.paths(), sorted_paths(&fixture.options).unwrap());
            }
        }
    }

    #[test]
    fn recent_name_sort_uses_the_newest_30000_and_does_not_truncate_shared_cache() {
        let mut fixture = Fixture::new();
        let scope = fixture.index.scope(&fixture.options).unwrap();
        fixture
            .index
            .connection
            .execute(
                "WITH RECURSIVE seq(n) AS (
            VALUES(0) UNION ALL SELECT n+1 FROM seq WHERE n<30004)
            INSERT INTO files(scope,path,changed_sec,changed_nsec)
            SELECT ?1,CAST(printf('/tmp/file-%05d',n) AS BLOB),n,0 FROM seq",
                [scope.id],
            )
            .unwrap();
        fixture.options.recent = true;
        fixture.options.sort = SortOrder::Name;
        let paths = fixture.paths();
        assert_eq!(paths.len(), 30000);
        assert_eq!(paths.first().unwrap(), Path::new("/tmp/file-00005"));
        assert_eq!(paths.last().unwrap(), Path::new("/tmp/file-30004"));
        fixture.options.recent = false;
        assert_eq!(fixture.paths().len(), 30005);
    }
}
