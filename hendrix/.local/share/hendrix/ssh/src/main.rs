//! Native `ssh` wrapper for the Hendrix alias.
//!
//! Every other use of ssh, including Git's, is handed to OpenSSH at once
//! instead of paying for a shell and a Python interpreter first. For Hendrix it
//! follows `ssh()` in hendrix.py: a reachable gateway gets the saved-password
//! askpass login directly, and only starting the KU VPN is delegated to
//! hendrix.py, which also keeps the credentials, setup and repair workflows.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const HOSTS: [&[u8]; 3] = [b"hendrix", b"hendrixgate", b"hendrixgate.unicph.domain"];
const USER: &[u8] = b"zlp616";
/// ssh options that take an argument, and those that only inspect or control.
const ARGUMENT_OPTIONS: &[u8] = b"BbcDEeFIiJLlmOopQRSWw";
const INSPECT_OPTIONS: &[u8] = b"GVQO";
const GATEWAY: (&str, u16) = ("hendrixgate.unicph.domain", 22);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// Bounds name resolution too, since the short name requires KU DNS.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const SYSTEM_SSH: &str = "/run/current-system/sw/bin/ssh";
const LOGIN_OPTIONS: [&str; 4] = [
    "-o",
    "PreferredAuthentications=publickey,password,keyboard-interactive",
    "-o",
    "NumberOfPasswordPrompts=1",
];

/// The locations hendrix.py and its shell wrapper use, from the environment.
struct Paths {
    ssh: OsString,
    password: PathBuf,
    script: PathBuf,
}

impl Paths {
    fn from_environment() -> Self {
        let ssh = env::var_os("DESKTOP_SSH_BIN").unwrap_or_else(|| {
            OsString::from(if Path::new(SYSTEM_SSH).exists() {
                SYSTEM_SSH
            } else {
                "/usr/bin/ssh"
            })
        });
        // Like os.environ.get in hendrix.py, an empty XDG_CONFIG_HOME is kept.
        let config = match env::var_os("XDG_CONFIG_HOME") {
            Some(base) => PathBuf::from(base),
            None => home().join(".config"),
        };
        // The wrapper's ${XDG_DATA_HOME:-$HOME/.local/share}.
        let data = match env::var_os("XDG_DATA_HOME") {
            Some(base) if !base.is_empty() => PathBuf::from(base),
            _ => {
                let mut base = env::var_os("HOME").unwrap_or_default();
                base.push("/.local/share");
                PathBuf::from(base)
            }
        };
        Paths {
            ssh,
            password: config.join("hendrix/ku-password"),
            script: data.join("hendrix/hendrix.py"),
        }
    }

    /// hendrix.py offers the askpass helper next to its own resolved file.
    fn askpass(&self) -> PathBuf {
        fs::canonicalize(&self.script)
            .unwrap_or_else(|_| self.script.clone())
            .with_file_name("askpass")
    }
}

/// Python's Path.home(): $HOME, where an empty value means "/", else the
/// password database.
#[allow(deprecated)]
fn home() -> PathBuf {
    match env::var_os("HOME") {
        Some(home) if home.is_empty() => PathBuf::from("/"),
        Some(home) => PathBuf::from(home),
        None => env::home_dir().unwrap_or_default(),
    }
}

/// The Hendrix destination, or None when ssh is used for anything else.
/// Mirrors `destination()` in hendrix.py; destinations.txt holds shared cases.
fn destination(args: &[OsString]) -> Option<&OsString> {
    let mut skip = false;
    let mut literal = false;
    let mut inspect_only = false;
    let mut user: &[u8] = b"";
    for (index, arg) in args.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        let bytes = arg.as_bytes();
        if !literal && bytes == b"--" {
            literal = true;
            continue;
        }
        if !literal && bytes.len() > 1 && bytes[0] == b'-' {
            for (offset, letter) in bytes.iter().enumerate().skip(1) {
                inspect_only |= INSPECT_OPTIONS.contains(letter);
                if ARGUMENT_OPTIONS.contains(letter) {
                    let mut value = &bytes[offset + 1..];
                    if value.is_empty() && index + 1 < args.len() {
                        value = args[index + 1].as_bytes();
                        skip = true;
                    }
                    if *letter == b'l' {
                        user = value;
                    }
                    break;
                }
            }
            continue;
        }
        if inspect_only {
            return None;
        }
        let (prefix, host) = match bytes.iter().rposition(|&byte| byte == b'@') {
            Some(at) => (&bytes[..at], &bytes[at + 1..]),
            None => (&bytes[..0], bytes),
        };
        let other_user = |name: &[u8]| !name.is_empty() && name != USER;
        if !HOSTS.contains(&host) || other_user(prefix) || other_user(user) {
            return None;
        }
        return Some(arg);
    }
    None
}

/// The configured ssh, run as given like os.execv does: a bare name must not
/// be searched on PATH, where it would find this wrapper again.
fn ssh_command(program: &OsStr) -> Command {
    let mut command = if program.as_bytes().contains(&b'/') {
        Command::new(program)
    } else {
        Command::new(Path::new(".").join(program))
    };
    command.arg0(program);
    command
}

/// The command that replaces this process, following `ssh()` in hendrix.py.
fn launch(args: &[OsString], paths: &Paths, reachable: impl FnOnce() -> bool) -> Command {
    let mut ssh = ssh_command(&paths.ssh);
    // Until setup completes, preserve the existing interactive SSH behaviour.
    if destination(args).is_none() || !paths.password.is_file() {
        ssh.args(args);
        return ssh;
    }
    if !reachable() {
        // Starting the VPN needs the saved credentials, sudo and polling.
        // hendrix.py does that rarely needed part and probes again first.
        let mut python = Command::new("python3");
        python.arg(&paths.script).arg("ssh").args(args);
        return python;
    }
    ssh.args(LOGIN_OPTIONS)
        .args(args)
        .env("SSH_ASKPASS", paths.askpass())
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("HENDRIX_PASSWORD_FILE", &paths.password);
    ssh
}

/// One TCP connection to the gateway, as `probe` under hendrix.py's
/// `reachable()`. A stalled lookup is abandoned when the timeout expires; exec
/// then replaces the whole process, threads included.
fn reachable(address: (&'static str, u16), timeout: Duration) -> bool {
    let (sender, receiver) = mpsc::channel();
    let probe = move || {
        let connected = address.to_socket_addrs().is_ok_and(|mut candidates| {
            candidates
                .any(|candidate| TcpStream::connect_timeout(&candidate, CONNECT_TIMEOUT).is_ok())
        });
        let _ = sender.send(connected);
    };
    thread::Builder::new().spawn(probe).is_ok() && receiver.recv_timeout(timeout) == Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    let paths = Paths::from_environment();
    let mut command = launch(&args, &paths, || reachable(GATEWAY, PROBE_TIMEOUT));
    let error = command.exec();
    eprintln!("{}: {error}", command.get_program().to_string_lossy());
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    const CASES: &str = include_str!("../destinations.txt");

    struct Fixture {
        root: PathBuf,
        paths: Paths,
    }

    impl Fixture {
        fn new(name: &str, password: bool) -> Self {
            let root = env::temp_dir().join(format!("hendrix-ssh-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("share")).unwrap();
            let paths = Paths {
                ssh: OsString::from("/fake/ssh"),
                password: root.join("ku-password"),
                script: root.join("share/hendrix.py"),
            };
            if password {
                fs::write(&paths.password, "fixture\n").unwrap();
            }
            Fixture { root, paths }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn arguments(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn command_line(command: &Command) -> Vec<&OsStr> {
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .collect()
    }

    fn environment(command: &Command) -> Vec<(&OsStr, Option<&OsStr>)> {
        let mut variables: Vec<_> = command.get_envs().collect();
        variables.sort();
        variables
    }

    #[test]
    fn destinations_match_the_shared_cases() {
        let cases = CASES
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'));
        let mut checked = 0;
        for line in cases {
            let mut words = line.split_whitespace();
            let expected = words.next().unwrap();
            let args: Vec<OsString> = words.map(OsString::from).collect();
            let actual = destination(&args).map(|arg| arg.to_str().unwrap());
            assert_eq!(actual, (expected != "-").then_some(expected), "{line}");
            checked += 1;
        }
        assert!(checked > 20);
    }

    #[test]
    fn other_hosts_and_inspection_run_plain_ssh() {
        let fixture = Fixture::new("plain", true);
        let calls: [&[&str]; 4] = [
            &["example.com", "true"],
            &["-G", "hendrix"],
            &["-O", "check", "hendrix"],
            &["other@hendrix"],
        ];
        for words in calls {
            let command = launch(&arguments(words), &fixture.paths, || {
                panic!("probed for {words:?}")
            });
            let mut expected = vec![OsStr::new("/fake/ssh")];
            expected.extend(words.iter().map(OsStr::new));
            assert_eq!(command_line(&command), expected);
            assert!(environment(&command).is_empty());
        }
    }

    #[test]
    fn hendrix_before_setup_runs_plain_ssh() {
        let fixture = Fixture::new("setup", false);
        let command = launch(&arguments(&["hendrix", "true"]), &fixture.paths, || {
            panic!("probed")
        });
        assert_eq!(command_line(&command), ["/fake/ssh", "hendrix", "true"]);
        assert!(environment(&command).is_empty());
    }

    #[test]
    fn reachable_hendrix_uses_the_saved_password_login() {
        let fixture = Fixture::new("login", true);
        fs::write(&fixture.paths.script, "").unwrap();
        let args = arguments(&["-p", "22", "zlp616@hendrix", "true"]);
        let command = launch(&args, &fixture.paths, || true);
        let mut expected = vec!["/fake/ssh"];
        expected.extend(LOGIN_OPTIONS);
        expected.extend(["-p", "22", "zlp616@hendrix", "true"]);
        assert_eq!(command_line(&command), expected);
        let askpass = fs::canonicalize(&fixture.root)
            .unwrap()
            .join("share/askpass");
        assert_eq!(
            environment(&command),
            [
                (
                    OsStr::new("HENDRIX_PASSWORD_FILE"),
                    Some(fixture.paths.password.as_os_str())
                ),
                (OsStr::new("SSH_ASKPASS"), Some(askpass.as_os_str())),
                (OsStr::new("SSH_ASKPASS_REQUIRE"), Some(OsStr::new("force"))),
            ]
        );
    }

    #[test]
    fn unreachable_hendrix_lets_python_start_the_vpn() {
        let fixture = Fixture::new("vpn", true);
        let command = launch(&arguments(&["hendrix"]), &fixture.paths, || false);
        let script = fixture.paths.script.as_os_str();
        assert_eq!(
            command_line(&command),
            [
                OsStr::new("python3"),
                script,
                OsStr::new("ssh"),
                OsStr::new("hendrix")
            ]
        );
        assert!(environment(&command).is_empty());
    }

    #[test]
    fn bare_ssh_names_are_not_searched_on_path() {
        assert_eq!(ssh_command(OsStr::new("ssh")).get_program(), "./ssh");
        assert_eq!(
            ssh_command(OsStr::new("/bin/ssh")).get_program(),
            "/bin/ssh"
        );
    }

    #[test]
    fn probe_needs_a_listening_gateway() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(reachable(("127.0.0.1", port), PROBE_TIMEOUT));
        drop(listener);
        assert!(!reachable(("127.0.0.1", port), PROBE_TIMEOUT));
    }
}
