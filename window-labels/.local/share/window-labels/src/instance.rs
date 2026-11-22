//! Single running instance, tracked by an advisory lock in the runtime dir.
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    os::fd::AsRawFd,
    path::PathBuf,
};

fn lock_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("window-labels.lock")
}

/// Returns the held lock when no other instance runs, `None` otherwise.
pub fn acquire() -> io::Result<Option<File>> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path())?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::EWOULDBLOCK) => Ok(None),
            _ => Err(error),
        };
    }
    file.set_len(0)?;
    file.rewind()?;
    write!(file, "{}", std::process::id())?;
    file.flush()?;
    Ok(Some(file))
}

pub fn stop_running() -> io::Result<()> {
    let mut pid = String::new();
    File::open(lock_path())?.read_to_string(&mut pid)?;
    let pid: libc::pid_t = pid
        .trim()
        .parse()
        .map_err(|_| io::Error::other("lock file holds no process id"))?;
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
