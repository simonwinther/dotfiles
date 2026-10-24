use std::collections::HashSet;
use std::env;
use std::ffi::{CString, OsStr, OsString};
use std::fs;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub mod index;
mod tui;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const EXCLUDES: &[&str] = &[
    ".git",
    ".cache",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "target",
    "build",
    ".cargo",
    ".rustup",
    "go/pkg/mod",
    "Android/Sdk",
    "/R",
    "/VirtualBox VMs",
    ".local/share/flatpak",
    ".local/share/pnpm",
    ".local/share/Steam",
    ".local/share/mise",
    ".local/share/uv",
    ".local/share/Trash",
    ".var/app",
];
const VIRTUAL: &[&str] = &["/proc", "/sys", "/dev", "/run"];

pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME must be set")
}

pub fn data_dir() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
}

pub fn config_dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path == Path::new("~") {
        home()
    } else if let Ok(relative) = path.strip_prefix("~/") {
        home().join(relative)
    } else {
        path.to_owned()
    };
    Ok(if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Pick,
    List,
    Copy(Vec<PathBuf>),
    Path(Vec<PathBuf>),
    Drag(Vec<PathBuf>),
    LatestDownload,
    Window,
    PrepareTerminal,
    CachedList,
    IndexUpdate,
    IndexUpdateIfIdle,
    IndexStatus,
    IndexRemove(Vec<PathBuf>),
    IndexClear,
    IndexStream,
    Help,
    Version,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub root: PathBuf,
    pub all: bool,
    pub pdf: bool,
    pub recent: bool,
    pub sort: SortOrder,
    pub action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Recent,
    Name,
    None,
}

impl SortOrder {
    fn argument(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Name => "name",
            Self::None => "none",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Recent => "newest",
            Self::Name => "name",
            Self::None => "scan",
        }
    }
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let args: Vec<_> = args.into_iter().collect();
        let mut options = Self {
            root: home(),
            all: false,
            pdf: false,
            recent: false,
            sort: SortOrder::Recent,
            action: Action::Pick,
        };
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            match arg.to_str() {
                Some("--root") => {
                    i += 1;
                    options.root = PathBuf::from(args.get(i).ok_or("--root requires a directory")?);
                }
                Some("--all") => options.all = true,
                Some("--pdf") => options.pdf = true,
                Some("--recent") => options.recent = true,
                Some("--sort") => {
                    i += 1;
                    options.sort = match args.get(i).and_then(|arg| arg.to_str()) {
                        Some("recent") => SortOrder::Recent,
                        Some("name") => SortOrder::Name,
                        Some("none") => SortOrder::None,
                        _ => return Err("--sort requires recent, name or none".into()),
                    };
                }
                Some("--help" | "-h") => options.action = Action::Help,
                Some("--version" | "-V") => options.action = Action::Version,
                Some(
                    "--list"
                    | "--latest-download"
                    | "--window"
                    | "--copy"
                    | "--path"
                    | "--drag"
                    | "--cached-list"
                    | "--index-update"
                    | "--index-update-if-idle"
                    | "--index-status"
                    | "--index-remove"
                    | "--index-clear"
                    | "--index-stream"
                    | "--prepare-terminal",
                ) => {
                    if options.action != Action::Pick {
                        return Err("Choose only one picker or index action".into());
                    }
                    options.action = match arg.to_str().unwrap() {
                        "--list" => Action::List,
                        "--latest-download" => Action::LatestDownload,
                        "--window" => Action::Window,
                        "--prepare-terminal" => Action::PrepareTerminal,
                        "--cached-list" => Action::CachedList,
                        "--index-update" => Action::IndexUpdate,
                        "--index-update-if-idle" => Action::IndexUpdateIfIdle,
                        "--index-status" => Action::IndexStatus,
                        "--index-clear" => Action::IndexClear,
                        "--index-stream" => Action::IndexStream,
                        action => {
                            let mut paths = Vec::new();
                            while i + 1 < args.len() && !args[i + 1].as_bytes().starts_with(b"--") {
                                i += 1;
                                paths.push(PathBuf::from(&args[i]));
                            }
                            if paths.is_empty() {
                                return Err(format!("{action} requires a file").into());
                            }
                            match action {
                                "--copy" => Action::Copy(paths),
                                "--path" => Action::Path(paths),
                                "--index-remove" => Action::IndexRemove(paths),
                                _ => Action::Drag(paths),
                            }
                        }
                    };
                }
                _ => return Err(format!("Unknown argument: {}", arg.to_string_lossy()).into()),
            }
            i += 1;
        }
        options.root = absolute(&options.root)?;
        if options.pdf && options.recent {
            return Err("Choose --pdf or --recent".into());
        }
        Ok(options)
    }
}

pub fn scan_command(root: &Path, all: bool, pdf: bool) -> Result<Command> {
    if !root.is_dir() {
        return Err(format!("Search directory does not exist: {}", root.display()).into());
    }
    let resolved = root.canonicalize()?;
    if VIRTUAL
        .iter()
        .any(|virtual_root| resolved.starts_with(virtual_root))
    {
        return Err(format!("Virtual filesystem is excluded: {}", root.display()).into());
    }
    let mut command = Command::new("fd");
    command.args([
        "--type",
        "f",
        "--absolute-path",
        "--color=never",
        "--print0",
        "--max-buffer-time",
        "0",
    ]);
    if pdf {
        command.args(["--extension", "pdf"]);
    } else {
        command.args(["--type", "l"]);
    }
    if !pdf || all {
        command.arg("--no-ignore");
    }
    if all {
        command.arg("--hidden");
    }
    for virtual_root in VIRTUAL {
        if let Ok(relative) = Path::new(virtual_root).strip_prefix(&resolved) {
            command.arg("--exclude").arg(Path::new("/").join(relative));
        }
    }
    if !all && !pdf {
        for exclude in EXCLUDES {
            command.args(["--exclude", exclude]);
        }
    }
    command.arg(".").arg(root);
    Ok(command)
}

fn changed_time(metadata: &fs::Metadata) -> i128 {
    let modified = metadata.mtime() as i128 * 1_000_000_000 + metadata.mtime_nsec() as i128;
    let changed = metadata.ctime() as i128 * 1_000_000_000 + metadata.ctime_nsec() as i128;
    modified.max(changed)
}

fn scan_files(options: &Options) -> Result<Vec<(i128, PathBuf)>> {
    let mut command = scan_command(&options.root, options.all, options.pdf)?;
    let output = terminate_with_parent(&mut command).output()?;
    if !output.status.success() {
        return Err(format!("File scan failed ({})", output.status).into());
    }
    let mut files = Vec::new();
    for name in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let path = PathBuf::from(OsString::from_vec(name.to_vec()));
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        if metadata.is_file() {
            files.push((changed_time(&metadata), path));
        }
    }
    Ok(files)
}

fn sorted_paths(options: &Options) -> Result<Vec<PathBuf>> {
    let mut files = scan_files(options)?;
    // Recent mode always selects the newest set before applying display order.
    if options.recent {
        files.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        files.truncate(30_000);
    }
    match options.sort {
        SortOrder::Recent => {
            files.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)))
        }
        SortOrder::Name => files.sort_unstable_by(|a, b| a.1.cmp(&b.1)),
        SortOrder::None => {}
    }
    Ok(files.into_iter().map(|(_, path)| path).collect())
}

pub fn list_files(options: &Options) -> Result<()> {
    if options.sort == SortOrder::None && !options.recent {
        return Err(scan_command(&options.root, options.all, options.pdf)?
            .exec()
            .into());
    }
    let mut output = io::BufWriter::new(io::stdout().lock());
    for path in sorted_paths(options)? {
        output.write_all(path.as_os_str().as_bytes())?;
        output.write_all(b"\0")?;
    }
    output.flush()?;
    Ok(())
}

pub fn checked_files(paths: impl IntoIterator<Item = PathBuf>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        let path = absolute(&path)?;
        let c_path = CString::new(path.as_os_str().as_bytes())?;
        // access() checks readability without reading the attachment contents.
        if !path.is_file() || unsafe { libc::access(c_path.as_ptr(), libc::R_OK) } != 0 {
            return Err(format!("File is missing or unreadable: {}", path.display()).into());
        }
        if seen.insert(path.clone()) {
            files.push(path);
        }
    }
    if files.is_empty() {
        return Err("No files selected".into());
    }
    Ok(files)
}

pub fn file_uris(paths: &[PathBuf]) -> Vec<u8> {
    let mut output = Vec::new();
    const HEX: &[u8] = b"0123456789ABCDEF";
    for path in paths {
        output.extend_from_slice(b"file://");
        for &byte in path.as_os_str().as_bytes() {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                output.push(byte);
            } else {
                output.extend_from_slice(&[
                    b'%',
                    HEX[(byte >> 4) as usize],
                    HEX[(byte & 15) as usize],
                ]);
            }
        }
        output.extend_from_slice(b"\r\n");
    }
    output
}

// The terminal may be closed by the compositor instead of by a TUI key. Ensure
// scans do not survive that path either. Detached clipboard/drag helpers opt out.
fn terminate_with_parent(command: &mut Command) -> &mut Command {
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Picker closed"));
            }
            Ok(())
        });
    }
    command
}

pub fn detached(command: &mut Command) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // A new session lets clipboard ownership and drag cards survive the terminal.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
}

pub fn notify(title: &str, body: &str, urgent: bool) {
    let _ = detached(Command::new("notify-send").args([
        "-a",
        "File picker",
        "-u",
        if urgent { "critical" } else { "low" },
        "-t",
        "3500",
        title,
        body,
    ]))
    .spawn();
}

pub fn output_timeout(command: &mut Command, timeout: Duration) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = thread::spawn(move || {
        let mut data = Vec::new();
        stdout.read_to_end(&mut data).map(|_| data)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let data = reader
        .join()
        .map_err(|_| "Could not read command output")??;
    if status.is_some_and(|status| status.success()) {
        Ok(data)
    } else {
        Err("Command failed or timed out".into())
    }
}

pub fn copy_selection(paths: &[PathBuf], as_path: bool) -> Result<()> {
    if as_path && paths.len() != 1 {
        return Err("Select one file to copy its path".into());
    }
    let mime = if as_path {
        "text/plain;charset=utf-8"
    } else {
        "text/uri-list"
    };
    let content = if as_path {
        paths[0].as_os_str().as_bytes().to_vec()
    } else {
        file_uris(paths)
    };
    let mut provider = detached(
        Command::new("wl-copy")
            .args(["--foreground", "--type", mime, "--"])
            .arg(OsStr::from_bytes(&content)),
    )
    .spawn()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) && provider.try_wait()?.is_none() {
        if output_timeout(
            Command::new("wl-paste").args(["--no-newline", "--type", mime]),
            Duration::from_millis(300),
        )
        .is_ok_and(|bytes| bytes == content)
        {
            let title = if as_path {
                "File path copied".into()
            } else if paths.len() == 1 {
                "File copied".into()
            } else {
                format!("{} files copied", paths.len())
            };
            notify(
                &title,
                if as_path {
                    "Paste into the upload dialog's location field (Ctrl+L)."
                } else {
                    "Paste with Ctrl+V."
                },
                false,
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = provider.kill();
    let _ = provider.wait();
    Err("Could not acquire the Wayland clipboard".into())
}

pub fn latest_download(root: &Path) -> Result<PathBuf> {
    if !root.is_dir() {
        return Err(format!("Downloads directory does not exist: {}", root.display()).into());
    }
    let mut latest: Option<(i128, OsString, PathBuf)> = None;
    for entry in fs::read_dir(root)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let name = entry.file_name();
        if name.as_bytes().starts_with(b".")
            || path.extension().is_some_and(|extension| {
                ["crdownload", "part", "partial", "download", "tmp", "aria2"]
                    .iter()
                    .any(|suffix| extension.eq_ignore_ascii_case(suffix))
            })
        {
            continue;
        }
        if [".part", ".aria2"].iter().any(|suffix| {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            Path::new(&sidecar).exists()
        }) {
            continue;
        }
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
            continue;
        };
        if !metadata.is_file() || unsafe { libc::access(c_path.as_ptr(), libc::R_OK) } != 0 {
            continue;
        }
        let modified = metadata.mtime() as i128 * 1_000_000_000 + metadata.mtime_nsec() as i128;
        let changed = metadata.ctime() as i128 * 1_000_000_000 + metadata.ctime_nsec() as i128;
        let candidate = (modified.max(changed), name, path);
        if latest.as_ref().is_none_or(|previous| candidate > *previous) {
            latest = Some(candidate);
        }
    }
    latest
        .map(|(_, _, path)| path)
        .ok_or_else(|| format!("No completed downloads found in {}", root.display()).into())
}

fn active_window_address() -> Result<String> {
    let data = output_timeout(
        Command::new("hyprctl").args(["-j", "activewindow"]),
        Duration::from_secs(2),
    )?;
    let window: serde_json::Value = serde_json::from_slice(&data)?;
    let address = window["address"].as_str().unwrap_or("");
    if address.len() <= 2
        || !address.starts_with("0x")
        || !address[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Focus the chat's message box before pressing Super+R".into());
    }
    Ok(address.to_owned())
}

pub fn paste_latest_download() -> Result<()> {
    let address = active_window_address()?;
    let downloads = output_timeout(
        Command::new("xdg-user-dir").arg("DOWNLOAD"),
        Duration::from_secs(2),
    )
    .ok()
    .filter(|bytes| !bytes.is_empty())
    .map(|mut bytes| {
        while bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        PathBuf::from(OsString::from_vec(bytes))
    })
    .unwrap_or_else(|| home().join("Downloads"));
    let path = latest_download(&downloads)?;
    copy_selection(&checked_files([path.clone()])?, false)?;
    if active_window_address()? != address {
        notify(
            "Download copied",
            &format!(
                "{}\nFocus changed; paste with Ctrl+V.",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            false,
        );
        return Ok(());
    }
    let dispatcher = format!(
        "hl.dsp.send_shortcut({{ mods = \"CTRL\", key = \"V\", window = \"address:{address}\" }})"
    );
    let output = output_timeout(
        Command::new("hyprctl").args(["dispatch", &dispatcher]),
        Duration::from_secs(2),
    )?;
    if output != b"ok\n" {
        return Err("Download copied; automatic paste failed. Paste with Ctrl+V.".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: &'static str,
    pub fg: &'static str,
    pub surface: &'static str,
    pub selected: &'static str,
    pub accent: &'static str,
    pub muted: &'static str,
    pub border: &'static str,
    pub on_accent: &'static str,
}

pub fn appearance_colors() -> Colors {
    let light = match env::var("FILE_PICKER_APPEARANCE").as_deref() {
        Ok("light") => true,
        Ok("dark") => false,
        _ => output_timeout(
            Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "color-scheme"]),
            Duration::from_millis(150),
        )
        .is_ok_and(|bytes| matches!(bytes.trim_ascii(), b"'prefer-light'" | b"'default'")),
    };
    if light {
        Colors {
            bg: "#eff1f5",
            fg: "#4c4f69",
            surface: "#e6e9ef",
            selected: "#dce6fa",
            accent: "#1e66f5",
            muted: "#6c6f85",
            border: "#bcc0cc",
            on_accent: "#ffffff",
        }
    } else {
        Colors {
            bg: "#11111b",
            fg: "#cdd6f4",
            surface: "#1e1e2e",
            selected: "#26334d",
            accent: "#89b4fa",
            muted: "#9399b2",
            border: "#45475a",
            on_accent: "#11111b",
        }
    }
}

pub fn drag_binary() -> Result<PathBuf> {
    Ok(executable_path()?.with_file_name("file-picker-drag"))
}

fn executable_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("FILE_PICKER_EXECUTABLE") {
        return Ok(PathBuf::from(path));
    }
    let path = env::current_exe()?;
    // Linux appends this suffix when an executable is replaced while running.
    let bytes = path.as_os_str().as_bytes();
    Ok(bytes
        .strip_suffix(b" (deleted)")
        .map(|bytes| PathBuf::from(OsStr::from_bytes(bytes)))
        .unwrap_or(path))
}

fn scan_args(command: &mut Command, options: &Options, action: &str) {
    if options.recent {
        command.arg("--recent");
    }
    command.arg(action).arg("--root").arg(&options.root).args([
        "--sort",
        match options.sort {
            SortOrder::Recent => "recent",
            SortOrder::Name => "name",
            SortOrder::None => "none",
        },
    ]);
    if options.pdf {
        command.arg("--pdf");
    }
    if options.all {
        command.arg("--all");
    }
}

pub fn pick(options: &Options) -> Result<()> {
    let Some(selection) = tui::pick(options)? else {
        return Ok(());
    };
    let paths = checked_files(selection.paths)?;
    match selection.action {
        tui::SelectionAction::Copy => copy_selection(&paths, false),
        tui::SelectionAction::Path => copy_selection(&paths, true),
        tui::SelectionAction::Drag => {
            detached(Command::new(drag_binary()?).args(paths)).spawn()?;
            Ok(())
        }
        tui::SelectionAction::Open | tui::SelectionAction::OpenPdf => {
            for path in paths {
                if selection.action == tui::SelectionAction::OpenPdf {
                    detached(Command::new("zathura").arg("--fork").arg(path)).spawn()?;
                } else {
                    detached(Command::new("xdg-open").arg(path)).spawn()?;
                }
            }
            Ok(())
        }
    }
}

fn shell_quote(value: &OsStr) -> OsString {
    let mut quoted = vec![b'\''];
    for byte in value.as_bytes() {
        if *byte == b'\'' {
            quoted.extend_from_slice(b"'\\''");
        } else {
            quoted.push(*byte);
        }
    }
    quoted.push(b'\'');
    OsString::from_vec(quoted)
}

fn picker_name(options: &Options) -> &'static str {
    if options.recent {
        "recent-picker"
    } else if options.pdf {
        "pdf-picker"
    } else {
        "file-picker"
    }
}

const PICKER_CLASS: &str = "file.picker";

fn picker_window(options: &Options) -> Result<Option<String>> {
    let data = output_timeout(
        Command::new("hyprctl").args(["-j", "clients"]),
        Duration::from_secs(2),
    )?;
    let clients: Vec<serde_json::Value> = serde_json::from_slice(&data)?;
    Ok(clients
        .iter()
        .find(|client| client["class"] == PICKER_CLASS && client["title"] == picker_name(options))
        .and_then(|client| client["address"].as_str())
        .filter(|address| {
            address.starts_with("0x") && address[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .map(str::to_owned))
}

fn hypr_eval(code: &str) -> Result<()> {
    output_timeout(
        Command::new("hyprctl").args(["eval", code]),
        Duration::from_secs(2),
    )?;
    Ok(())
}

fn launch_lock() -> Result<fs::File> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR must be set")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(runtime.join("file-picker-launch.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(lock)
}

fn terminal_running() -> Result<bool> {
    let output = output_timeout(
        Command::new("gdbus").args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            PICKER_CLASS,
        ]),
        Duration::from_secs(1),
    )?;
    Ok(output.trim_ascii() == b"(true,)")
}

fn ensure_terminal() -> Result<()> {
    if terminal_running()? {
        return Ok(());
    }
    let mut fallback = OsString::from("--command=shell:exec ");
    fallback.push(shell_quote(executable_path()?.as_os_str()));
    let mut command = Command::new("ghostty");
    command
        .args([
            &format!("--class={PICKER_CLASS}"),
            "--gtk-single-instance=true",
            "--initial-window=false",
            // These short-lived surfaces launch detached clipboard/index helpers.
            // Avoid per-surface systemd setup and keep their lifetime independent.
            "--linux-cgroup=never",
            "--quit-after-last-window-closed=false",
            "--wait-after-command=false",
            "--confirm-close-surface=false",
            "--title=file-picker",
            "--font-size=13",
            "--window-padding-x=10",
            "--window-padding-y=8",
            "--shell-integration=none",
            "--background-opacity=1",
        ])
        .arg(fallback);
    let mut host = detached(&mut command).spawn()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if terminal_running()? {
            return Ok(());
        }
        if host.try_wait()?.is_some_and(|status| !status.success()) {
            return Err("Could not prepare the picker terminal".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = host.kill();
    let _ = host.wait();
    Err("The picker terminal did not register with D-Bus".into())
}

/// Prepare GTK/fonts once with zero windows instead of once per shortcut press.
pub fn prepare_terminal() -> Result<()> {
    let _lock = launch_lock()?;
    ensure_terminal()
}

fn terminal_request(options: &Options, executable: &Path) -> Command {
    let mut command = Command::new("ghostty");
    command.args([
        "+new-window",
        &format!("--class={PICKER_CLASS}"),
        &format!("--title={}", picker_name(options)),
        "-e",
        "env",
    ]);
    // The persistent host has its original environment. Forward documented
    // per-invocation overrides so independent caches cannot leak between calls.
    let variables = [
        "FILE_PICKER_INDEX",
        "FILE_PICKER_APPEARANCE",
        "FILE_PICKER_EXECUTABLE",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "PATH",
        "NO_COLOR",
        "CLICOLOR",
        "CLICOLOR_FORCE",
    ];
    for name in variables {
        command.args(["-u", name]);
    }
    command.arg("--");
    for name in variables {
        let value = if name == "FILE_PICKER_EXECUTABLE" {
            Some(executable.as_os_str().to_owned())
        } else {
            env::var_os(name)
        };
        if let Some(value) = value {
            let mut argument = OsString::from(name);
            argument.push("=");
            argument.push(value);
            command.arg(argument);
        }
    }
    command
        .arg(executable)
        .arg("--root")
        .arg(&options.root)
        .arg("--sort")
        .arg(options.sort.argument());
    if options.pdf {
        command.arg("--pdf");
    }
    if options.recent {
        command.arg("--recent");
    }
    if options.all {
        command.arg("--all");
    }
    command
}

fn standalone_terminal(options: &Options, executable: &Path) -> Result<std::process::Child> {
    // D-Bus arguments must be UTF-8; retain a byte-safe fallback for other roots.
    let mut argument = OsString::from("--command=shell:exec ");
    argument.push(shell_quote(executable.as_os_str()));
    argument.push(" --root ");
    argument.push(shell_quote(options.root.as_os_str()));
    argument.push(" --sort ");
    argument.push(options.sort.argument());
    if options.pdf {
        argument.push(" --pdf");
    }
    if options.recent {
        argument.push(" --recent");
    }
    if options.all {
        argument.push(" --all");
    }
    let mut command = Command::new("ghostty");
    command
        .args([
            &format!("--class={PICKER_CLASS}"),
            &format!("--title={}", picker_name(options)),
            "--gtk-single-instance=false",
            "--quit-after-last-window-closed=true",
            "--wait-after-command=false",
            "--confirm-close-surface=false",
            "--font-size=13",
            "--window-padding-x=10",
            "--window-padding-y=8",
            "--shell-integration=none",
            "--background-opacity=1",
        ])
        .arg(argument);
    Ok(detached(&mut command).spawn()?)
}

fn terminal_dbus_request(request: &Command) -> Result<Command> {
    // This is the same public GAction used by Ghostty's +new-window command,
    // without starting another GTK-linked Ghostty command-line process.
    let arguments: Vec<_> = request
        .get_args()
        .skip(2)
        .map(|arg| {
            arg.to_str()
                .ok_or("D-Bus requires UTF-8 terminal arguments")
        })
        .collect::<std::result::Result<_, _>>()?;
    let parameter = format!("[<{}>]", serde_json::to_string(&arguments)?);
    let mut command = Command::new("gdbus");
    command.args([
        "call",
        "--session",
        "--dest",
        PICKER_CLASS,
        "--object-path",
        "/file/picker",
        "--method",
        "org.gtk.Actions.Activate",
        "new-window-command",
        &parameter,
        "{}",
    ]);
    Ok(command)
}

/// Open a disposable floating window through a shared, windowless Ghostty host.
pub fn window(options: &Options) -> Result<()> {
    let _lock = launch_lock()?;
    let address = picker_window(options)?;
    let title = picker_name(options);
    hypr_eval(&format!(
        "for _,w in ipairs(hl.get_windows()) do if (w.class=='file.picker' or w.class=='pdf.picker' or w.class=='recent.picker') and (w.class~='{PICKER_CLASS}' or w.title~='{title}') then hl.dispatch(hl.dsp.window.close({{window=w}})) end end"
    ))?;
    if let Some(address) = address {
        hypr_eval(&format!(
            "local w='address:{address}'; hl.dispatch(hl.dsp.window.move({{window=w,workspace=hl.get_active_workspace(),follow=false}})); hl.dispatch(hl.dsp.window.center({{window=w}})); hl.dispatch(hl.dsp.focus({{window=w}}))"
        ))?;
        return Ok(());
    }
    let executable = executable_path()?;
    let mut request = terminal_request(options, &executable);
    let utf8 = request
        .get_args()
        .all(|argument| argument.to_str().is_some());
    let mut standalone = if utf8 {
        ensure_terminal()?;
        let mut direct = terminal_dbus_request(&request)?;
        if output_timeout(&mut direct, Duration::from_secs(3)).is_err() {
            output_timeout(&mut request, Duration::from_secs(3))?;
        }
        None
    } else {
        Some(standalone_terminal(options, &executable)?)
    };
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if picker_window(options)?.is_some() {
            return Ok(());
        }
        if let Some(terminal) = &mut standalone
            && terminal.try_wait()?.is_some()
        {
            return Err("The picker terminal exited before opening".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    if let Some(mut terminal) = standalone {
        let _ = terminal.kill();
        let _ = terminal.wait();
    }
    Err("The picker terminal did not open".into())
}

pub const HELP: &str = "File picker (Ratatui + SQLite)

Usage: file-picker [--root DIRECTORY] [--all] [--pdf | --recent] [--sort recent|name|none] [ACTION]

Actions:
  --list                Live scan; stream NUL-delimited file paths
  --cached-list         Read cached paths without scanning
  --copy FILE...        Copy files as attachments
  --path FILE           Copy one absolute file path
  --drag FILE...        Show a native drag card
  --latest-download     Copy the newest completed download and paste it
  --window              Open a floating picker; close on selection or Escape
  --prepare-terminal    Prepare a terminal host with zero windows

Index maintenance:
  --index-update        Refresh the current scope; add, update and prune rows
  --index-status        Show cached scopes, counts and last refresh times
  --index-remove FILE... Remove cached paths from all scopes; keep actual files
  --index-clear         Clear the current root/mode cache; keep actual files

  --help, -h            Show this help
  --version, -V         Show version

The picker shows cached results first, refreshes in the background on open,
and refreshes every 30 seconds while open. Ctrl+R refreshes immediately.
FILE_PICKER_INDEX overrides the SQLite cache path.
Without an action, open the Ratatui picker in the current terminal.
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_ipc_preserves_argument_boundaries_and_uses_a_shared_host() {
        let options = Options::parse(
            ["--root", "/tmp/a folder's $data", "--pdf", "--sort", "name"].map(OsString::from),
        )
        .unwrap();
        let executable = Path::new("/tmp/tool folder/picker");
        let command = terminal_request(&options, executable);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args[0], "+new-window");
        assert_eq!(args[1], "--class=file.picker");
        assert_eq!(args[2], "--title=pdf-picker");
        let start = args
            .iter()
            .position(|arg| *arg == executable.as_os_str())
            .unwrap();
        assert_eq!(
            &args[start..],
            [
                executable.as_os_str(),
                OsStr::new("--root"),
                options.root.as_os_str(),
                OsStr::new("--sort"),
                OsStr::new("name"),
                OsStr::new("--pdf")
            ]
        );
        for variable in ["FILE_PICKER_INDEX", "FILE_PICKER_APPEARANCE"] {
            assert!(
                args.windows(2)
                    .any(|pair| pair == [OsStr::new("-u"), OsStr::new(variable)])
            );
        }
        assert_eq!(
            Options::parse([OsString::from("--prepare-terminal")])
                .unwrap()
                .action,
            Action::PrepareTerminal
        );
        let direct = terminal_dbus_request(&command).unwrap();
        assert_eq!(direct.get_program(), "gdbus");
        let direct_args: Vec<_> = direct.get_args().collect();
        let parameter = direct_args[direct_args.len() - 2].to_str().unwrap();
        let decoded: Vec<String> = serde_json::from_str(
            parameter
                .strip_prefix("[<")
                .unwrap()
                .strip_suffix(">]")
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            decoded,
            args[2..]
                .iter()
                .map(|arg| arg.to_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn default_search_skips_app_state_and_android_sdk_but_all_can_find_them() {
        let root = env::temp_dir().join(format!("picker-noise-test-{}", std::process::id()));
        let names = [
            "Downloads/report.pdf",
            "project/main.rs",
            "project/.env",
            ".claude/file-history/internal.json",
            "Android/Sdk/skins/mask.webp",
            "R/x86_64-pc-linux-gnu-library/4.5/package/help/index.rds",
            "VirtualBox VMs/example/disk.vdi",
            "project/analysis.R",
        ];
        for name in names {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"fixture").unwrap();
        }
        let scan = |all| {
            let output = scan_command(&root, all, false).unwrap().output().unwrap();
            assert!(output.status.success());
            output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|name| !name.is_empty())
                .map(|name| PathBuf::from(OsString::from_vec(name.to_vec())))
                .collect::<HashSet<_>>()
        };
        let normal = scan(false);
        let all = scan(true);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            normal,
            [
                root.join(names[0]),
                root.join(names[1]),
                root.join(names[7])
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(all, names.iter().map(|name| root.join(name)).collect());
    }

    #[test]
    fn recent_and_name_sort_preserve_paths_and_change_order() {
        let root = env::temp_dir().join(format!("picker-sort-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let old = root.join("aaa old\nfile.pdf");
        let new = root.join("zzz new.pdf");
        fs::write(&old, b"older").unwrap();
        thread::sleep(Duration::from_millis(20));
        fs::write(&new, b"newer").unwrap();
        let mut options = Options::parse([
            OsString::from("--root"),
            root.as_os_str().to_owned(),
            OsString::from("--pdf"),
        ])
        .unwrap();
        assert_eq!(sorted_paths(&options).unwrap(), [new.clone(), old.clone()]);
        options.sort = SortOrder::Name;
        assert_eq!(sorted_paths(&options).unwrap(), [old, new]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attachment_uris_escape_bytes_and_delimit_multiple_files() {
        let paths = [
            PathBuf::from("/tmp/ø #\n%.pdf"),
            PathBuf::from(OsString::from_vec(b"/tmp/\xff".to_vec())),
        ];
        assert_eq!(
            file_uris(&paths),
            b"file:///tmp/%C3%B8%20%23%0A%25.pdf\r\nfile:///tmp/%FF\r\n"
        );
    }

    #[test]
    fn roots_and_shell_commands_preserve_literal_paths() {
        let options =
            Options::parse(["--root", "/tmp/a')+execute(touch bad)", "--all"].map(OsString::from))
                .unwrap();
        assert_eq!(options.root, Path::new("/tmp/a')+execute(touch bad)"));
        assert!(options.all);
        assert_eq!(shell_quote(OsStr::new("a'b")), OsString::from("'a'\\''b'"));
    }

    #[test]
    fn reject_virtual_roots_and_conflicting_actions() {
        assert!(scan_command(Path::new("/proc"), true, false).is_err());
        assert!(Options::parse(["--list", "--latest-download"].map(OsString::from)).is_err());
    }

    #[test]
    fn download_selection_ignores_unfinished_files_and_keeps_older_server_dates() {
        let root = env::temp_dir().join(format!("file-picker-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let result = (|| -> Result<()> {
            let old = root.join("old.txt");
            fs::write(&old, b"old")?;
            thread::sleep(Duration::from_millis(20));
            let new = root.join("new.txt");
            fs::write(&new, b"new")?;
            let times = [libc::timespec {
                tv_sec: 1,
                tv_nsec: 0,
            }; 2];
            let path = CString::new(new.as_os_str().as_bytes())?;
            assert_eq!(
                unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) },
                0
            );
            for name in [
                "incomplete.PART",
                ".hidden",
                "sidecar.txt",
                "sidecar.txt.aria2",
            ] {
                fs::write(root.join(name), b"partial")?;
            }
            assert_eq!(latest_download(&root)?, new);
            assert_eq!(checked_files([old.clone(), old.clone()])?, [old]);
            Ok(())
        })();
        fs::remove_dir_all(root).unwrap();
        result.unwrap();
    }
}
