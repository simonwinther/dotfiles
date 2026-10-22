use crate::Result;
use crate::config::Paths;
use crate::wallpaper::set_wallpaper;
use similar::TextDiff;
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SOURCE: &str = "source = ~/.config/background-grid/hyprland.conf";
pub const HELP: &str = "Connect the wallpaper grid to this PC's independent Hyprland configuration.\n\nUsage: background-grid-setup [--dry-run] [--no-reload]\n\n  --dry-run    Show configuration changes without writing files or reloading\n  --no-reload  Prepare settings outside a running Hyprland session\n  -h, --help   Show this help";

pub fn configuration(main: &str, startup: &str) -> (String, String) {
    let main = if main.lines().any(|line| line == SOURCE) {
        main.to_owned()
    } else {
        format!(
            "{}\n\n# Personal wallpaper grid\n{SOURCE}\n",
            main.trim_end()
        )
    };
    let startup = startup
        .split_inclusive('\n')
        .filter(|line| {
            let Some(command) = line
                .strip_prefix("exec-once")
                .and_then(|rest| rest.trim_start_matches([' ', '\t']).strip_prefix('='))
            else {
                return true;
            };
            let words: Vec<_> = command
                .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .filter(|word| !word.is_empty())
                .collect();
            !(words
                .iter()
                .any(|word| matches!(*word, "awww-daemon" | "swww-daemon"))
                || words
                    .windows(2)
                    .any(|pair| pair == ["waypaper", "--restore"]))
        })
        .collect();
    (main, startup)
}

fn available(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| {
            directory.join(name).metadata().is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn checked(command: &mut Command) -> Result<std::process::Output> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            command.get_program().to_string_lossy(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output)
}

fn reload() -> Result<()> {
    checked(Command::new("hyprctl").arg("reload"))?;
    let result = checked(Command::new("hyprctl").arg("configerrors"))?;
    let errors = String::from_utf8_lossy(&result.stdout);
    if !errors.trim().is_empty() {
        return Err(errors.trim().to_owned().into());
    }
    Ok(())
}

fn write_pair(paths: &[&Path; 2], contents: &[String; 2]) -> Result<()> {
    for (path, content) in paths.iter().zip(contents) {
        fs::write(path, content)?;
    }
    Ok(())
}

pub fn run(paths: &Paths, dry_run: bool, no_reload: bool) -> Result<()> {
    if paths.config_home.join("hypr/hyprland.lua").is_file() {
        println!(
            "The wallpaper grid is declared in the shared Lua configuration. Edit the Stow source and rebuild when using NixOS; no configuration was changed."
        );
        return Ok(());
    }
    for name in ["awww", "awww-daemon", "uwsm-app", "hyprctl"] {
        if !available(name) {
            return Err(format!(
                "Missing {name}. Install the wallpaper and desktop dependencies first."
            )
            .into());
        }
    }
    if !paths.home.join(".local/bin/background-grid").is_file()
        || !paths.config.join("hyprland.conf").is_file()
    {
        return Err("Run stow background-grid and background-grid-build first.".into());
    }
    let main = paths.config_home.join("hypr/hyprland.conf");
    let startup = paths.config_home.join("hypr/autostart.conf");
    let files = [main.as_path(), startup.as_path()];
    for file in &files {
        if fs::symlink_metadata(file)?.file_type().is_symlink() {
            return Err(format!(
                "Refusing to edit linked configuration: {}. Edit its Stow or Nix source instead.",
                file.display()
            )
            .into());
        }
    }
    let old = [fs::read_to_string(&main)?, fs::read_to_string(&startup)?];
    let (main, startup) = configuration(&old[0], &old[1]);
    let new = [main, startup];
    for ((path, before), after) in files.iter().zip(&old).zip(&new) {
        print!(
            "{}",
            TextDiff::from_lines(before, after)
                .unified_diff()
                .header(&path.display().to_string(), &path.display().to_string())
        );
    }
    if dry_run {
        return Ok(());
    }
    if !no_reload {
        reload().map_err(|error| format!("Fix existing Hyprland errors first:\n{error}"))?;
    }
    if old != new {
        let state = env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| paths.home.join(".local/state"))
            .join("wallpaper-grid");
        fs::create_dir_all(&state)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let backup = tempfile::Builder::new()
            .prefix(&format!("setup-{stamp}-"))
            .tempdir_in(state)?
            .keep();
        for file in &files {
            fs::copy(
                file,
                backup.join(file.file_name().ok_or("Invalid configuration path")?),
            )?;
        }
        println!("Previous settings saved to {}", backup.display());
        if let Err(error) = write_pair(&files, &new) {
            write_pair(&files, &old)?;
            return Err(error);
        }
    }
    if !no_reload {
        if let Err(error) = reload() {
            write_pair(&files, &old)?;
            let _ = checked(Command::new("hyprctl").arg("reload"));
            return Err(format!("Settings restored after a configuration error:\n{error}").into());
        }
        set_wallpaper(paths, &paths.current(), Some("none"))?;
    }
    println!("Ready. Ctrl + Super + Space opens the wallpaper grid.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_is_idempotent_and_preserves_other_startup_commands() {
        let startup = "exec-once = /usr/bin/awww-daemon --no-cache\nexec-once = waypaper --restore\nexec-once = waypaper --other\nexec-once = terminal\n# exec-once = swww-daemon\n";
        let (main, startup) = configuration("monitor = existing\n", startup);
        assert_eq!(
            startup,
            "exec-once = waypaper --other\nexec-once = terminal\n# exec-once = swww-daemon\n"
        );
        assert!(main.starts_with("monitor = existing\n"));
        assert_eq!(configuration(&main, &startup), (main, startup));
    }
}
