use crate::job::Cancellation;
use anyhow::{Context, Result, ensure};
use std::{
    ffi::{CStr, CString},
    fs::{self, File},
    io,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

pub struct TemporaryOutput {
    directory: PathBuf,
    pub path: PathBuf,
}

impl TemporaryOutput {
    pub fn new(parent: &Path, format: &str) -> Result<Self> {
        let template = parent.join(".desktop-transcode-XXXXXX");
        let mut bytes = CString::new(template.as_os_str().as_bytes())?.into_bytes_with_nul();
        // SAFETY: mkdtemp gets a writable, NUL-terminated template. It creates
        // an unpredictable private (0700) directory on the destination FS.
        let directory = unsafe {
            let pointer = libc::mkdtemp(bytes.as_mut_ptr().cast());
            if pointer.is_null() {
                return Err(io::Error::last_os_error())
                    .context("Could not create temporary output");
            }
            PathBuf::from(std::ffi::OsString::from_vec(
                CStr::from_ptr(pointer).to_bytes().to_vec(),
            ))
        };
        let path = directory.join(format!("output.{format}"));
        Ok(Self { directory, path })
    }

    pub fn publish(&self, destination: &Path, cancel: &Cancellation) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.path).context("Encoder produced no output")?;
        ensure!(
            metadata.is_file() && metadata.len() > 0,
            "Encoder produced no complete file"
        );
        File::open(&self.path)?
            .sync_all()
            .context("Could not flush encoded output")?;
        let source = CString::new(self.path.as_os_str().as_bytes())?;
        let destination_c = CString::new(destination.as_os_str().as_bytes())?;
        cancel.check()?;
        // SAFETY: both paths are NUL-terminated and remain live during the
        // syscall. RENAME_NOREPLACE is one atomic move: existing files, links
        // (including dangling links), and directories are never replaced.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                source.as_ptr(),
                libc::AT_FDCWD,
                destination_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error()).with_context(|| {
                format!(
                    "Could not publish output without replacing {}",
                    destination.display()
                )
            });
        }
        Ok(())
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            eprintln!("Could not clean up {}: {error}", self.directory.display());
        }
    }
}
