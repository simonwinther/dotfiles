use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn script(path: &Path, body: &str) {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    fs::write(path, format!("#!{shell}\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn hidden_terminal_can_warm_and_is_shown_before_waiting_for_its_child() {
    let root = env::temp_dir().join(format!("picker-window-test-{}", std::process::id()));
    fs::create_dir_all(root.join("bin")).unwrap();
    // Model a compositor that withholds the terminal's first draw while hidden.
    // The child can report readiness only after a presentation dispatch.
    script(
        &root.join("bin/hyprctl"),
        r#"if [ "$1" = "-j" ]; then
    if [ -e "$XDG_RUNTIME_DIR/window" ]; then
        printf '[{"class":"%s.picker","address":"0x123abc"}]\n' "$PICKER_TEST_MODE"
    else
        printf '[]\n'
    fi
else
    printf '%s\n' "$*" >> "$XDG_RUNTIME_DIR/presentations"
    : > "$XDG_RUNTIME_DIR/$PICKER_TEST_MODE-picker-ready"
fi"#,
    );
    script(
        &root.join("bin/ghostty"),
        r#": > "$XDG_RUNTIME_DIR/window"
printf 'launch\n' >> "$XDG_RUNTIME_DIR/launches""#,
    );
    let path = env::join_paths(
        [root.join("bin")]
            .into_iter()
            .chain(env::split_paths(&env::var_os("PATH").unwrap())),
    )
    .unwrap();

    for mode in ["file", "pdf", "recent"] {
        let runtime = root.join(mode);
        fs::create_dir_all(&runtime).unwrap();
        let run = |action: &str| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_file-picker-native"));
            command
                .args([action, "--root"])
                .arg(&root)
                .env("PATH", &path)
                .env("XDG_RUNTIME_DIR", &runtime)
                .env("PICKER_TEST_MODE", mode);
            if mode != "file" {
                command.arg(format!("--{mode}"));
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{mode} {action}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run("--warm");
        let ready = runtime.join(format!("{mode}-picker-ready"));
        assert!(!ready.exists(), "warm-up must not require a visible draw");
        assert!(!runtime.join("presentations").exists());
        run("--window");
        assert!(ready.exists(), "opening must let the child initialize");
        run("--window");
        assert_eq!(
            fs::read_to_string(runtime.join("launches")).unwrap(),
            "launch\n"
        );
    }
    fs::remove_dir_all(root).unwrap();
}
