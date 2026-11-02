use project_picker::{cache, native};
use std::{
    env,
    ffi::OsString,
    io,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::PathBuf,
    process::{Command, Stdio},
};

fn executable(path: &std::path::Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn picker_binary() -> io::Result<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_else(|| OsString::from("/bin:/usr/bin"));
    for directory in env::split_paths(&path) {
        let binary = directory.join("project-picker-tui");
        if executable(&binary) {
            return Ok(binary);
        }
    }
    if let Ok(current) = env::current_exe() {
        let binary = current.with_file_name("project-picker-tui");
        if executable(&binary) {
            return Ok(binary);
        }
    }
    let binary = cache::cache_path()?
        .with_file_name("bin")
        .join("project-picker-tui");
    if executable(&binary) {
        return Ok(binary);
    }
    Err(io::Error::other(
        "Rust picker unavailable; install the native package or run project-picker-build",
    ))
}

/// Shows a project in the most recent ChatGPT app window when the browser
/// extension is listening, otherwise starts a new app window directly.
fn open_project(url: &str) -> io::Result<()> {
    if native::request_open(url) {
        focus_project_window();
        return Ok(());
    }
    launch_webapp(url)
}

/// Raises the most recently used ChatGPT app window in Hyprland.
fn focus_project_window() {
    let Ok(output) = Command::new("hyprctl").args(["-j", "clients"]).output() else {
        return;
    };
    let Ok(clients) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) else {
        return;
    };
    let Some(address) = clients
        .iter()
        .filter(|client| {
            client["class"]
                .as_str()
                .is_some_and(|class| class.starts_with("chrome-chatgpt.com"))
        })
        .min_by_key(|client| client["focusHistoryID"].as_i64().unwrap_or(i64::MAX))
        .and_then(|client| client["address"].as_str())
    else {
        return;
    };
    let selector = serde_json::json!(format!("address:{address}"));
    let _ = Command::new("hyprctl")
        .args([
            "eval",
            &format!("hl.dispatch(hl.dsp.focus({{ window = {selector} }}))"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The configured browser, as desktopctl reads it from its settings.
fn browser() -> String {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .and_then(|config| std::fs::read(config.join("desktop/settings.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|settings| settings["browser"].as_str().map(str::to_owned))
        .filter(|browser| !browser.is_empty())
        .unwrap_or_else(|| "chromium".into())
}

fn on_path(name: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| executable(&directory.join(name)))
    })
}

/// Starts the browser in its own application scope, as desktopctl would.
fn launch_webapp(url: &str) -> io::Result<()> {
    if !native::project_url(url) {
        return Err(io::Error::other("Only ChatGPT pages can be opened"));
    }
    let browser = [browser(), format!("--app={url}")];
    let mut command = if on_path("uwsm-app") {
        let mut command = Command::new("uwsm-app");
        command.arg("--").args(&browser);
        command
    } else if on_path("systemd-run") {
        let mut command = Command::new("systemd-run");
        command
            .args(["--user", "--scope", "--collect", "--quiet", "--"])
            .args(&browser);
        command
    } else {
        let mut command = Command::new(&browser[0]);
        command.arg(&browser[1]);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and the child hook neither allocates
    // nor locks. Detach the browser launch from the picker's terminal session.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    command.spawn()?;
    Ok(())
}

fn choose() -> io::Result<()> {
    let cache = cache::cache_path()?;
    let result = Command::new(picker_binary()?)
        .arg("--cache")
        .arg(&cache)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()?;
    if result.status.code() == Some(130) {
        return Ok(());
    }
    if !result.status.success() {
        return Err(io::Error::other("Project search failed"));
    }
    let selected = std::str::from_utf8(&result.stdout)
        .map_err(|_| io::Error::other("Invalid project selection"))?
        .trim();
    if selected.is_empty() {
        return Ok(());
    }
    // Re-read with the same schema after the TUI exits: a removed project or
    // changed workspace must not launch from an old selection.
    let view = cache::load(&cache);
    let project = view
        .projects
        .iter()
        .find(|project| project.id == selected)
        .ok_or_else(|| {
            io::Error::other("This project is no longer available; reopen the picker")
        })?;
    open_project(&project.url)
}

/// Whether the selected bar style is the notch, read from the theme that
/// desktopctl publishes for the bar.
fn notch_style() -> bool {
    let Some(runtime) = env::var_os("XDG_RUNTIME_DIR") else {
        return false;
    };
    std::fs::read(PathBuf::from(runtime).join("desktop/bar-theme.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|theme| theme["style"] == "native-motion")
}

/// Opens the native Projects page in a running notch bar.
fn open_in_notch() -> bool {
    Command::new("desktop-bar")
        .args(["--panel", "projects"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Projects, sync notice and last-sync line for the bar's Projects page.
fn catalog() -> io::Result<()> {
    let view = cache::load(&cache::cache_path()?);
    let value = serde_json::json!({
        "projects": view.projects,
        "notice": view.notice,
        "last_synced": view.last_synced,
    });
    println!("{value}");
    Ok(())
}

fn run(args: &[OsString]) -> io::Result<()> {
    match args {
        [] if notch_style() && open_in_notch() => Ok(()),
        [] => Err(Command::new("desktopctl")
            .args([
                "terminal",
                "--float",
                "--app-id",
                "project.picker",
                "project-picker",
                "--choose",
            ])
            .exec()),
        [flag] if flag == "--choose" => choose(),
        [flag] if flag == "--catalog" => catalog(),
        [flag, url] if flag == "--open" => open_project(
            url.to_str()
                .ok_or_else(|| io::Error::other("Invalid project URL"))?,
        ),
        [flag] if flag == "--list" => {
            for project in cache::load(&cache::cache_path()?).projects {
                println!("{}\t{}", project.name, project.url);
            }
            Ok(())
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "Browse projects from a local, browser-maintained snapshot.\n\nUsage: project-picker [--choose | --list | --catalog | --open URL]\n  --choose   Run the picker inside the current terminal\n  --list     Print cached projects without opening a window\n  --catalog  Print projects and sync status as JSON for the notch bar\n  --open     Show a ChatGPT page in the open app window, or start one\n\nWith the notch bar style, project-picker opens its Projects page."
            );
            Ok(())
        }
        _ => Err(io::Error::other(
            "Usage: project-picker [--choose | --list | --catalog | --open URL]",
        )),
    }
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--native-host") {
        let result = cache::cache_path().and_then(|path| {
            let origin = if args.len() == 2 {
                Some(args[1].as_os_str())
            } else {
                None
            };
            native::serve(
                origin,
                &path,
                // Unlocked: a listening host reads stdin on another thread.
                &mut io::stdin(),
                &mut io::stdout(),
            )
        });
        std::process::exit(if matches!(result, Ok(true)) { 0 } else { 1 });
    }
    if let Err(error) = run(&args) {
        eprintln!("project-picker: {error}");
        std::process::exit(1);
    }
}
