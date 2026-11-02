//! Helper commands have bounded output, deadlines and cancellable lifetimes.
use std::{
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Running {
    child: Child,
    reaped: bool,
}
impl Drop for Running {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        // Keep the leader waitable until its group is stopped, so its ID cannot
        // be reused while descendants still hold inherited output pipes.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn drain(
    pipe: &mut impl Read,
    bytes: &mut Vec<u8>,
    remaining: usize,
    done: &mut bool,
) -> io::Result<()> {
    if *done {
        return Ok(());
    }
    let initial = bytes.len();
    let mut chunk = [0; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => {
                *done = true;
                return Ok(());
            }
            Ok(size) => {
                if size > remaining.saturating_sub(bytes.len() - initial) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Helper output exceeds its limit",
                    ));
                }
                bytes.extend_from_slice(&chunk[..size]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
}

fn exited(child: &Child) -> io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.si_pid() } != 0)
}

pub fn output(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
    mut cancelled: impl FnMut() -> bool,
) -> io::Result<Output> {
    if cancelled() {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    unsafe {
        command.pre_exec(|| {
            crate::reset_child_signals();
            Ok(())
        });
    }
    let mut running = Running {
        child: command.spawn()?,
        reaped: false,
    };
    // Readable once the helper exits, so the final wait needs no fixed sleep.
    let exit = match unsafe {
        libc::syscall(libc::SYS_pidfd_open, running.child.id() as libc::pid_t, 0)
    } {
        fd if fd >= 0 => Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) }),
        _ => None,
    };
    let mut stdout = running.child.stdout.take().unwrap();
    let mut stderr = running.child.stderr.take().unwrap();
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let deadline = Instant::now() + timeout;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let (mut out_done, mut err_done) = (false, false);
    loop {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Cancelled"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "Helper timed out"));
        }
        let remaining = limit - out.len() - err.len();
        drain(&mut stdout, &mut out, remaining, &mut out_done)?;
        let remaining = limit - out.len() - err.len();
        drain(&mut stderr, &mut err, remaining, &mut err_done)?;
        if out_done && err_done && exited(&running.child)? {
            // Stop descendants before reaping the leader and releasing its ID.
            unsafe {
                libc::kill(-(running.child.id() as i32), libc::SIGKILL);
            }
            let status = running.child.wait()?;
            running.reaped = true;
            return Ok(Output {
                status,
                stdout: out,
                stderr: err,
            });
        }
        let mut descriptors = [
            libc::pollfd {
                fd: if out_done { -1 } else { stdout.as_raw_fd() },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: if err_done { -1 } else { stderr.as_raw_fd() },
                events: libc::POLLIN,
                revents: 0,
            },
            // Only after both pipes close: an exited leader whose descendants
            // still hold a pipe must not make this loop spin.
            libc::pollfd {
                fd: match &exit {
                    Some(fd) if out_done && err_done => fd.as_raw_fd(),
                    _ => -1,
                },
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let delay = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(40));
        let result = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as _,
                delay.as_millis().max(1) as i32,
            )
        };
        if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(io::Error::last_os_error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn helper(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "process::tests::fixture", "--nocapture"])
            .env("DESKTOP_BAR_TEST_HELPER", mode);
        command
    }
    #[test]
    fn fixture() {
        let Ok(mode) = std::env::var("DESKTOP_BAR_TEST_HELPER") else {
            return;
        };
        if mode == "normal" {
            std::io::stdout().write_all(&vec![b'o'; 96 * 1024]).unwrap();
            std::io::stderr().write_all(&vec![b'e'; 96 * 1024]).unwrap();
            std::process::exit(7);
        }
        if mode == "flood" {
            loop {
                std::io::stdout().write_all(&[b'x'; 8192]).unwrap();
            }
        }
        if mode == "descendant" {
            let child = helper("blocked")
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            if let Some(path) = std::env::var_os("DESKTOP_BAR_TEST_CHILD") {
                std::fs::write(path, child.id().to_string()).unwrap();
            }
            std::process::exit(0);
        }
        if mode == "closed" {
            unsafe {
                libc::close(1);
                libc::close(2);
            }
        }
        std::thread::sleep(Duration::from_secs(20));
    }
    #[test]
    fn captures_both_pipes_and_preserves_failure_status() {
        let result = output(
            &mut helper("normal"),
            Duration::from_secs(3),
            256 * 1024,
            || false,
        )
        .unwrap();
        assert_eq!(result.status.code(), Some(7));
        assert!(result.stdout.windows(1024).any(|part| part == [b'o'; 1024]));
        assert_eq!(result.stderr, vec![b'e'; 96 * 1024]);
    }
    #[test]
    fn deadlines_limits_and_cancellation_release_the_worker() {
        for mode in ["blocked", "closed", "flood"] {
            let started = Instant::now();
            let result = output(
                &mut helper(mode),
                Duration::from_millis(150),
                32 * 1024,
                || false,
            )
            .unwrap_err();
            assert_eq!(
                result.kind(),
                if mode == "flood" {
                    io::ErrorKind::InvalidData
                } else {
                    io::ErrorKind::TimedOut
                }
            );
            assert!(started.elapsed() < Duration::from_secs(3));
        }
        let started = Instant::now();
        let result = output(&mut helper("blocked"), Duration::from_secs(3), 1024, || {
            started.elapsed() > Duration::from_millis(60)
        })
        .unwrap_err();
        assert_eq!(result.kind(), io::ErrorKind::Interrupted);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            output(
                &mut helper("normal"),
                Duration::from_secs(3),
                256 * 1024,
                || false
            )
            .is_ok()
        );
    }
    #[test]
    fn descendants_cannot_keep_a_timed_out_helper_alive() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("child");
        let result = output(
            helper("descendant").env("DESKTOP_BAR_TEST_CHILD", &marker),
            Duration::from_millis(500),
            4096,
            || false,
        )
        .unwrap_err();
        assert_eq!(result.kind(), io::ErrorKind::TimedOut);
        let pid = std::fs::read_to_string(marker).unwrap();
        let stopped = || {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            stat.is_err() || stat.unwrap().rsplit(") ").next().unwrap().starts_with('Z')
        };
        for _ in 0..60 {
            if stopped() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(stopped());
    }
}
