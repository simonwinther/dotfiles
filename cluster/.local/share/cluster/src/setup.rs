use crate::process::{prepend_path, quote, run, with_input};
use crate::remote::{self, home};
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PROBE: &str = include_str!("probe.sh");
const PATH_LINE: &str = r#"case "$PATH" in "$HOME/.local/bin"|"$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac # cluster-setup"#;

#[derive(Args, Debug)]
pub struct SetupArgs {
    #[arg(value_name = "SSH_HOST")]
    pub host: Option<String>,
    /// Report available tools without changing the target or downloading files
    #[arg(long)]
    pub check: bool,
    /// Set up this machine
    #[arg(long)]
    pub local: bool,
    /// Skip tmux plugin downloads
    #[arg(long)]
    pub skip_plugins: bool,
    /// SSH port (SSH config aliases and ProxyJump also work)
    #[arg(short, long, value_parser = clap::value_parser!(u16).range(1..))]
    pub port: Option<u16>,
    #[arg(long, hide = true)]
    pub source: Option<PathBuf>,
}

#[derive(Default)]
struct Tools {
    missing: Vec<String>,
    wrappers: Vec<String>,
    need_stow: bool,
    compiler: bool,
    old_tmux: bool,
    private_stow: Option<PathBuf>,
}

struct Probe {
    success: bool,
    output: String,
    location: String,
}

fn version(text: &str) -> Option<(u64, u64, u64)> {
    let re = Regex::new(r"([0-9]+)\.([0-9]+)(?:\.([0-9]+))?").unwrap();
    let caps = re.captures(text)?;
    Some((
        caps[1].parse().ok()?,
        caps[2].parse().ok()?,
        caps.get(3).map_or(Some(0), |v| v.as_str().parse().ok())?,
    ))
}

fn parse_probe(bytes: &[u8]) -> Result<Tools> {
    let mut fields = bytes.split(|b| *b == 0).collect::<Vec<_>>();
    ensure!(
        fields.pop() == Some(&b""[..]) && fields.len() % 4 == 0,
        "Unexpected tool probe output; check shell startup messages on stdout."
    );
    let mut probes = BTreeMap::new();
    for record in fields.as_chunks::<4>().0 {
        let text = |index: usize| {
            String::from_utf8(record[index].to_vec()).context("Tool probe output is not UTF-8")
        };
        probes.insert(
            text(0)?,
            Probe {
                success: text(1)? == "0",
                output: text(2)?,
                location: text(3)?,
            },
        );
    }
    let get = |key: &str| {
        probes
            .get(key)
            .with_context(|| format!("Missing tool probe field: {key}"))
    };
    println!("\nChecking tools on {}", get("platform")?.output);
    let mut tools = Tools::default();
    for (command, package, minimum) in [
        ("nvim", "nvim>=0.12.0", "0.12.0"),
        ("tmux", "tmux>=3.2", "3.2"),
        ("git", "git>=2.19", "2.19"),
        ("rg", "ripgrep", ""),
        ("fd", "fd-find", ""),
        ("fzf", "fzf>=0.25", "0.25"),
        ("tree-sitter", "tree-sitter-cli>=0.26.1", "0.26.1"),
        ("curl", "curl", ""),
        ("unzip", "unzip", ""),
        // Python remains a Neovim/project tool, not the cluster runtime.
        ("python3", "python>=3.6", "3.6"),
    ] {
        let found = get(command)?;
        if found.success
            && (minimum.is_empty()
                || version(&found.output)
                    .zip(version(minimum))
                    .is_some_and(|(a, b)| a >= b))
        {
            println!(
                "  {command:<14} {}",
                found.output.lines().next().unwrap_or("")
            );
        } else {
            println!(
                "  {command:<14} missing or too old{}",
                if minimum.is_empty() {
                    String::new()
                } else {
                    format!(" (need >= {minimum})")
                }
            );
            tools.missing.push(package.to_owned());
            tools.wrappers.push(command.to_owned());
        }
    }
    let stow = get("stow")?;
    if stow.success {
        println!("  {:<14} {}", "stow", stow.location);
        if stow
            .location
            .ends_with("/.local/share/cluster-setup/stow/bin/stow")
        {
            tools.private_stow = Some(PathBuf::from(&stow.location));
        }
    } else {
        println!("  {:<14} missing (will install GNU Stow)", "stow");
        tools.need_stow = true;
        for tool in ["perl", "make"] {
            if !get(tool)?.success {
                tools.missing.push(tool.into());
                tools.wrappers.push(tool.into());
            }
        }
    }
    let compiler = get("compiler")?;
    tools.compiler = !compiler.output.is_empty();
    if tools.compiler {
        println!("  {:<14} {}", "C compiler", compiler.output);
    } else {
        println!(
            "  {:<14} missing (will install for Treesitter)",
            "C compiler"
        );
        tools.missing.push("c-compiler".into());
    }
    tools.old_tmux = old_tmux(&get("tmux-server")?.output);
    if tools.old_tmux {
        print_old_tmux();
    }
    Ok(tools)
}

fn old_tmux(text: &str) -> bool {
    text.contains("server version is too old for client")
        || text.contains("protocol version mismatch")
}

fn print_old_tmux() {
    println!(
        "  {:<14} incompatible older server on the default socket; existing sessions kept running",
        "tmux server"
    );
}

fn probe(command: &mut Command) -> Result<Tools> {
    command.stderr(Stdio::inherit()).stdout(Stdio::piped());
    let result = with_input(command, PROBE.as_bytes())?;
    ensure!(result.status.success(), "Tool check failed.");
    parse_probe(&result.stdout)
}

pub fn setup(
    args: SetupArgs,
    global_host: Option<String>,
    global_local: bool,
    source_root: Option<PathBuf>,
) -> Result<()> {
    ensure!(
        global_host.is_none() || args.host.is_none() || global_host == args.host,
        "Choose one setup host, using either --host or the setup argument."
    );
    let local = global_local || args.local;
    let host = args.host.clone().or(global_host);
    ensure!(
        !(local && host.is_some()),
        "Choose an SSH host or --local for setup."
    );
    ensure!(
        !local || args.port.is_none(),
        "An SSH port cannot be used with --local."
    );
    ensure!(
        local || host.is_some(),
        "Choose a target: cluster setup SSH_HOST, or cluster setup --local."
    );
    ensure!(local || args.source.is_none(), "--source requires --local");
    if local {
        local_install(&args, args.source.clone().or(source_root))
    } else {
        let host = remote::validate_host(host.as_deref().unwrap())?;
        if args.check {
            probe(&mut remote::ssh(host, args.port, "bash -s")?)?;
            println!("\nCheck complete. No files changed.");
            Ok(())
        } else {
            remote_install(
                &args,
                host,
                source_root
                    .context("Run setup from the dotfiles checkout on your PC")?
                    .as_path(),
            )
        }
    }
}

fn validate_source(source: &Path) -> Result<()> {
    ensure!(
        source.join("nvim/.config/nvim/init.lua").is_file()
            && source.join("tmux/.tmux.conf").is_file()
            && source.join("cluster/.local/bin/cluster-setup").is_file(),
        "The source must include the nvim, tmux, and cluster packages"
    );
    Ok(())
}

fn remote_install(args: &SetupArgs, host: &str, source: &Path) -> Result<()> {
    let source = source.canonicalize()?;
    validate_source(&source)?;
    let executable = remote::binary()?;
    let arch = remote::binary_arch(&executable)?;
    let bundle = tempfile::tempdir()?;
    let archive = bundle.path().join("payload.tar");
    let tracked = Command::new("git")
        .args(["ls-files", "-z", "--", "nvim", "tmux"])
        .current_dir(&source)
        .output()?;
    ensure!(
        tracked.status.success(),
        "Run remote setup from the dotfiles checkout on your PC"
    );
    let mut files = tracked.stdout;
    for relative in [
        "cluster/.local/bin/cluster",
        "cluster/.local/bin/cluster-setup",
        "cluster/.local/bin/cluster-build",
    ] {
        files.extend(relative.as_bytes());
        files.push(0);
    }
    let rust_root = source.join("cluster/.local/share/cluster");
    files.extend(b"cluster/.local/share/cluster/Cargo.toml\0");
    for entry in fs::read_dir(rust_root.join("src"))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "rs")
            && path.file_name().is_none_or(|name| name != "probe.sh")
        {
            continue;
        }
        ensure!(
            entry.file_type()?.is_file(),
            "Unexpected directory in cluster Rust sources"
        );
        files.extend(path.strip_prefix(&source)?.as_os_str().as_encoded_bytes());
        files.push(0);
    }
    let list = bundle.path().join("files");
    fs::write(&list, files)?;
    run(Command::new("tar")
        .arg("-C")
        .arg(&source)
        .args(["--null", "--verbatim-files-from", "-T"])
        .arg(list)
        .arg("-cf")
        .arg(&archive))?;
    let native_relative = "cluster/.local/share/cluster/bin/cluster-native";
    let native = bundle.path().join(native_relative);
    fs::create_dir_all(native.parent().unwrap())?;
    fs::copy(executable, &native)?;
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700))?;
    run(Command::new("tar")
        .arg("-C")
        .arg(bundle.path())
        .arg("-rf")
        .arg(&archive)
        .arg(native_relative))?;
    let mut terminfo = Vec::new();
    for terminal in [
        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into()),
        "tmux-256color".into(),
    ] {
        if let Ok(result) = Command::new("infocmp")
            .args(["-x", "--", &terminal])
            .output()
            && result.status.success()
        {
            terminfo.extend(result.stdout);
        }
    }
    if !terminfo.is_empty() {
        fs::write(bundle.path().join("terminal.info"), terminfo)?;
        run(Command::new("tar")
            .arg("-C")
            .arg(bundle.path())
            .arg("-rf")
            .arg(&archive)
            .arg("terminal.info"))?;
    }
    let script = format!(
        r#"set -euo pipefail
umask 077
{}state="${{CLUSTER_SETUP_HOME:-$HOME}}/.local/share/cluster-setup"
mkdir -p -- "$state"
incoming=$(mktemp -d "$state/incoming.XXXXXX")
trap 'rm -rf -- "$incoming"' EXIT
tar -xf - -C "$incoming"
"$incoming/cluster/.local/bin/cluster-setup" --local --source "$incoming"{}
"#,
        remote::arch_guard(arch),
        if args.skip_plugins {
            " --skip-plugins"
        } else {
            ""
        }
    );
    let mut command = remote::ssh(host, args.port, &format!("bash -c {}", quote(&script)))?;
    command.stderr(Stdio::inherit()).stdout(Stdio::inherit());
    let result = with_input(&mut command, fs::File::open(archive)?)?;
    std::io::stdout().write_all(&result.stdout)?;
    ensure!(result.status.success(), "Remote setup failed.");
    Ok(())
}

struct Install {
    home: PathBuf,
    state: PathBuf,
    packages: PathBuf,
    prefix: PathBuf,
    backup: PathBuf,
    path: OsString,
    moved: Vec<PathBuf>,
    copied: Vec<String>,
    links_before: BTreeSet<PathBuf>,
    linking_started: bool,
    applied: bool,
}

impl Install {
    fn owned_links(&self) -> Result<BTreeSet<PathBuf>> {
        fn collect(path: &Path, packages: &Path, links: &mut BTreeSet<PathBuf>) -> Result<()> {
            let metadata = match fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            if metadata.file_type().is_symlink() {
                if normalized(&path.parent().unwrap().join(fs::read_link(path)?))
                    .starts_with(packages)
                {
                    links.insert(path.to_path_buf());
                }
            } else if metadata.is_dir() {
                for entry in fs::read_dir(path)? {
                    collect(&entry?.path(), packages, links)?;
                }
            }
            Ok(())
        }
        let mut links = BTreeSet::new();
        for relative in [
            ".config/nvim",
            ".local/bin",
            ".local/share/cluster",
            ".tmux.conf",
            ".tmux.conf.custom",
        ] {
            collect(&self.home.join(relative), &self.packages, &mut links)?;
        }
        Ok(links)
    }

    fn command(&self, name: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(name);
        command.env("PATH", &self.path);
        command
    }

    fn backup_path(&mut self, relative: &Path) -> Result<()> {
        let path = self.home.join(relative);
        if fs::symlink_metadata(&path).is_err() {
            return Ok(());
        }
        if path
            .canonicalize()
            .is_ok_and(|p| p.starts_with(&self.packages))
        {
            return Ok(());
        }
        let backup = self.backup.join(relative);
        fs::create_dir_all(backup.parent().unwrap())?;
        fs::rename(&path, backup)?;
        self.moved.push(relative.to_path_buf());
        println!("  Backed up {}", path.display());
        Ok(())
    }

    fn replace_package(&mut self, source: &Path, package: &str) -> Result<()> {
        let destination = self.packages.join(package);
        fs::create_dir_all(&self.packages)?;
        if destination.exists() {
            let backup = self.backup.join("previous-source").join(package);
            fs::create_dir_all(backup.parent().unwrap())?;
            fs::rename(&destination, backup)?;
        }
        self.copied.push(package.to_owned());
        fs::create_dir_all(&destination)?;
        run(self
            .command("cp")
            .args(["-a", "--"])
            .arg(source.join("."))
            .arg(&destination))
    }

    fn fetch(&self, url: &str, destination: &Path) -> Result<()> {
        if available("curl", &self.path) {
            run(self
                .command("curl")
                .args([
                    "-fLsS",
                    "--retry",
                    "2",
                    "--connect-timeout",
                    "20",
                    "--max-time",
                    "300",
                    "-o",
                ])
                .arg(destination)
                .arg(url))
        } else if available("wget", &self.path) {
            run(self
                .command("wget")
                .args(["-q", "--timeout=30", "--tries=3", "-O"])
                .arg(destination)
                .arg(url))
        } else {
            bail!("curl or wget is needed to bootstrap downloads; load a module providing one");
        }
    }

    fn startup(&self, relative: &str) -> Result<()> {
        let path = self.home.join(relative);
        let mut content = if path.exists() {
            fs::read(&path)?
        } else {
            Vec::new()
        };
        if content
            .windows(b"# cluster-setup".len())
            .any(|v| v == b"# cluster-setup")
        {
            return Ok(());
        }
        if fs::symlink_metadata(&path).is_ok() {
            fs::create_dir_all(&self.backup)?;
            run(self
                .command("cp")
                .args(["-a", "--"])
                .arg(&path)
                .arg(self.backup.join(relative)))?;
        }
        content.extend(format!("\n{PATH_LINE}\n").as_bytes());
        let mode = fs::metadata(&path).map_or(0o600, |m| m.permissions().mode() & 0o777);
        let mut file = tempfile::NamedTempFile::new_in(&self.home)?;
        file.write_all(&content)?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
        file.persist(path)?;
        Ok(())
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        if self.applied {
            return;
        }
        let restore = || -> Result<()> {
            if self.linking_started {
                for link in self.owned_links()? {
                    if !self
                        .links_before
                        .iter()
                        .any(|before| link.starts_with(before))
                    {
                        fs::remove_file(link)?;
                    }
                }
            }
            // These are exclusively the private package paths replaced here.
            for package in self.copied.iter().rev() {
                let destination = self.packages.join(package);
                if destination.exists() {
                    fs::remove_dir_all(&destination)?;
                }
                let previous = self.backup.join("previous-source").join(package);
                if previous.exists() {
                    fs::rename(previous, destination)?;
                }
            }
            for relative in self.moved.iter().rev() {
                let path = self.home.join(relative);
                if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
                    && fs::read_link(&path).is_ok_and(|p| {
                        normalized(&path.parent().unwrap().join(p)).starts_with(&self.packages)
                    })
                {
                    fs::remove_file(&path)?;
                }
                if fs::symlink_metadata(&path).is_err() {
                    fs::rename(self.backup.join(relative), path)?;
                }
            }
            Ok(())
        };
        match restore() {
            Ok(()) => eprintln!("Setup failed before linking; original configurations restored."),
            Err(error) => eprintln!(
                "Setup rollback needs attention: {error:#}. Backups: {}",
                self.backup.display()
            ),
        }
    }
}

fn normalized(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

fn available(name: &str, path: &OsStr) -> bool {
    std::env::split_paths(path).any(|dir| {
        fs::metadata(dir.join(name))
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

fn wrapper(directory: &Path, name: &str, executable: &Path) -> Result<()> {
    ensure!(
        executable.is_file() && fs::metadata(executable)?.permissions().mode() & 0o111 != 0,
        "Installation did not provide {name}"
    );
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    writeln!(
        file,
        "#!/bin/sh\nexec {} \"$@\"",
        quote(executable.to_str().context("Tool path is not UTF-8")?)
    )?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o700))?;
    file.persist(directory.join(name))?;
    Ok(())
}

fn local_install(args: &SetupArgs, source: Option<PathBuf>) -> Result<()> {
    let target = std::env::var_os("CLUSTER_SETUP_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?);
    ensure!(
        target.is_absolute(),
        "The target home must be an absolute path"
    );
    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let path = prepend_path(&[&target.join(".local/bin")], &original_path)?;
    let mut tools = probe(Command::new("bash").arg("-s").env("PATH", &path))?;
    if args.check {
        println!("\nCheck complete. No files changed.");
        return Ok(());
    }
    ensure!(
        std::env::consts::OS == "linux",
        "Installation currently supports Linux"
    );
    ensure!(
        std::env::var_os("XDG_CONFIG_HOME")
            .is_none_or(|v| v.is_empty() || Path::new(&v) == target.join(".config")),
        "This Stow layout requires XDG_CONFIG_HOME to be unset or ~/.config"
    );
    let source = source
        .context("The source must include the nvim, tmux, and cluster packages")?
        .canonicalize()?;
    validate_source(&source)?;
    for parent in [
        ".config",
        ".local",
        ".local/bin",
        ".local/share",
        ".config/tmux",
    ] {
        ensure!(
            !fs::symlink_metadata(target.join(parent)).is_ok_and(|m| m.file_type().is_symlink()),
            "Make ~/{parent} a real directory before setup (it is currently a symlink)"
        );
    }
    let state = target.join(".local/share/cluster-setup");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&state)?;
    let state = state.canonicalize()?;
    let work = tempfile::Builder::new()
        .prefix("work.")
        .tempdir_in(&state)?;
    let stamp = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        std::process::id()
    );
    let mut install = Install {
        home: target,
        packages: state.join("dotfiles"),
        prefix: state.join("env"),
        backup: state.join("backups").join(stamp),
        state,
        path,
        moved: Vec::new(),
        copied: Vec::new(),
        links_before: BTreeSet::new(),
        linking_started: false,
        applied: false,
    };
    if let Some(stow) = &tools.private_stow {
        install.path = prepend_path(&[stow.parent().unwrap()], &install.path)?;
    }
    if !tools.missing.is_empty() {
        let platform = match std::env::consts::ARCH {
            "x86_64" => "linux-64",
            "aarch64" => "linux-aarch64",
            _ => bail!("Automatic tool installation supports Linux x86_64 and ARM64"),
        };
        let micromamba = install.state.join("micromamba");
        if !micromamba.is_file() {
            println!("\nDownloading micromamba for a private tools environment");
            let archive = work.path().join("micromamba.tar.bz2");
            install.fetch(
                &format!("https://micro.mamba.pm/api/micromamba/{platform}/latest"),
                &archive,
            )?;
            run(install
                .command("tar")
                .arg("-xjf")
                .arg(archive)
                .arg("-C")
                .arg(work.path())
                .arg("bin/micromamba"))?;
            fs::copy(work.path().join("bin/micromamba"), &micromamba)?;
            fs::set_permissions(&micromamba, fs::Permissions::from_mode(0o700))?;
        }
        println!("\nInstalling missing tools: {}", tools.missing.join(" "));
        let action = if install.prefix.join("conda-meta").is_dir() {
            "install"
        } else {
            "create"
        };
        run(install
            .command(&micromamba)
            .args(["--no-rc", action, "-y", "-r"])
            .arg(install.state.join("mamba"))
            .arg("-p")
            .arg(&install.prefix)
            .args([
                "--override-channels",
                "-c",
                "conda-forge",
                "--strict-channel-priority",
            ])
            .args(&tools.missing))?;
    }
    let staged_tools = work.path().join("cluster-tools");
    let previous_tools = install.packages.join("cluster-tools");
    fs::create_dir_all(&staged_tools)?;
    if previous_tools.is_dir() {
        run(install
            .command("cp")
            .args(["-a", "--"])
            .arg(previous_tools.join("."))
            .arg(&staged_tools))?;
    }
    let tool_bin = staged_tools.join(".local/bin");
    fs::create_dir_all(&tool_bin)?;
    for command in &tools.wrappers {
        wrapper(
            &tool_bin,
            command,
            &install.prefix.join("bin").join(command),
        )?;
    }
    if !tools.compiler {
        let compiler = fs::read_dir(install.prefix.join("bin"))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .is_some_and(|v| v.to_string_lossy().ends_with("-conda-linux-gnu-cc"))
            })
            .context("Installation did not provide a C compiler")?;
        wrapper(&tool_bin, "cc", &compiler)?;
    }
    install.path = prepend_path(&[&tool_bin, &install.prefix.join("bin")], &install.path)?;
    if !tools.old_tmux
        && let Ok(result) = install
            .command("tmux")
            .args(["-L", "default", "list-sessions"])
            .env("LC_ALL", "C")
            .output()
    {
        tools.old_tmux = old_tmux(&format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        ));
        if tools.old_tmux {
            print_old_tmux();
        }
    }
    if tools.need_stow {
        println!("\nInstalling GNU Stow under your home directory");
        let archive = work.path().join("stow.tar.gz");
        install.fetch("https://ftp.gnu.org/gnu/stow/stow-2.4.1.tar.gz", &archive)?;
        run(install
            .command("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(work.path()))?;
        let build = work.path().join("stow-2.4.1");
        let log = work.path().join("stow-build.log");
        let build_stow = || -> Result<()> {
            let log_file = fs::File::create(&log)?;
            for (name, arguments) in [
                (
                    "./configure",
                    vec![format!("--prefix={}", install.state.join("stow").display())],
                ),
                ("make", vec![]),
                ("make", vec!["install".into()]),
            ] {
                run(install
                    .command(name)
                    .args(arguments)
                    .current_dir(&build)
                    .stdout(Stdio::from(log_file.try_clone()?))
                    .stderr(Stdio::from(log_file.try_clone()?)))?;
            }
            Ok(())
        };
        if let Err(error) = build_stow() {
            eprint!("{}", fs::read_to_string(&log).unwrap_or_default());
            bail!("GNU Stow installation failed: {error:#}");
        }
        install.path = prepend_path(&[&install.state.join("stow/bin")], &install.path)?;
    }
    let private_stow = install.state.join("stow/bin/stow");
    if private_stow.is_file() {
        wrapper(&tool_bin, "stow", &private_stow)?;
    }
    println!("\nApplying the Neovim and tmux configurations with Stow");
    if source != install.packages {
        for package in ["nvim", "tmux", "cluster"] {
            install.replace_package(&source.join(package), package)?;
        }
    }
    install.replace_package(&staged_tools, "cluster-tools")?;
    let native = install
        .packages
        .join("cluster/.local/share/cluster/bin/cluster-native");
    if !native.is_file() {
        fs::create_dir_all(native.parent().unwrap())?;
        fs::copy(std::env::current_exe()?, &native)?;
        fs::set_permissions(&native, fs::Permissions::from_mode(0o700))?;
    }
    for relative in [
        ".config/nvim",
        ".tmux.conf",
        ".tmux.conf.custom",
        ".config/tmux/tmux.conf",
        ".local/share/cluster",
    ] {
        install.backup_path(Path::new(relative))?;
    }
    for directory in [
        install.packages.join("cluster-tools/.local/bin"),
        install.packages.join("cluster/.local/bin"),
    ] {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                install.backup_path(&Path::new(".local/bin").join(entry.file_name()))?;
            }
        }
    }
    fs::create_dir_all(install.home.join(".config"))?;
    fs::create_dir_all(install.home.join(".local/bin"))?;
    // Avoid the staging directory after it is renamed into the snapshot.
    install.path = prepend_path(
        &[
            &install.packages.join("cluster-tools/.local/bin"),
            &install.prefix.join("bin"),
        ],
        &original_path,
    )?;
    if private_stow.is_file() {
        install.path = prepend_path(&[private_stow.parent().unwrap()], &install.path)?;
    }
    install.links_before = install.owned_links()?;
    for flags in [["-n", "-v"], ["-R", "-v"]] {
        if flags[0] == "-R" {
            install.linking_started = true;
        }
        run(install
            .command("stow")
            .args(flags)
            .arg("-d")
            .arg(&install.packages)
            .arg("-t")
            .arg(&install.home)
            .args(["nvim", "tmux", "cluster", "cluster-tools"]))?;
    }
    install.applied = true;
    install.path = prepend_path(&[&install.home.join(".local/bin")], &install.path)?;
    let terminal = source.join("terminal.info");
    if fs::metadata(&terminal).is_ok_and(|m| m.len() > 0)
        && available("tic", &install.path)
        && run(install
            .command("tic")
            .arg("-x")
            .arg("-o")
            .arg(install.home.join(".terminfo"))
            .arg(terminal))
        .is_err()
    {
        eprintln!("Could not install terminal definitions; use TERM=xterm-256color if needed.");
    }
    if !args.skip_plugins {
        println!("\nInstalling the tmux plugins declared in your config");
        let plugins = install.home.join(".tmux/plugins");
        fs::create_dir_all(&plugins)?;
        let pattern =
            Regex::new(r#"^\s*set(?:-option)?\s+-g\s+@plugin\s+['"]?([^'"\s]+)"#).unwrap();
        let valid = Regex::new(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").unwrap();
        for line in fs::read_to_string(install.packages.join("tmux/.tmux.conf"))?.lines() {
            if let Some(caps) = pattern.captures(line) {
                let plugin = &caps[1];
                ensure!(valid.is_match(plugin), "Unsupported tmux plugin: {plugin}");
                let destination = plugins.join(plugin.split('/').nth(1).unwrap());
                if !destination.is_dir() {
                    let temporary = work.path().join("plugin");
                    run(install
                        .command("git")
                        .args([
                            "clone",
                            "--depth",
                            "1",
                            &format!("https://github.com/{plugin}.git"),
                        ])
                        .arg(&temporary)
                        .env("GIT_TERMINAL_PROMPT", "0"))?;
                    fs::rename(temporary, destination)?;
                }
            }
        }
    }
    for rc in [".bashrc", ".profile", ".zshrc"] {
        install.startup(rc)?;
    }
    for rc in [".bash_profile", ".bash_login"] {
        if install.home.join(rc).is_file() {
            install.startup(rc)?;
            break;
        }
    }
    println!("\nReady. Reconnect, then run tmux or nvim.");
    if tools.old_tmux {
        println!(
            "An older tmux server is still running. Start a separate server with:\n  tmux -L cluster new-session -A -s main"
        );
    }
    println!(
        "For this shell: export PATH=\"$HOME/.local/bin:$PATH\"\nNeovim installs plugins on first launch; Copilot needs its own login."
    );
    if install.backup.is_dir() {
        println!("Backups: {}", install.backup.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_distribution_version_suffixes() {
        assert!(version("tmux 3.7c") >= version("3.2"));
        assert!(version("NVIM v0.12.5") >= version("0.12.0"));
        assert!(version("v0.11.5") < version("0.12.0"));
        assert_eq!(version("unknown"), None);
    }
}
