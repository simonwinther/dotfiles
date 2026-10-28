//! Kernel waits for the cache feed. Notifications are hints; SQLite remains the
//! authority. A missing watch/pidfd keeps the bounded polling fallback.
use std::{
    ffi::CString,
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::ffi::OsStrExt,
    },
    path::Path,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

pub(super) const FALLBACK: Duration = Duration::from_millis(100);
const DIAGNOSTIC_LIMIT: usize = 16 * 1024;

pub(super) struct Refresh {
    child: Child,
    pidfd: Option<OwnedFd>,
    status: Option<ExitStatus>,
    reported: bool,
    diagnostic: Vec<u8>,
    truncated: bool,
}

impl Refresh {
    pub(super) fn new(child: Child) -> io::Result<Self> {
        if let Some(stderr) = &child.stderr {
            let flags = unsafe { libc::fcntl(stderr.as_raw_fd(), libc::F_GETFL) };
            if flags < 0
                || unsafe {
                    libc::fcntl(stderr.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id(), 0) };
        let pidfd = if fd >= 0 {
            Some(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
        } else {
            None
        };
        Ok(Self {
            child,
            pidfd,
            status: None,
            reported: false,
            diagnostic: Vec::new(),
            truncated: false,
        })
    }

    fn drain(&mut self) -> io::Result<()> {
        let Some(stderr) = self.child.stderr.as_mut() else {
            return Ok(());
        };
        let mut buffer = [0; 8192];
        // Bound a wake even if a faulty child writes continuously. Keep draining
        // on later wakes, but never retain unbounded diagnostics in memory.
        for _ in 0..8 {
            match stderr.read(&mut buffer) {
                Ok(0) => {
                    self.child.stderr.take();
                    break;
                }
                Ok(count) => {
                    let keep = count.min(DIAGNOSTIC_LIMIT.saturating_sub(self.diagnostic.len()));
                    self.diagnostic.extend_from_slice(&buffer[..keep]);
                    self.truncated |= keep < count;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(super) fn finished(&mut self) -> io::Result<bool> {
        self.drain()?;
        if self.status.is_none() {
            self.status = self.child.try_wait()?;
            if self.status.is_some() {
                self.pidfd = None;
                self.drain()?;
                // A delegate retaining stderr must not hold the feed open.
                self.child.stderr.take();
            }
        }
        Ok(self.status.is_some())
    }

    pub(super) fn take_error(&mut self) -> Option<String> {
        let status = self.status?;
        if std::mem::replace(&mut self.reported, true) || status.success() {
            return None;
        }
        let mut message = String::from_utf8_lossy(&self.diagnostic).trim().to_owned();
        if message.is_empty() {
            message = "Could not refresh file index".into();
        }
        if self.truncated {
            message.push_str("\n[diagnostic truncated]");
        }
        Some(message)
    }

    fn needs_poll(&self) -> bool {
        self.pidfd.is_none()
    }
}

pub(super) struct Watch {
    fd: Option<OwnedFd>,
    names: [Vec<u8>; 3],
}

impl Watch {
    pub(super) fn new(database: &Path) -> Self {
        let name = database.file_name().unwrap().as_bytes();
        let names = [
            name.to_vec(),
            [name, b"-wal"].concat(),
            [name, b"-journal"].concat(),
        ];
        Self {
            fd: Self::open(database).ok(),
            names,
        }
    }

    fn open(database: &Path) -> io::Result<OwnedFd> {
        let path = CString::new(database.parent().unwrap().as_os_str().as_bytes())?;
        let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mask = libc::IN_MODIFY
            | libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO
            | libc::IN_DELETE_SELF
            | libc::IN_MOVE_SELF
            | libc::IN_ONLYDIR;
        if unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(fd)
    }

    pub(super) fn available(&self) -> bool {
        self.fd.is_some()
    }

    pub(super) fn disable(&mut self) {
        self.fd = None;
    }

    fn drain(&mut self) -> bool {
        let Some(fd) = &self.fd else {
            return false;
        };
        let mut changed = false;
        let mut invalid = false;
        let mut buffer = [0u8; 8192];
        for _ in 0..8 {
            let count =
                unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
            if count < 0 {
                match io::Error::last_os_error().kind() {
                    io::ErrorKind::WouldBlock => break,
                    io::ErrorKind::Interrupted => continue,
                    _ => {
                        invalid = true;
                        break;
                    }
                }
            }
            if count == 0 {
                invalid = true;
                break;
            }
            let mut offset = 0;
            while offset + size_of::<libc::inotify_event>() <= count as usize {
                let event = unsafe {
                    std::ptr::read_unaligned(
                        buffer.as_ptr().add(offset).cast::<libc::inotify_event>(),
                    )
                };
                let start = offset + size_of::<libc::inotify_event>();
                let end = start + event.len as usize;
                if end > count as usize {
                    invalid = true;
                    break;
                }
                let name = &buffer[start..end];
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(name.len())];
                changed |=
                    event.mask & libc::IN_Q_OVERFLOW != 0 || self.names.iter().any(|n| n == name);
                invalid |= event.mask
                    & (libc::IN_IGNORED
                        | libc::IN_DELETE_SELF
                        | libc::IN_MOVE_SELF
                        | libc::IN_UNMOUNT)
                    != 0;
                offset = end;
            }
        }
        if invalid {
            self.fd = None;
        }
        changed || invalid
    }

    /// Wait for database writes, child completion/diagnostics, consumer closure,
    /// or the next scheduled refresh. A busy writer gets a bounded retry while
    /// filesystem events accumulate/coalesce in the kernel.
    pub(super) fn wait(
        &mut self,
        output: RawFd,
        refresh: Option<&Refresh>,
        next: Instant,
        writer_busy: bool,
    ) -> io::Result<bool> {
        let fallback = !self.available() || writer_busy || refresh.is_some_and(Refresh::needs_poll);
        let deadline = if fallback {
            next.min(Instant::now() + FALLBACK)
        } else {
            next
        };
        loop {
            let raw = |fd: Option<&OwnedFd>| fd.map_or(-1, AsRawFd::as_raw_fd);
            let mut fds = [
                libc::pollfd {
                    fd: output,
                    events: 0,
                    revents: 0,
                },
                libc::pollfd {
                    fd: if writer_busy {
                        -1
                    } else {
                        raw(self.fd.as_ref())
                    },
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: raw(refresh.and_then(|r| r.pidfd.as_ref())),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: refresh
                        .and_then(|r| r.child.stderr.as_ref())
                        .map_or(-1, AsRawFd::as_raw_fd),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let remaining = deadline.saturating_duration_since(Instant::now());
            let milliseconds = remaining
                .as_millis()
                .saturating_add(u128::from(!remaining.is_zero()))
                .min(i32::MAX as u128) as i32;
            if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, milliseconds) } < 0
            {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if fds[0].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Picker closed"));
            }
            if fds[1].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                self.fd = None;
                return Ok(true);
            }
            return Ok(fds[1].revents & libc::POLLIN != 0 && self.drain());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::net::UnixStream,
        process::{Command, Stdio},
    };

    #[test]
    fn directory_watch_tracks_wal_and_database_replacement_without_other_file_noise() {
        let directory = tempfile::TempDir::new().unwrap();
        let database = directory.path().join("index.sqlite3");
        fs::write(&database, "old").unwrap();
        let mut watch = Watch::new(&database);
        assert!(watch.available());
        let (_reader, writer) = UnixStream::pair().unwrap();
        fs::write(directory.path().join("unrelated"), "not an index").unwrap();
        assert!(
            !watch
                .wait(
                    writer.as_raw_fd(),
                    None,
                    Instant::now() + Duration::from_secs(1),
                    false
                )
                .unwrap()
        );
        fs::write(directory.path().join("index.sqlite3-wal"), "commit").unwrap();
        assert!(
            watch
                .wait(
                    writer.as_raw_fd(),
                    None,
                    Instant::now() + Duration::from_secs(1),
                    false
                )
                .unwrap()
        );
        let replacement = directory.path().join("replacement");
        fs::write(&replacement, "new").unwrap();
        fs::rename(replacement, &database).unwrap();
        assert!(
            watch
                .wait(
                    writer.as_raw_fd(),
                    None,
                    Instant::now() + Duration::from_secs(1),
                    false
                )
                .unwrap()
        );
        assert!(watch.available());
    }

    #[test]
    fn removed_watch_and_unavailable_watch_both_keep_a_polling_fallback() {
        let directory = tempfile::TempDir::new().unwrap();
        let parent = directory.path().join("cache");
        fs::create_dir(&parent).unwrap();
        let mut watch = Watch::new(&parent.join("index.sqlite3"));
        let (_reader, writer) = UnixStream::pair().unwrap();
        fs::rename(&parent, directory.path().join("moved-cache")).unwrap();
        assert!(
            watch
                .wait(
                    writer.as_raw_fd(),
                    None,
                    Instant::now() + Duration::from_secs(1),
                    false
                )
                .unwrap()
        );
        assert!(!watch.available());
        assert!(!Watch::new(&parent.join("index.sqlite3")).available());
        let began = Instant::now();
        assert!(
            !watch
                .wait(
                    writer.as_raw_fd(),
                    None,
                    began + Duration::from_secs(10),
                    false
                )
                .unwrap()
        );
        assert!(began.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn closing_the_consumer_wakes_an_otherwise_idle_feed() {
        let directory = tempfile::TempDir::new().unwrap();
        let mut watch = Watch::new(&directory.path().join("index.sqlite3"));
        let (reader, writer) = UnixStream::pair().unwrap();
        drop(reader);
        let error = watch
            .wait(
                writer.as_raw_fd(),
                None,
                Instant::now() + Duration::from_secs(30),
                false,
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn failed_refresh_drains_large_diagnostics_without_deadlock_and_reports_once() {
        let child = Command::new("sh").args(["-c", "i=0; while [ \"$i\" -lt 4000 ]; do printf 'failed refresh diagnostic: abcdefghijklmnopqrstuvwxyz0123456789\\n' >&2; i=$((i+1)); done; exit 1"])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        let mut refresh = Refresh::new(child).unwrap();
        let directory = tempfile::TempDir::new().unwrap();
        let mut watch = Watch::new(&directory.path().join("index.sqlite3"));
        let (_reader, writer) = UnixStream::pair().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !refresh.finished().unwrap() {
            assert!(
                Instant::now() < deadline,
                "refresh stderr must not fill its pipe"
            );
            watch
                .wait(writer.as_raw_fd(), Some(&refresh), deadline, false)
                .unwrap();
        }
        let error = refresh.take_error().unwrap();
        assert!(error.starts_with("failed refresh diagnostic:"));
        assert!(error.ends_with("[diagnostic truncated]"));
        assert!(error.len() <= DIAGNOSTIC_LIMIT + 32);
        assert!(refresh.take_error().is_none());
    }

    #[test]
    fn unavailable_pidfd_uses_bounded_checks_until_the_child_exits() {
        let child = Command::new("sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut refresh = Refresh::new(child).unwrap();
        refresh.pidfd = None;
        let directory = tempfile::TempDir::new().unwrap();
        let mut watch = Watch::new(&directory.path().join("index.sqlite3"));
        let (_reader, writer) = UnixStream::pair().unwrap();
        let began = Instant::now();
        let result = watch.wait(
            writer.as_raw_fd(),
            Some(&refresh),
            began + Duration::from_secs(5),
            false,
        );
        refresh.child.kill().unwrap();
        refresh.child.wait().unwrap();
        assert!(!result.unwrap());
        assert!(began.elapsed() < Duration::from_secs(1));
    }
}
