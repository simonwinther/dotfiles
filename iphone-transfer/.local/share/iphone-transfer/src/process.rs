use anyhow::{Context, Result, bail, ensure};
use std::{
    io::{Read, Seek, SeekFrom},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

pub fn capture(command: &mut Command, timeout: Option<Duration>) -> Result<Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .with_context(|| format!("Could not start {program}"))?;
    let status = if let Some(timeout) = timeout {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("{program} did not respond in time");
            }
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        child.wait()?
    };
    let read = |file: &mut std::fs::File| -> Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_OUTPUT,
            "Excessive output from {program}"
        );
        Ok(bytes)
    };
    Ok(Output {
        status,
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
    })
}

pub fn detail(output: &Output) -> String {
    let bytes = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let bytes = &bytes[bytes.len().saturating_sub(2000)..];
    String::from_utf8_lossy(bytes).trim().to_owned()
}
