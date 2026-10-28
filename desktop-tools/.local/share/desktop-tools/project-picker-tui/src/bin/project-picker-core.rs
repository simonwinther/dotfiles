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

fn open_project(url: &str) -> io::Result<()> {
    let mut command = Command::new("desktopctl");
    command
        .args(["launch", "webapp", url])
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

fn run(args: &[OsString]) -> io::Result<()> {
    match args {
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
        [flag] if flag == "--list" => {
            for project in cache::load(&cache::cache_path()?).projects {
                println!("{}\t{}", project.name, project.url);
            }
            Ok(())
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "Browse projects from a local, browser-maintained snapshot.\n\nUsage: project-picker [--choose | --list]\n  --choose  Run the picker inside the current terminal\n  --list    Print cached projects without opening a window"
            );
            Ok(())
        }
        _ => Err(io::Error::other(
            "Usage: project-picker [--choose | --list]",
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
                &mut io::stdin().lock(),
                &mut io::stdout().lock(),
            )
        });
        std::process::exit(if matches!(result, Ok(true)) { 0 } else { 1 });
    }
    if let Err(error) = run(&args) {
        eprintln!("project-picker: {error}");
        std::process::exit(1);
    }
}
