//! Editor saves become new history entries while originals remain immutable.
use crate::{backend::Backend, config::private_directory, store::Capture};
use anyhow::{Result, ensure};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::Read,
    os::{fd::FromRawFd, unix::ffi::OsStrExt},
    sync::{Arc, atomic::Ordering},
};

pub fn start(backend: Arc<Backend>) -> Result<()> {
    let directory = backend.paths.state.join("edits");
    private_directory(&directory)?;
    let name = CString::new(directory.as_os_str().as_bytes())?;
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    ensure!(fd >= 0, "Unable to watch clipboard editor saves");
    let mut events = unsafe { File::from_raw_fd(fd) };
    ensure!(
        unsafe {
            libc::inotify_add_watch(fd, name.as_ptr(), libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO)
        } >= 0,
        "Unable to watch the clipboard edit directory"
    );
    std::thread::Builder::new()
        .name("search-editor-saves".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let mut buffer = [0_u8; 16 * 1024];
            while let Ok(size) = events.read(&mut buffer) {
                if size == 0 {
                    break;
                }
                let mut offset = 0;
                while offset + std::mem::size_of::<libc::inotify_event>() <= size {
                    let event = unsafe {
                        std::ptr::read_unaligned(
                            buffer.as_ptr().add(offset).cast::<libc::inotify_event>(),
                        )
                    };
                    let start = offset + std::mem::size_of::<libc::inotify_event>();
                    let end = start + event.len as usize;
                    if end > size {
                        break;
                    }
                    if let Ok(name) = CStr::from_bytes_until_nul(&buffer[start..end])
                        && let Ok(name) = name.to_str()
                        && !name.starts_with('.')
                        && !name.ends_with('~')
                    {
                        let path = directory.join(name);
                        if path
                            .symlink_metadata()
                            .is_ok_and(|metadata| metadata.file_type().is_file())
                        {
                            let mime = match path
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
                                _ => "text/plain;charset=utf-8",
                            };
                            let result = (|| -> Result<()> {
                                let mut file = File::open(&path)?;
                                backend.store.lock().unwrap().capture(Capture {
                                    mime,
                                    reader: &mut file,
                                    identifier: None,
                                    pinned: false,
                                    label: None,
                                    import_position: Some(
                                        backend.position.fetch_add(1, Ordering::Relaxed),
                                    ),
                                })?;
                                Ok(())
                            })();
                            if result.is_ok() {
                                backend.events.notify("clipboard");
                            } else if let Err(error) = result {
                                eprintln!("desktop-search: editor save rejected: {error}");
                            }
                        }
                    }
                    offset = end;
                }
            }
        })?;
    Ok(())
}
