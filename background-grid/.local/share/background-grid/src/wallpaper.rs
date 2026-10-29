use crate::Result;
use crate::catalog::{catalog, next_image};
use crate::config::Paths;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
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

fn lock(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().ok_or("Invalid lock path")?)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(file);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
}

fn read_state(paths: &Paths) -> Result<Value> {
    let file = paths.state().join("state.json");
    if file.is_symlink() {
        return Err("Refusing to overwrite linked desktop state".into());
    }
    let state: Value = match fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(error) => return Err(error.into()),
    };
    if !state.is_object() {
        return Err("Desktop state must be an object".into());
    }
    if let Some(wallpaper) = state.get("wallpaper")
        && !wallpaper.is_string()
    {
        return Err("Wallpaper path must be a string".into());
    }
    if let Some(wallpapers) = state.get("wallpapers") {
        let values = wallpapers
            .as_object()
            .ok_or("Wallpaper state must be an object")?;
        if values.values().any(|value| !value.is_string()) {
            return Err("Wallpaper paths must be strings".into());
        }
    }
    Ok(state)
}

fn default_image(paths: &Paths, state: &Value) -> Result<Option<PathBuf>> {
    // The wildcard is authoritative for legacy state whose last per-output
    // selection also overwrote `wallpaper` and the background link.
    if let Some(image) = state
        .get("wallpapers")
        .and_then(|value| value.get("*"))
        .or_else(|| state.get("wallpaper"))
        .and_then(Value::as_str)
    {
        return Ok(Some(paths.expand(Path::new(image))));
    }
    match fs::read_link(paths.current()) {
        Ok(image) => return Ok(Some(paths.state().join(image))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    Ok(paths
        .desktop_settings()?
        .get("defaultWallpaper")
        .and_then(Value::as_str)
        .map(|image| paths.expand(Path::new(image))))
}

pub fn current_wallpaper(paths: &Paths) -> Result<Option<PathBuf>> {
    default_image(paths, &read_state(paths)?)
}

fn publish_link(current: &Path, image: &Path) -> Result<()> {
    guard_current(current)?;
    let temporary = tempfile::Builder::new()
        .prefix(".background-grid-")
        .tempdir_in(current.parent().ok_or("Invalid selected wallpaper path")?)?;
    let link = temporary.path().join("background");
    symlink(image, &link)?;
    fs::rename(link, current)?;
    Ok(())
}

fn publish_selection(paths: &Paths, image: &Path, output: Option<&str>) -> Result<()> {
    // Always take wallpaper.lock before state.lock. Re-read here so unrelated
    // theme/font updates during rendering are preserved.
    let _state_lock = lock(&paths.state().join("state.lock"))?;
    let mut state = read_state(paths)?;
    let values = state
        .as_object_mut()
        .ok_or("Desktop state must be an object")?;
    if let Some(output) = output {
        values
            .entry("wallpapers")
            .or_insert(serde_json::json!({}))
            .as_object_mut()
            .ok_or("Wallpaper state must be an object")?
            .insert(output.into(), serde_json::json!(image));
    } else {
        values.insert("wallpaper".into(), serde_json::json!(image));
        values.insert("wallpapers".into(), serde_json::json!({"*": image}));
    }
    let temporary = tempfile::NamedTempFile::new_in(paths.state())?;
    fs::write(temporary.path(), serde_json::to_vec_pretty(&state)?)?;
    if output.is_none() {
        publish_link(&paths.current(), image)?;
    }
    temporary.persist(paths.state().join("state.json"))?;
    Ok(())
}

fn animation(paths: &Paths) -> Result<Animation> {
    let animation = match fs::read(paths.config.join("transition.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Animation {
            kind: "any".into(),
            duration: 2.0,
            fps: 60,
            step: 80,
        },
        Err(error) => return Err(error.into()),
    };
    if !animation.duration.is_finite()
        || animation.duration < 0.0
        || animation.fps == 0
        || animation.step == 0
    {
        return Err("Invalid wallpaper transition settings".into());
    }
    Ok(animation)
}

fn start_renderer() -> Result<bool> {
    if renderer_running()? {
        return Ok(false);
    }
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
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Err("The wallpaper renderer did not start.".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn render(image: &Path, output: Option<&str>, animation: Option<&Animation>) -> Result<()> {
    let mut command = Command::new("awww");
    command.args([
        "img",
        "--resize",
        "crop",
        "--transition-type",
        animation.map_or("none", |animation| &animation.kind),
    ]);
    if let Some(animation) = animation {
        command.args([
            "--transition-duration",
            &animation.duration.to_string(),
            "--transition-fps",
            &animation.fps.to_string(),
            "--transition-step",
            &animation.step.to_string(),
        ]);
    }
    if let Some(output) = output {
        command.args(["--outputs", output]);
    }
    checked(command.arg(image), Duration::from_secs(15))?;
    Ok(())
}

fn confirm_surface() -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if wallpaper_ready()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("The wallpaper renderer has no visible surface.".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn stop_fallback() {
    let _ = output(
        Command::new("pkill").args(["-u", &unsafe { libc::getuid() }.to_string(), "-x", "swaybg"]),
        Duration::from_secs(2),
    );
}

fn restore_plan(paths: &Paths, state: &Value) -> Result<Vec<(Option<String>, PathBuf)>> {
    let mut images = Vec::new();
    if let Some(image) = default_image(paths, state)?
        && let Ok(image) = fs::canonicalize(image)
        && image.is_file()
    {
        images.push((None, image));
    }
    if let Some(wallpapers) = state.get("wallpapers").and_then(Value::as_object) {
        for (output, saved) in wallpapers {
            if output != "*"
                && let Some(saved) = saved.as_str()
                && let Ok(image) = fs::canonicalize(paths.expand(Path::new(saved)))
                && image.is_file()
            {
                images.push((Some(output.clone()), image));
            }
        }
    }
    Ok(images)
}

fn apply_selection(paths: &Paths, image: &Path, output: Option<&str>) -> Result<()> {
    if output.is_some_and(|output| output.is_empty() || output == "*") {
        return Err("Specify an output name; omit OUTPUT to select all outputs".into());
    }
    let image = fs::canonicalize(paths.expand(image))
        .map_err(|error| format!("This image is no longer available: {error}"))?;
    if !image.is_file() {
        return Err("This image is no longer available.".into());
    }
    let state = read_state(paths)?;
    guard_current(&paths.current())?;
    let animation = animation(paths)?;
    if start_renderer()? {
        // A newly started renderer has no cache. Recover all outputs before
        // applying a transition or a new per-output override.
        for (output, image) in restore_plan(paths, &state)? {
            render(&image, output.as_deref(), None)?;
        }
    }
    render(&image, output, Some(&animation))?;
    confirm_surface()?;
    publish_selection(paths, &image, output)?;
    stop_fallback();
    Ok(())
}

pub fn set_wallpaper(paths: &Paths, image: &Path, output: Option<&str>) -> Result<()> {
    let _wallpaper_lock = lock(&paths.state().join("wallpaper.lock"))?;
    apply_selection(paths, image, output)
}

pub fn next_wallpaper(paths: &Paths) -> Result<()> {
    // Selection must happen under the same lock as rendering and publication:
    // concurrent --next requests each advance from the last completed choice.
    let _wallpaper_lock = lock(&paths.state().join("wallpaper.lock"))?;
    guard_current(&paths.current())?;
    let images = catalog(&paths.folders()?)?.all_images();
    let current = current_wallpaper(paths)?
        .and_then(|path| path.canonicalize().ok())
        .unwrap_or_default();
    let next = next_image(&images, &current).ok_or("No wallpapers found")?;
    apply_selection(paths, &next, None)
}

pub fn restore_wallpapers(paths: &Paths) -> Result<()> {
    let _wallpaper_lock = lock(&paths.state().join("wallpaper.lock"))?;
    let state = read_state(paths)?;
    guard_current(&paths.current())?;
    let images = restore_plan(paths, &state)?;
    if images.is_empty() {
        return Err("No saved wallpaper is available.".into());
    }
    start_renderer()?;
    for (output, image) in &images {
        render(image, output.as_deref(), None)?;
    }
    confirm_surface()?;
    if let Some((None, image)) = images.first() {
        // Repair a stale legacy link without turning restoration into a new
        // selection or dropping disconnected/missing output overrides.
        publish_link(&paths.current(), image)?;
    }
    stop_fallback();
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
