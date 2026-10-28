use anyhow::{Context, Result, bail, ensure};
use signal_hook::{consts::signal::*, flag};
use std::{
    io::Write,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub struct Cancellation(Arc<AtomicUsize>);

impl Cancellation {
    pub fn install() -> Result<Self> {
        let signal = Arc::new(AtomicUsize::new(0));
        for number in [SIGINT, SIGTERM, SIGHUP] {
            flag::register_usize(number, signal.clone(), number as usize)?;
        }
        Ok(Self(signal))
    }

    pub fn signal(&self) -> i32 {
        self.0.load(Ordering::Relaxed) as i32
    }

    pub fn check(&self) -> Result<()> {
        ensure!(self.signal() == 0, "Transcoding cancelled");
        Ok(())
    }

    pub fn run(
        &self,
        command: &mut Command,
        input: Option<&[u8]>,
        timeout: Option<Duration>,
    ) -> Result<()> {
        self.check()?;
        // A separate group lets cancellation reach encoder delegates as well as
        // the direct child, even when only this supervisor receives the signal.
        command.process_group(0).stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let program = command.get_program().to_string_lossy().into_owned();
        let child = command
            .spawn()
            .with_context(|| format!("Could not start {program}"))?;
        let mut job = Job {
            child,
            active: true,
        };
        if let Some(input) = input {
            job.child
                .stdin
                .take()
                .context("Missing child stdin")?
                .write_all(input)?;
        }
        let started = Instant::now();
        loop {
            self.check()?;
            if let Some(status) = job.child.try_wait()? {
                // Leave failure cleanup armed to stop any remaining delegates.
                ensure!(status.success(), "{program} failed ({status})");
                job.active = false;
                return self.check();
            }
            if timeout.is_some_and(|timeout| started.elapsed() >= timeout) {
                bail!("{program} timed out");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

struct Job {
    child: Child,
    active: bool,
}

impl Job {
    fn signal_group(&self, signal: i32) {
        // SAFETY: the child created its own group with PGID equal to its PID.
        unsafe {
            libc::kill(-(self.child.id() as i32), signal);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.signal_group(SIGTERM);
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        // Delegates may have ignored TERM or outlived the direct child. Kill
        // the whole group before the caller removes the temporary directory.
        self.signal_group(SIGKILL);
        let _ = self.child.wait();
    }
}
