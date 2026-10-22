use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Output};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Desktop {
    _temporary: tempfile::TempDir,
    home: PathBuf,
    config: PathBuf,
    tools: PathBuf,
    images: PathBuf,
    current: PathBuf,
    log: PathBuf,
}

impl Desktop {
    fn new() -> Result<Self> {
        let temporary = tempfile::tempdir()?;
        let home = temporary.path().join("home");
        let config = home.join(".config/background-grid");
        let tools = home.join("tools");
        let images = home.join("wallpapers with spaces");
        let current = home.join(".local/state/desktop/background");
        let log = home.join("commands.log");
        for path in [&config, &tools, &images, current.parent().unwrap()] {
            fs::create_dir_all(path)?;
        }
        fs::write(
            config.join("folders.json"),
            serde_json::to_vec(&vec![&images])?,
        )?;
        fs::write(
            config.join("transition.json"),
            r#"{"type":"any","duration":2.0,"fps":60,"step":80}"#,
        )?;
        fs::write(images.join("a.png"), b"original A")?;
        fs::write(images.join("b.png"), b"original B")?;
        symlink(images.join("a.png"), &current)?;
        let scripts = [
            (
                "awww",
                r#"#!/bin/sh
printf 'awww\n' >> "$BG_TEST_LOG"
printf '%s\n' "$@" >> "$BG_TEST_LOG"
if [ "$1" = query ] && [ "${BG_TEST_START:-}" = 1 ]; then
  test -f "$BG_TEST_STARTED"
elif [ "$1" = img ] && [ "${BG_TEST_FAIL:-}" = 1 ]; then
  echo 'renderer refused image' >&2
  exit 1
fi
"#,
            ),
            ("pgrep", "#!/bin/sh\necho 42\n"),
            ("awww-daemon", "#!/bin/sh\nexit 0\n"),
            (
                "hyprctl",
                r#"#!/bin/sh
if [ "$1" = reload ]; then
  echo reload >> "$BG_TEST_RELOADS"
elif [ "$1" = configerrors ]; then
  if [ "${BG_TEST_BAD_CONFIG:-}" = 1 ] && [ "$(wc -l < "$BG_TEST_RELOADS")" -ge 2 ]; then
    echo 'test configuration error'
  fi
elif [ "${BG_TEST_NO_SURFACE:-}" = 1 ]; then
  echo '{}'
else
  echo '{"eDP-1":{"levels":{"0":[{"namespace":"awww-daemon","pid":42}]}}}'
fi
"#,
            ),
            (
                "pkill",
                "#!/bin/sh\necho 'stop swaybg' >> \"$BG_TEST_LOG\"\n",
            ),
            (
                "uwsm-app",
                "#!/bin/sh\necho 'start renderer' >> \"$BG_TEST_LOG\"\ntouch \"$BG_TEST_STARTED\"\n",
            ),
        ];
        for (name, script) in scripts {
            let path = tools.join(name);
            fs::write(&path, script)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        }
        Ok(Self {
            _temporary: temporary,
            home,
            config,
            tools,
            images,
            current,
            log,
        })
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_background-grid-native"));
        command
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("BACKGROUND_GRID_CONFIG_DIR", &self.config)
            .env("PATH", format!("{}:/usr/bin:/bin", self.tools.display()))
            .env("BG_TEST_LOG", &self.log)
            .env("BG_TEST_RELOADS", self.home.join("reloads.log"))
            .env("BG_TEST_STARTED", self.home.join("renderer-started"));
        command
    }

    fn success(&self, args: &[&str]) -> Result<Output> {
        let output = self.command().args(args).output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(output)
    }

    fn setup_files(&self) -> Result<(PathBuf, PathBuf)> {
        let main = self.home.join(".config/hypr/hyprland.conf");
        let startup = self.home.join(".config/hypr/autostart.conf");
        fs::create_dir_all(main.parent().unwrap())?;
        fs::create_dir_all(self.home.join(".local/bin"))?;
        fs::write(self.home.join(".local/bin/background-grid"), "placeholder")?;
        fs::write(self.config.join("hyprland.conf"), "fixture")?;
        fs::write(&main, "monitor = keep-existing\n")?;
        fs::write(&startup, "exec-once = awww-daemon\nexec-once = terminal\n")?;
        Ok((main, startup))
    }
}

#[test]
fn setup_dry_run_does_not_write_files_or_reload_the_desktop() -> Result<()> {
    let desktop = Desktop::new()?;
    let (main, startup) = desktop.setup_files()?;
    let output = desktop.success(&["--setup", "--dry-run"])?;
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("+source = ~/.config/background-grid/hyprland.conf")
    );
    assert_eq!(fs::read_to_string(main)?, "monitor = keep-existing\n");
    assert_eq!(
        fs::read_to_string(startup)?,
        "exec-once = awww-daemon\nexec-once = terminal\n"
    );
    assert!(!desktop.home.join(".local/state/wallpaper-grid").exists());
    assert!(!desktop.home.join("reloads.log").exists());
    assert!(!desktop.log.exists());
    Ok(())
}

#[test]
fn setup_without_reload_backs_up_changed_files_and_is_idempotent() -> Result<()> {
    let desktop = Desktop::new()?;
    let (main, startup) = desktop.setup_files()?;
    desktop.success(&["--setup", "--no-reload"])?;
    assert!(fs::read_to_string(&main)?.contains("monitor = keep-existing"));
    assert!(
        fs::read_to_string(&main)?.contains("source = ~/.config/background-grid/hyprland.conf")
    );
    assert_eq!(fs::read_to_string(&startup)?, "exec-once = terminal\n");
    let state = desktop.home.join(".local/state/wallpaper-grid");
    let backup = fs::read_dir(&state)?.next().unwrap()?.path();
    assert_eq!(
        fs::read_to_string(backup.join("hyprland.conf"))?,
        "monitor = keep-existing\n"
    );
    desktop.success(&["--setup", "--no-reload"])?;
    assert_eq!(fs::read_dir(state)?.count(), 1);
    assert!(!desktop.home.join("reloads.log").exists());
    assert!(!desktop.log.exists());
    Ok(())
}

#[test]
fn setup_restores_both_files_after_a_configuration_error() -> Result<()> {
    let desktop = Desktop::new()?;
    let (main, startup) = desktop.setup_files()?;
    let output = desktop
        .command()
        .args(["--setup"])
        .env("BG_TEST_BAD_CONFIG", "1")
        .output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Settings restored"));
    assert_eq!(fs::read_to_string(main)?, "monitor = keep-existing\n");
    assert_eq!(
        fs::read_to_string(startup)?,
        "exec-once = awww-daemon\nexec-once = terminal\n"
    );
    assert!(!desktop.log.exists());
    Ok(())
}

#[test]
fn setup_refuses_symlinks_into_linked_configuration() -> Result<()> {
    let desktop = Desktop::new()?;
    let (main, _) = desktop.setup_files()?;
    let source = desktop
        .home
        .join(".local/share/desktop-managed/hyprland.conf");
    fs::create_dir_all(source.parent().unwrap())?;
    fs::write(&source, "managed content\n")?;
    fs::remove_file(&main)?;
    symlink(&source, &main)?;
    let output = desktop.command().args(["--setup", "--dry-run"]).output()?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Refusing to edit linked configuration")
    );
    assert_eq!(fs::read_to_string(source)?, "managed content\n");
    Ok(())
}

#[test]
fn set_handles_spaces_and_leaves_originals_untouched() -> Result<()> {
    let desktop = Desktop::new()?;
    let image = desktop.images.join("b.png");
    let state = desktop.current.parent().unwrap().join("state.json");
    fs::write(
        &state,
        r#"{"theme":"tokyoled","wallpapers":{"HDMI-A-1":"/keep/image.png"}}"#,
    )?;
    desktop.success(&["--set", image.to_str().unwrap()])?;
    assert_eq!(fs::read_link(&desktop.current)?, image);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(state)?)?;
    assert_eq!(saved["theme"], "tokyoled");
    assert_eq!(saved["wallpaper"], image.to_str().unwrap());
    assert_eq!(saved["wallpapers"]["*"], image.to_str().unwrap());
    assert_eq!(saved["wallpapers"]["HDMI-A-1"], "/keep/image.png");
    assert_eq!(fs::read(&image)?, b"original B");
    let log = fs::read_to_string(&desktop.log)?;
    assert!(log.contains("--transition-type\nany\n"));
    assert!(log.ends_with("stop swaybg\n"));
    Ok(())
}

#[test]
fn restore_uses_no_transition_and_next_wraps_the_library() -> Result<()> {
    let desktop = Desktop::new()?;
    desktop.success(&["--restore"])?;
    assert!(fs::read_to_string(&desktop.log)?.contains("--transition-type\nnone\n"));
    desktop.success(&["--next"])?;
    assert_eq!(
        fs::read_link(&desktop.current)?,
        desktop.images.join("b.png")
    );
    desktop.success(&["--next"])?;
    assert_eq!(
        fs::read_link(&desktop.current)?,
        desktop.images.join("a.png")
    );
    Ok(())
}

#[test]
fn refuses_to_replace_a_regular_selected_file_before_calling_renderer() -> Result<()> {
    let desktop = Desktop::new()?;
    fs::remove_file(&desktop.current)?;
    fs::write(&desktop.current, b"keep this image")?;
    let output = desktop.command().arg("--next").output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to replace"));
    assert_eq!(fs::read(&desktop.current)?, b"keep this image");
    assert!(!desktop.log.exists());
    Ok(())
}

#[test]
fn failed_renderer_keeps_previous_selection_and_swaybg() -> Result<()> {
    let desktop = Desktop::new()?;
    let output = desktop
        .command()
        .env("BG_TEST_FAIL", "1")
        .arg("--next")
        .output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("renderer refused image"));
    assert_eq!(
        fs::read_link(&desktop.current)?,
        desktop.images.join("a.png")
    );
    assert!(!fs::read_to_string(&desktop.log)?.contains("stop swaybg"));
    Ok(())
}

#[test]
fn missing_surface_keeps_previous_selection_and_swaybg() -> Result<()> {
    let desktop = Desktop::new()?;
    let output = desktop
        .command()
        .env("BG_TEST_NO_SURFACE", "1")
        .arg("--next")
        .output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no visible surface"));
    assert_eq!(
        fs::read_link(&desktop.current)?,
        desktop.images.join("a.png")
    );
    assert!(!fs::read_to_string(&desktop.log)?.contains("stop swaybg"));
    Ok(())
}

#[test]
fn starts_renderer_and_bootstraps_previous_image_before_transition() -> Result<()> {
    let desktop = Desktop::new()?;
    let output = desktop
        .command()
        .env("BG_TEST_START", "1")
        .arg("--next")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_to_string(&desktop.log)?;
    let bootstrap = log.find("img\n--transition-type\nnone\n").unwrap();
    let transition = log.find("img\n--resize\ncrop\n").unwrap();
    assert!(log.find("start renderer").unwrap() < bootstrap && bootstrap < transition);
    assert_eq!(
        fs::read_link(&desktop.current)?,
        desktop.images.join("b.png")
    );
    Ok(())
}

#[test]
fn help_and_listing_work_without_a_display_or_wallpaper_changes() -> Result<()> {
    let desktop = Desktop::new()?;
    let help = desktop.success(&["--help"])?;
    assert!(String::from_utf8_lossy(&help.stdout).contains("--restore"));
    let list = desktop.success(&["--list"])?;
    let expected = format!(
        "{}\n{}\n",
        desktop.images.join("a.png").display(),
        desktop.images.join("b.png").display()
    );
    assert_eq!(String::from_utf8(list.stdout)?, expected);
    assert!(!desktop.log.exists());
    assert_eq!(
        fs::read_link(&desktop.current)?,
        desktop.images.join("a.png")
    );
    Ok(())
}
