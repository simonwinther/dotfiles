use crate::{
    api::{Api, Pending, escaped},
    process,
};
use anyhow::{Context, Result, bail, ensure};
use reqwest::Method;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::Duration,
};

const CATEGORIES: [&str; 5] = ["Pictures", "Videos", "Documents", "Audio", "Files"];
const DOCUMENTS: &[&str] = &[
    "pdf", "doc", "docx", "odt", "rtf", "xls", "xlsx", "ods", "ppt", "pptx", "odp", "pages",
    "numbers", "key", "epub", "txt", "md", "csv", "json", "xml", "yaml", "yml",
];

fn private_dir(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e).with_context(|| format!("Could not create {}", path.display())),
    }
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { libc::getuid() },
        "Unsafe receiving directory: {}",
        path.display()
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | "..") && !name.bytes().any(|c| c == b'/' || c == 0)
}

fn category(source: &Path, name: &str) -> Result<&'static str> {
    let mut mime = mime_guess::from_path(name)
        .first_raw()
        .unwrap_or("")
        .to_owned();
    if mime.is_empty() || mime == "application/octet-stream" {
        let output = process::capture(
            Command::new("file")
                .args(["--brief", "--mime-type", "--"])
                .arg(source),
            Some(Duration::from_secs(10)),
        )?;
        mime = if output.status.success() {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        } else {
            String::new()
        };
    }
    for (prefix, folder) in [
        ("image/", "Pictures"),
        ("video/", "Videos"),
        ("audio/", "Audio"),
        ("text/", "Documents"),
    ] {
        if mime.starts_with(prefix) {
            return Ok(folder);
        }
    }
    let extension = Path::new(name)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    Ok(if DOCUMENTS.contains(&extension.as_str()) {
        "Documents"
    } else {
        "Files"
    })
}

pub struct Receiver {
    root: PathBuf,
    api: Api,
    _lock: File,
}

impl Receiver {
    pub fn new(root: PathBuf, api: Api) -> Result<Self> {
        private_dir(&root)?;
        let incoming = root.join(".incoming");
        private_dir(&incoming)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(incoming.join("receiver.lock"))?;
        let m = lock.metadata()?;
        ensure!(
            m.is_file() && m.uid() == unsafe { libc::getuid() } && m.nlink() == 1,
            "Unsafe receiver lock file"
        );
        ensure!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "Another receiver is already using {}",
            root.display()
        );
        for folder in CATEGORIES {
            private_dir(&root.join(folder))?;
        }
        for entry in fs::read_dir(&incoming)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type()?.is_file()
                && name.starts_with("receive-")
                && name.ends_with(".part")
            {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(Self {
            root,
            api,
            _lock: lock,
        })
    }

    fn publish(&self, source: &Path, name: &str) -> Result<PathBuf> {
        let folder = self.root.join(category(source, name)?);
        private_dir(&folder)?;
        let filename = Path::new(name);
        let stem = filename.file_stem().unwrap_or_default().to_string_lossy();
        let suffix = filename
            .extension()
            .map(|s| format!(".{}", s.to_string_lossy()))
            .unwrap_or_default();
        for counter in 0..100_000 {
            let candidate = folder.join(if counter == 0 {
                name.to_owned()
            } else {
                format!("{stem} ({counter}){suffix}")
            });
            match fs::hard_link(source, &candidate) {
                Ok(()) => {
                    // The downloaded inode was synced before publication. Make its
                    // directory entry durable before acknowledging Tailscale.
                    File::open(&folder)?.sync_all()?;
                    return Ok(candidate);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                    bail!("The receiving folders must share a filesystem")
                }
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("Could not save {}", candidate.display()));
                }
            }
        }
        bail!("Too many existing copies of {name:?}")
    }

    fn receive(&self, item: Pending) -> Result<PathBuf> {
        ensure!(valid_name(&item.name), "The received filename is invalid");
        ensure!(item.size <= i64::MAX as u64, "Invalid received file size");
        let path = escaped(&item.name);
        let response = self.api.request(Method::GET, &path, 200)?;
        if let Some(length) = response.content_length() {
            ensure!(
                length == item.size,
                "The transfer of {:?} has an unexpected length",
                item.name
            );
        }
        let mut output = tempfile::Builder::new()
            .prefix("receive-")
            .suffix(".part")
            .tempfile_in(self.root.join(".incoming"))?;
        let copied = io::copy(&mut response.take(item.size + 1), output.as_file_mut())
            .with_context(|| format!("The transfer of {:?} was incomplete", item.name))?;
        ensure!(
            copied == item.size,
            "The transfer of {:?} was incomplete",
            item.name
        );
        output.as_file().sync_all()?;
        let destination = self.publish(output.path(), &item.name)?;
        // Publication never replaces another file. A failed acknowledgement leaves
        // the completed local copy and remote inbox copy available for recovery.
        self.api
            .request(Method::DELETE, &path, 204)
            .with_context(|| {
                format!(
                    "Saved {} but could not acknowledge it",
                    destination.display()
                )
            })?;
        println!("Received {:?} into {:?}", item.name, destination);
        Ok(destination)
    }

    pub fn batch(&self, wait: bool) -> Result<()> {
        let mut first_error = None;
        for item in self.api.pending(wait)? {
            if let Err(error) = self.receive(item) {
                eprintln!("Could not receive file: {error:#}");
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    pub fn run(&self) -> ! {
        loop {
            if let Err(error) = self.batch(true) {
                eprintln!("Could not receive files: {error:#}");
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}
