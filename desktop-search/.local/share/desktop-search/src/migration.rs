use crate::{
    backend::Backend,
    client,
    protocol::{Activation, Query},
    store::{Capture, supported_mime},
    todo,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};

#[derive(Default, Serialize)]
pub struct Imported {
    pub clipboard: usize,
    pub pinned: usize,
    pub images: usize,
    pub tasks: usize,
    pub calculations: usize,
}

pub fn import(backend: &Backend, socket: &Path) -> Result<Imported> {
    ensure!(
        socket != backend.paths.socket,
        "Cannot import from the current search service"
    );
    let connection = client::connect(socket)?;
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                connection.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut size,
            )
        } == 0,
        "Cannot verify legacy provider"
    );
    ensure!(
        credentials.pid != std::process::id() as i32,
        "The legacy collector is no longer running"
    );
    drop(connection);
    let previous = client::state(socket, "clipboard")?;
    let previous_mode = previous
        .states
        .iter()
        .find(|state| state.starts_with("show_"))
        .cloned()
        .unwrap_or_else(|| "show_combined".into());
    client::activate(
        socket,
        &Activation {
            provider: "clipboard".into(),
            action: "show_combined".into(),
            ..Default::default()
        },
    )?;
    let result = import_combined(backend, socket);
    let restored = client::activate(
        socket,
        &Activation {
            provider: "clipboard".into(),
            action: previous_mode,
            ..Default::default()
        },
    );
    let imported = result?;
    restored?;
    backend
        .store
        .lock()
        .unwrap()
        .set_setting("legacy_imported", "true")?;
    backend.events.notify("clipboard");
    backend.events.notify("todo");
    Ok(imported)
}

fn import_combined(backend: &Backend, socket: &Path) -> Result<Imported> {
    let backup = backend.paths.state.join(format!(
        "legacy-backup-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S-%f")
    ));
    crate::config::private_directory(&backup)?;
    for name in ["clipboard.gob", "todo.csv", "calc.gob"] {
        let source = backend.paths.cache.join("elephant").join(name);
        if source.is_file() {
            let mut destination = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(backup.join(name))?;
            std::io::copy(&mut File::open(source)?, &mut destination)?;
            destination.sync_all()?;
        }
    }
    let mut imported = Imported::default();
    let mut position = -1_i64;
    client::query(
        socket,
        &Query {
            providers: vec!["clipboard".into()],
            maxresults: 100_000,
            ..Default::default()
        },
        |item| {
            let pinned = item.state.iter().any(|state| state == "pinned");
            let uri_list = item.preview.starts_with("file://") && !item.text.starts_with("file://");
            let mime = if supported_mime(&item.mimetype) {
                item.mimetype.as_str()
            } else if uri_list {
                "text/uri-list"
            } else if item.preview_type == "file" {
                match Path::new(&item.preview)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .unwrap_or("")
                {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "webp" => "image/webp",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    "tif" | "tiff" => "image/tiff",
                    _ => "text/uri-list",
                }
            } else {
                "text/plain;charset=utf-8"
            };
            let path = if !item.payload_path.is_empty() {
                Some(Path::new(&item.payload_path))
            } else if item.preview_type == "file" && mime.starts_with("image/") {
                Some(Path::new(&item.preview))
            } else {
                None
            };
            let mut file = path.map(File::open).transpose()?;
            let text = if mime == "text/uri-list" && item.preview_type == "file" && !uri_list {
                format!("file://{}\r\n", item.preview).into_bytes()
            } else {
                item.preview.as_bytes().to_vec()
            };
            let mut slice = text.as_slice();
            let reader: &mut dyn std::io::Read = if let Some(file) = &mut file {
                file
            } else {
                &mut slice
            };
            backend.store.lock().unwrap().capture(Capture {
                mime,
                reader,
                identifier: Some(&item.identifier),
                pinned,
                label: Some(&item.subtext),
                import_position: Some(position),
            })?;
            imported.clipboard += 1;
            imported.pinned += usize::from(pinned);
            imported.images += usize::from(mime.starts_with("image/"));
            position -= 1;
            Ok(())
        },
    )?;
    imported.tasks = todo::import(backend, &backend.paths.cache.join("elephant/todo.csv"))?;
    client::query(
        socket,
        &Query {
            providers: vec!["calc".into()],
            maxresults: 100_000,
            ..Default::default()
        },
        |item| {
            if item.provider == "calc" && item.state.iter().any(|value| value == "saved") {
                backend
                    .store
                    .lock()
                    .unwrap()
                    .set_setting(&format!("calc:import-{}", item.identifier), &item.text)?;
                imported.calculations += 1;
            }
            Ok(())
        },
    )?;
    let mut manifest = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(backup.join("import.json"))?;
    manifest.write_all(&serde_json::to_vec_pretty(&imported)?)?;
    manifest.sync_all()?;
    Ok(imported)
}
