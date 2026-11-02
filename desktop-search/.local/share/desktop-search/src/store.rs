use crate::{config::private_directory, protocol::Item};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const PREVIEW_CHARS: usize = 20_000;
static TEMPORARY: AtomicU64 = AtomicU64::new(0);

pub struct Store {
    pub db: Connection,
    pub blobs: PathBuf,
    pub max_items: usize,
    pub max_bytes: u64,
}

pub struct Capture<'a> {
    pub mime: &'a str,
    pub reader: &'a mut dyn Read,
    pub identifier: Option<&'a str>,
    pub pinned: bool,
    pub label: Option<&'a str>,
    // Imported entries use negative positions; live selections use a monotonic
    // positive position assigned before their payload starts transferring.
    pub import_position: Option<i64>,
}

impl Store {
    pub fn open(root: &Path, max_items: usize, max_bytes: u64) -> Result<Self> {
        private_directory(root)?;
        let blobs = root.join("blobs");
        private_directory(&blobs)?;
        let path = root.join("history.sqlite3");
        if !path.exists() {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
        }
        ensure!(
            path.symlink_metadata()?.file_type().is_file(),
            "History must be a regular file"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             PRAGMA cache_size=-512;
             PRAGMA mmap_size=0;
             PRAGMA temp_store=FILE;
             PRAGMA journal_size_limit=1048576;
             CREATE TABLE IF NOT EXISTS clips (
                 sequence INTEGER PRIMARY KEY,
                 id TEXT NOT NULL UNIQUE,
                 hash TEXT NOT NULL UNIQUE,
                 mime TEXT NOT NULL,
                 path TEXT NOT NULL,
                 preview TEXT NOT NULL,
                 label TEXT NOT NULL,
                 pinned INTEGER NOT NULL DEFAULT 0,
                 position INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS clips_order ON clips(pinned DESC, position DESC);
             CREATE VIRTUAL TABLE IF NOT EXISTS clip_search USING fts5(text);
             CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS usage (
                 provider TEXT NOT NULL, id TEXT NOT NULL,
                 count INTEGER NOT NULL DEFAULT 0, last_used INTEGER NOT NULL,
                 PRIMARY KEY(provider,id)
             );
             CREATE TABLE IF NOT EXISTS todos (
                 id TEXT PRIMARY KEY, text TEXT NOT NULL, category TEXT NOT NULL DEFAULT '',
                 state TEXT NOT NULL DEFAULT 'inactive', urgency TEXT NOT NULL DEFAULT 'normal',
                 created INTEGER NOT NULL, scheduled INTEGER, started INTEGER, finished INTEGER,
                 notified INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        let mut store = Self {
            db,
            blobs,
            max_items,
            max_bytes,
        };
        store.prune()?;
        store.remove_orphans()?;
        Ok(store)
    }

    pub fn capture(&mut self, mut capture: Capture<'_>) -> Result<String> {
        if matches!(
            capture.mime,
            "text/plain"
                | "text/plain;charset=UTF-8"
                | "text/plain; charset=utf-8"
                | "text/plain; charset=UTF-8"
        ) {
            capture.mime = "text/plain;charset=utf-8";
        }
        ensure!(
            supported_mime(capture.mime),
            "Unsupported clipboard MIME type"
        );
        let temporary = self.blobs.join(format!(
            ".incoming-{}-{}",
            std::process::id(),
            TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let result = self.write_capture(&temporary, capture);
        let _ = std::fs::remove_file(&temporary);
        result
    }

    fn write_capture(&mut self, temporary: &Path, capture: Capture<'_>) -> Result<String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(temporary)?;
        let mut digest = Sha256::new();
        digest.update(capture.mime.as_bytes());
        digest.update([0]);
        let mut size = 0_u64;
        let mut buffer = [0; 16 * 1024];
        loop {
            let count = capture.reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            size += count as u64;
            ensure!(
                size <= self.max_bytes,
                "Clipboard payload exceeds the configured limit"
            );
            file.write_all(&buffer[..count])?;
            digest.update(&buffer[..count]);
        }
        ensure!(size > 0, "Empty clipboard payload");
        file.sync_all()?;
        drop(file);
        let hash = format!("{:x}", digest.finalize());
        let destination = self
            .blobs
            .join(format!("{hash}.{}", extension(capture.mime)));
        let text = if capture.mime.starts_with("text/") {
            // The full value is indexed on disk; only a bounded preview is sent
            // to frontends. Copy/paste always reads the unmodified payload file.
            String::from_utf8_lossy(&std::fs::read(temporary)?).into_owned()
        } else {
            String::new()
        };
        let preview: String = text.chars().take(PREVIEW_CHARS).collect();
        if !destination.exists() {
            std::fs::rename(temporary, &destination)?;
            File::open(&self.blobs)?.sync_all()?;
        }
        let existing: Option<(String, i64)> = self
            .db
            .query_row(
                "SELECT id,sequence FROM clips WHERE hash=?1",
                [&hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let label = capture.label.map(str::to_owned).unwrap_or_else(|| {
            chrono::Local::now()
                .format("%a, %d %b %Y %H:%M:%S")
                .to_string()
        });
        let transaction = self.db.transaction()?;
        let position = if let Some(position) = capture.import_position {
            position
        } else {
            transaction.query_row(
                "SELECT MAX(COALESCE(MAX(position),0),0)+1 FROM clips",
                [],
                |row| row.get::<_, i64>(0),
            )?
        };
        let id = if let Some((id, _)) = existing {
            if capture.import_position.is_some_and(|position| position < 0) {
                transaction.execute(
                    "UPDATE clips SET pinned=MAX(pinned,?2) WHERE id=?1",
                    params![id, capture.pinned],
                )?;
            } else {
                transaction.execute(
                    "UPDATE clips SET label=CASE WHEN position<=?2 THEN ?3 ELSE label END,position=MAX(position,?2),pinned=MAX(pinned,?4) WHERE id=?1",
                    params![id, position, label, capture.pinned],
                )?;
            }
            id
        } else {
            let id = capture.identifier.unwrap_or(&hash).to_owned();
            ensure!(
                id.len() <= 4096 && !id.is_empty(),
                "Invalid clipboard identifier"
            );
            transaction.execute(
                "INSERT INTO clips(id,hash,mime,path,preview,label,pinned,position) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![id,hash,capture.mime,destination.to_string_lossy(),preview,label,capture.pinned,position],
            )?;
            let sequence = transaction.last_insert_rowid();
            transaction.execute(
                "INSERT INTO clip_search(rowid,text) VALUES(?1,?2)",
                params![sequence, text],
            )?;
            id
        };
        transaction.commit()?;
        self.prune()?;
        Ok(id)
    }

    pub fn list(&self, query: &str, limit: usize, mode: &str) -> Result<Vec<Item>> {
        let terms = query
            .split_whitespace()
            .map(|word| format!("\"{}\"*", word.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let has_terms = query.chars().any(char::is_alphanumeric);
        let mut statement = self.db.prepare(
            "SELECT id,mime,path,preview,label,pinned FROM clips
             WHERE (?1='' OR (?3 AND sequence IN (SELECT rowid FROM clip_search WHERE clip_search MATCH ?2))
                    OR (NOT ?3 AND instr(preview,?1)>0))
               AND (?4!='show_images_only' OR mime LIKE 'image/%')
               AND (?4!='show_text_only' OR mime LIKE 'text/%')
               AND (?4!='show_pinned_only' OR pinned=1)
             ORDER BY pinned DESC,position DESC LIMIT ?5",
        )?;
        let rows = statement.query_map(
            params![query, terms, has_terms, mode, limit.min(100_000) as i64],
            |row| {
                let mime: String = row.get(1)?;
                let path: String = row.get(2)?;
                let preview: String = row.get(3)?;
                let pinned: bool = row.get(5)?;
                let image = mime.starts_with("image/");
                let text = if image {
                    format!("{} image", mime.trim_start_matches("image/").to_uppercase())
                } else {
                    preview
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(180)
                        .collect()
                };
                let mut actions: Vec<String> = ["copy", "paste", "edit", "pin"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect();
                if pinned {
                    actions[3] = "unpin".into();
                } else {
                    actions.push("remove".into());
                }
                Ok(Item {
                    identifier: row.get(0)?,
                    text,
                    subtext: row.get(4)?,
                    provider: "clipboard".into(),
                    mimetype: mime,
                    preview: if image { path.clone() } else { preview },
                    preview_type: if image { "file" } else { "text" }.into(),
                    payload_path: path,
                    state: if pinned {
                        vec!["pinned".into()]
                    } else {
                        vec![]
                    },
                    actions,
                    ..Default::default()
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn payload(&self, identifier: &str) -> Result<(String, PathBuf)> {
        self.db
            .query_row(
                "SELECT mime,path FROM clips WHERE id=?1",
                [identifier],
                |row| Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?))),
            )
            .context("Clipboard item no longer exists")
    }

    pub fn pin(&mut self, identifier: &str, value: bool) -> Result<()> {
        ensure!(
            self.db.execute(
                "UPDATE clips SET pinned=?2 WHERE id=?1",
                params![identifier, value]
            )? == 1,
            "Clipboard item no longer exists"
        );
        self.prune()
    }

    pub fn remove(&mut self, identifier: &str) -> Result<()> {
        let pinned: bool = self.db.query_row(
            "SELECT pinned FROM clips WHERE id=?1",
            [identifier],
            |row| row.get(0),
        )?;
        ensure!(!pinned, "Unpin this item before removing it");
        self.delete("id=?1", identifier)?;
        Ok(())
    }

    pub fn clear_unpinned(&mut self) -> Result<()> {
        self.delete("pinned=0", "")
    }

    fn delete(&mut self, predicate: &str, value: &str) -> Result<()> {
        let mut statement = self.db.prepare(&format!(
            "SELECT sequence,path FROM clips WHERE {predicate}"
        ))?;
        let values = if predicate.contains("?1") {
            vec![value]
        } else {
            vec![]
        };
        let removed = statement
            .query_map(rusqlite::params_from_iter(values), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let transaction = self.db.transaction()?;
        for (sequence, _) in &removed {
            transaction.execute("DELETE FROM clip_search WHERE rowid=?1", [sequence])?;
            transaction.execute("DELETE FROM clips WHERE sequence=?1", [sequence])?;
        }
        transaction.commit()?;
        for (_, path) in removed {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    fn prune(&mut self) -> Result<()> {
        let pinned: i64 =
            self.db
                .query_row("SELECT COUNT(*) FROM clips WHERE pinned=1", [], |row| {
                    row.get(0)
                })?;
        let keep = self.max_items.saturating_sub(pinned as usize);
        self.delete(&format!("pinned=0 AND sequence IN (SELECT sequence FROM clips WHERE pinned=0 ORDER BY position DESC LIMIT -1 OFFSET {keep})"),"")
    }

    pub fn setting(&self, key: &str) -> Result<String> {
        Ok(self
            .db
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value])?;
        Ok(())
    }

    fn remove_orphans(&self) -> Result<()> {
        for file in std::fs::read_dir(&self.blobs)? {
            let file = file?;
            if !file.file_type()?.is_file() {
                continue;
            }
            let referenced: bool = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM clips WHERE path=?1)",
                [file.path().to_string_lossy()],
                |row| row.get(0),
            )?;
            if !referenced {
                std::fs::remove_file(file.path())?;
            }
        }
        Ok(())
    }
}

pub fn supported_mime(mime: &str) -> bool {
    mime.starts_with("text/")
        || matches!(
            mime,
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "image/bmp" | "image/tiff"
        )
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "txt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(store: &mut Store, text: &[u8], pinned: bool) -> String {
        store
            .capture(Capture {
                mime: "text/plain;charset=utf-8",
                reader: &mut &text[..],
                identifier: None,
                pinned,
                label: None,
                import_position: None,
            })
            .unwrap()
    }

    #[test]
    fn retention_deduplication_and_pins_survive_reopening() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path(), 3, 1024).unwrap();
        let pinned = put(&mut store, b"pinned", true);
        for index in 0..7 {
            put(&mut store, format!("item {index}").as_bytes(), false);
        }
        let items = store.list("", 100, "").unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].identifier, pinned);
        assert_eq!(items[1].preview, "item 6");
        assert_eq!(items[2].preview, "item 5");
        let recent = put(&mut store, b"item 5", false);
        assert_eq!(store.list("", 100, "").unwrap()[1].identifier, recent);
        assert!(store.remove(&pinned).is_err());
        drop(store);
        let mut store = Store::open(root.path(), 3, 1024).unwrap();
        assert_eq!(store.list("", 100, "").unwrap().len(), 3);
        store.clear_unpinned().unwrap();
        assert_eq!(store.list("", 100, "").unwrap()[0].identifier, pinned);
        assert_eq!(std::fs::read_dir(&store.blobs).unwrap().count(), 1);
    }

    #[test]
    fn long_text_has_a_bounded_preview_and_an_exact_searchable_payload() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path(), 100, 1024 * 1024).unwrap();
        let text = format!(
            "{}\nneedle-after-preview\næ <literal> $HOME",
            "a".repeat(50_000)
        );
        let id = put(&mut store, text.as_bytes(), false);
        let items = store.list("needle", 100, "").unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].preview.len() <= PREVIEW_CHARS);
        let (_, path) = store.payload(&id).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), text.as_bytes());
        assert_eq!(
            store.blobs.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            root.path()
                .join("history.sqlite3")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn rejected_capture_never_changes_history_or_leaves_a_partial_file() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path(), 100, 8).unwrap();
        put(&mut store, b"safe", false);
        assert!(
            store
                .capture(Capture {
                    mime: "text/plain",
                    reader: &mut &b"too much text"[..],
                    identifier: None,
                    pinned: false,
                    label: None,
                    import_position: None
                })
                .is_err()
        );
        assert_eq!(store.list("", 100, "").unwrap().len(), 1);
        assert_eq!(std::fs::read_dir(&store.blobs).unwrap().count(), 1);
    }

    #[test]
    fn imported_pins_merge_without_reordering_new_captures() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path(), 2, 1024).unwrap();
        put(&mut store, b"new", false);
        for index in (0..4).rev() {
            let value = format!("old {index}");
            store
                .capture(Capture {
                    mime: "text/plain",
                    reader: &mut value.as_bytes(),
                    identifier: Some(&format!("legacy-{index}")),
                    pinned: true,
                    label: Some("original time"),
                    import_position: Some(-(index + 1)),
                })
                .unwrap();
        }
        let items = store.list("", 100, "").unwrap();
        assert_eq!(items.len(), 4);
        assert!(items.iter().all(|item| item.state == ["pinned"]));
        assert_eq!(items[0].identifier, "legacy-0");
    }

    #[test]
    fn slow_duplicate_transfers_cannot_reorder_newer_selections() {
        let root = tempfile::tempdir().unwrap();
        let mut store = Store::open(root.path(), 100, 1024).unwrap();
        for (value, mime, position) in [
            ("same", "text/plain", 3),
            ("other", "text/plain", 2),
            ("same", "text/plain;charset=UTF-8", 1),
        ] {
            store
                .capture(Capture {
                    mime,
                    reader: &mut value.as_bytes(),
                    identifier: None,
                    pinned: false,
                    label: None,
                    import_position: Some(position),
                })
                .unwrap();
        }
        let items = store.list("", 100, "").unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].preview, "same");
        assert_eq!(items[1].preview, "other");
    }
}
