use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};

pub fn quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&b))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub fn shell_join(values: &[String]) -> String {
    values
        .iter()
        .map(|v| quote(v))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("Could not run {:?}", command.get_program()))?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    if !result.status.success() {
        bail!(
            "{}",
            if stderr.trim().is_empty() {
                format!("Command failed: {:?}", command.get_program())
            } else {
                stderr.trim().to_owned()
            }
        );
    }
    eprint!("{stderr}");
    Ok(String::from_utf8(result.stdout)
        .context("Command output is not UTF-8")?
        .trim()
        .to_owned())
}

pub fn git(root: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(root) = root {
        command.current_dir(root);
    }
    output(&mut command)
}

pub fn git_config(root: &Path, key: &str) -> Option<String> {
    let result = Command::new("git")
        .args(["config", "--local", "--get", key])
        .current_dir(root)
        .output()
        .ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

pub fn repo_root() -> Result<std::path::PathBuf> {
    Path::new(&git(None, &["rev-parse", "--show-toplevel"])?)
        .canonicalize()
        .context("Resolve Git checkout")
}

pub fn status_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
}

pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("Could not run {:?}", command.get_program()))?;
    if !status.success() {
        bail!(
            "Command failed: {:?} (exit {})",
            command.get_program(),
            status_code(status)
        );
    }
    Ok(())
}

// Stream input without buffering an archive/binary in memory. Captured output is
// drained concurrently by wait_with_output, including when the peer exits early.
pub fn with_input(command: &mut Command, mut input: impl std::io::Read + Send) -> Result<Output> {
    command.stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("Could not run {:?}", command.get_program()))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let (result, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            let result = std::io::copy(&mut input, &mut stdin);
            drop(stdin);
            result
        });
        let result = child.wait_with_output();
        (result, writer.join().expect("input writer"))
    });
    let result = result?;
    if result.status.success() {
        written.context("Send command input")?;
    }
    Ok(result)
}

pub fn prepend_path(paths: &[&Path], tail: &OsStr) -> Result<OsString> {
    std::env::join_paths(
        paths
            .iter()
            .map(|p| p.to_path_buf())
            .chain(std::env::split_paths(tail)),
    )
    .context("Construct command search path")
}
