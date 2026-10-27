use std::{env, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
use tempfile::TempDir;

fn script(path: &Path, body: &str) {
    let shell = env::split_paths(&env::var_os("PATH").unwrap())
        .map(|directory| directory.join("sh"))
        .find(|path| path.is_file())
        .expect("sh must be available in PATH");
    fs::write(path, format!("#!{}\nset -eu\n{body}\n", shell.display())).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Desktop {
    temp: TempDir,
    path: std::ffi::OsString,
}

impl Desktop {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        script(
            &bin.join("hyprctl"),
            r#"if [ "$1" = "-j" ]; then
    if [ -e "$XDG_RUNTIME_DIR/window" ]; then
        cat "$XDG_RUNTIME_DIR/window"
    else
        printf '[]\n'
    fi
else
    printf '%s\n' "$*" >> "$XDG_RUNTIME_DIR/dispatches"
fi"#,
        );
        script(
            &bin.join("ghostty"),
            r#"case "$*" in
    *--initial-window=false*)
        printf 'host\n' >> "$XDG_RUNTIME_DIR/hosts"
        : > "$XDG_RUNTIME_DIR/host"
        ;;
    +new-window*)
        printf 'fallback\n' >> "$XDG_RUNTIME_DIR/fallbacks"
        printf '[{"class":"file.picker","title":"file-picker","address":"0x123abc"}]\n' > "$XDG_RUNTIME_DIR/window"
        ;;
    *) exit 1 ;;
esac"#,
        );
        script(
            &bin.join("gdbus"),
            r#"case "$*" in
    *NameHasOwner*)
        if [ -e "$XDG_RUNTIME_DIR/host" ]; then printf '(true,)\n'; else printf '(false,)\n'; fi
        ;;
    *org.gtk.Actions.Activate*)
        if [ "${PICKER_TEST_FAIL_DIRECT:-}" = 1 ]; then exit 1; fi
        printf '%s\n' "$@" >> "$XDG_RUNTIME_DIR/requests"
        case "$*" in
            *--title=pdf-picker*) title=pdf-picker ;;
            *--title=recent-picker*) title=recent-picker ;;
            *) title=file-picker ;;
        esac
        printf '[{"class":"file.picker","title":"%s","address":"0x123abc"}]\n' "$title" > "$XDG_RUNTIME_DIR/window"
        printf '%s\n' "$title" >> "$XDG_RUNTIME_DIR/opened"
        ;;
    *) exit 1 ;;
esac"#,
        );
        let path = env::join_paths(
            [bin]
                .into_iter()
                .chain(env::split_paths(&env::var_os("PATH").unwrap())),
        )
        .unwrap();
        Self { temp, path }
    }

    fn run(&self, args: &[&str], fail_direct: bool) {
        let output = Command::new(env!("CARGO_BIN_EXE_file-picker-native"))
            .args(args)
            .arg("--root")
            .arg(self.temp.path().join("$literal ' root"))
            .env("PATH", &self.path)
            .env("XDG_RUNTIME_DIR", self.temp.path())
            .env(
                "FILE_PICKER_INDEX",
                self.temp.path().join("private cache.sqlite3"),
            )
            .env(
                "PICKER_TEST_FAIL_DIRECT",
                if fail_direct { "1" } else { "" },
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.temp.path().join(name)).unwrap()
    }
}

#[test]
fn shared_host_warms_without_windows_and_opens_each_mode_on_demand() {
    let desktop = Desktop::new();
    desktop.run(&["--prepare-terminal"], false);
    desktop.run(&["--prepare-terminal"], false);
    assert!(!desktop.temp.path().join("window").exists());
    assert!(!desktop.temp.path().join("dispatches").exists());
    assert_eq!(desktop.read("hosts"), "host\n");

    for mode in ["file", "pdf", "recent"] {
        let mut args = vec!["--window"];
        if mode == "pdf" {
            args.push("--pdf");
        } else if mode == "recent" {
            args.push("--recent");
        }
        desktop.run(&args, false);
        desktop.run(&args, false);
    }
    assert_eq!(desktop.read("hosts"), "host\n");
    assert_eq!(
        desktop.read("opened"),
        "file-picker\npdf-picker\nrecent-picker\n"
    );
    let requests = desktop.read("requests");
    assert!(requests.contains("FILE_PICKER_INDEX="));
    assert!(requests.contains("private cache.sqlite3"));
    assert!(requests.contains("$literal ' root"));
    let dispatches = desktop.read("dispatches");
    assert!(dispatches.contains("window.close"));
    assert!(dispatches.contains("window.center"));
    assert!(!dispatches.contains("special:"));
}

#[test]
fn terminal_cli_is_used_when_the_direct_dbus_action_fails() {
    let desktop = Desktop::new();
    desktop.run(&["--window"], true);
    assert_eq!(desktop.read("fallbacks"), "fallback\n");
    assert_eq!(desktop.read("hosts"), "host\n");
}
