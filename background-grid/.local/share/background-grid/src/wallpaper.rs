use crate::Result;
use crate::config::Paths;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Animation {
    #[serde(rename = "type")]
    kind: String,
    duration: f64,
    fps: u8,
    step: u8,
}

fn output(command: &mut Command, timeout: Duration) -> Result<Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take().ok_or("Missing command output")?;
    let mut stderr = child.stderr.take().ok_or("Missing command error output")?;
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + timeout;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = out.join().map_err(|_| "Command output reader stopped")??;
    let stderr = err.join().map_err(|_| "Command error reader stopped")??;
    if timed_out {
        return Err(format!("{program} did not respond in time").into());
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn checked(command: &mut Command, timeout: Duration) -> Result<Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    let result = output(command, timeout)?;
    if !result.status.success() {
        let message = if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        };
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(message).trim()
        )
        .into());
    }
    Ok(result)
}

fn renderer_running() -> Result<bool> {
    Ok(
        output(Command::new("awww").arg("query"), Duration::from_secs(2))?
            .status
            .success(),
    )
}

pub fn layers_ready(layers: &Value, pids: &HashSet<i64>) -> bool {
    layers.as_object().is_some_and(|monitors| {
        monitors.values().any(|monitor| {
            monitor
                .get("levels")
                .and_then(Value::as_object)
                .is_some_and(|levels| {
                    levels
                        .values()
                        .filter_map(Value::as_array)
                        .flatten()
                        .any(|layer| {
                            layer.get("namespace").and_then(Value::as_str) == Some("awww-daemon")
                                && layer
                                    .get("pid")
                                    .and_then(Value::as_i64)
                                    .is_some_and(|pid| pids.contains(&pid))
                        })
                })
        })
    })
}

fn wallpaper_ready() -> Result<bool> {
    // A successful renderer query is not enough: verify the surface belongs to it.
    let uid = unsafe { libc::getuid() }.to_string();
    let result = output(
        Command::new("pgrep").args(["-u", &uid, "-x", "awww-daemon"]),
        Duration::from_secs(2),
    )?;
    let pids: HashSet<i64> = String::from_utf8_lossy(&result.stdout)
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .collect();
    let layers = checked(
        Command::new("hyprctl").args(["-j", "layers"]),
        Duration::from_secs(2),
    )?;
    Ok(layers_ready(
        &serde_json::from_slice(&layers.stdout)?,
        &pids,
    ))
}

fn guard_current(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_symlink() => {
            Err("The current background must be a symlink; refusing to replace an image.".into())
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

fn publish_selection(current: &Path, image: &Path) -> Result<()> {
    let parent = current.parent().ok_or("Invalid selected wallpaper path")?;
    fs::create_dir_all(parent)?;
    // Share the controller's lock and JSON state so theme changes and per-output
    // wallpaper choices survive either interface being used.
    let state_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join("state.lock"))?;
    loop {
        if unsafe { libc::flock(state_lock.as_raw_fd(), libc::LOCK_EX) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
    let state_file = parent.join("state.json");
    let mut state: Value = match fs::read(&state_file) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error.into()),
    };
    let values = state
        .as_object_mut()
        .ok_or("Desktop state must be an object")?;
    values.insert("wallpaper".into(), serde_json::json!(image));
    let wallpapers = values.entry("wallpapers").or_insert(serde_json::json!({}));
    wallpapers
        .as_object_mut()
        .ok_or("Wallpaper state must be an object")?
        .insert("*".into(), serde_json::json!(image));
    let state_temporary = tempfile::NamedTempFile::new_in(parent)?;
    fs::write(state_temporary.path(), serde_json::to_vec_pretty(&state)?)?;
    guard_current(current)?;
    let temporary = tempfile::Builder::new()
        .prefix(".background-grid-")
        .tempdir_in(parent)?;
    let link = temporary.path().join("background");
    symlink(image, &link)?;
    fs::rename(link, current)?;
    state_temporary.persist(&state_file)?;
    Ok(())
}

pub fn set_wallpaper(paths: &Paths, image: &Path, transition: Option<&str>) -> Result<()> {
    let image = fs::canonicalize(paths.expand(image))
        .map_err(|error| format!("This image is no longer available: {error}"))?;
    if !image.is_file() {
        return Err("This image is no longer available.".into());
    }
    let animation: Animation =
        serde_json::from_slice(&fs::read(paths.config.join("transition.json"))?)?;
    if !animation.duration.is_finite()
        || animation.duration < 0.0
        || animation.fps == 0
        || animation.step == 0
    {
        return Err("Invalid wallpaper transition settings".into());
    }
    fs::create_dir_all(&paths.cache)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(paths.cache.join("apply.lock"))?;
    loop {
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
    let current = paths.current();
    guard_current(&current)?;
    if !renderer_running()? {
        let mut child = Command::new("uwsm-app")
            .args(["--", "awww-daemon", "--no-cache"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            if renderer_running()? {
                break;
            }
            if Instant::now() >= deadline {
                return Err("The wallpaper renderer did not start.".into());
            }
            thread::sleep(Duration::from_millis(50));
        }
        if let Ok(previous) = fs::canonicalize(&current)
            && previous.is_file()
        {
            checked(
                Command::new("awww")
                    .args(["img", "--transition-type", "none"])
                    .arg(previous),
                Duration::from_secs(15),
            )?;
        }
    }
    checked(
        Command::new("awww")
            .args([
                "img",
                "--resize",
                "crop",
                "--transition-type",
                transition.unwrap_or(&animation.kind),
                "--transition-duration",
                &animation.duration.to_string(),
                "--transition-fps",
                &animation.fps.to_string(),
                "--transition-step",
                &animation.step.to_string(),
            ])
            .arg(&image),
        Duration::from_secs(15),
    )?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if wallpaper_ready()? {
            break;
        }
        if Instant::now() >= deadline {
            return Err("The wallpaper renderer has no visible surface.".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    // Publish the new selection only after the compositor confirms it is visible.
    publish_selection(&current, &image)?;
    let _ = output(
        Command::new("pkill").args(["-u", &unsafe { libc::getuid() }.to_string(), "-x", "swaybg"]),
        Duration::from_secs(2),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_surface_must_match_namespace_and_owner() {
        let layers = serde_json::json!({"eDP-1": {"levels": {"0": [{"namespace": "awww-daemon", "pid": 42}]}}});
        assert!(layers_ready(&layers, &HashSet::from([42])));
        assert!(!layers_ready(&layers, &HashSet::from([43])));
        assert!(!layers_ready(
            &serde_json::json!({"eDP-1": {"levels": {"0": [{"namespace": "swaybg", "pid": 42}]}}}),
            &HashSet::from([42])
        ));
    }
}
