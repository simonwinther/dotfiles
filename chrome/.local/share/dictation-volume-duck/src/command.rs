use anyhow::{Context, Result, bail, ensure};
use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Deadline(Instant);

impl Deadline {
    pub fn new(budget: Duration) -> Self {
        Self(Instant::now() + budget)
    }

    pub fn remaining(&self) -> Result<Duration> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .context("Volume operation timed out; saved state retained")
    }

    pub fn sleep(&self, duration: Duration) -> Result<()> {
        ensure!(duration < self.remaining()?, "Volume operation timed out");
        thread::sleep(duration);
        Ok(())
    }
}

pub fn exists(program: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|p| {
            fs::metadata(p.join(program))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

// Files avoid pipe deadlocks while keeping command timeouts independent of output.
pub fn capture(program: &str, args: &[&str], deadline: &Deadline) -> Result<String> {
    let timeout = deadline.remaining()?.min(Duration::from_millis(750));
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .process_group(0)
        .spawn()
        .with_context(|| format!("Could not start {program}"))?;
    let end = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < end => thread::sleep(Duration::from_millis(5)),
            result => {
                // The child has its own process group, including any descendants.
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.wait();
                if let Err(error) = result {
                    return Err(error).with_context(|| format!("Could not wait for {program}"));
                }
                bail!("{program} timed out; saved state retained");
            }
        }
    };
    let read = |file: &mut fs::File| -> Result<String> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(65_537).read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 65_536, "Excessive output from {program}");
        Ok(String::from_utf8(bytes)?.trim().to_owned())
    };
    let out = read(&mut stdout)?;
    let err = read(&mut stderr)?;
    ensure!(
        status.success(),
        "{program} failed ({}): {}",
        status,
        if err.is_empty() { &out } else { &err }
    );
    Ok(out)
}
