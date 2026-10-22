use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{PermissionsExt, symlink},
    },
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const BINARY: &str = env!("CARGO_BIN_EXE_dictation-volume-duck-native");
const FAKE_AUDIO: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$DUCK_TEST_ROOT/calls"
if [ -f "$DUCK_TEST_ROOT/hang" ]; then exec /bin/sleep 30; fi
case "$1" in
  inspect)
    read -r serial < "$DUCK_TEST_ROOT/serial"
    printf 'id 42, type PipeWire:Interface:Node\n  * object.serial = "%s"\n' "$serial"
    ;;
  get-default-sink) printf 'original.sink\n' ;;
  get-volume)
    read -r volume < "$DUCK_TEST_ROOT/volume"
    read -r muted < "$DUCK_TEST_ROOT/muted"
    if [ "$muted" = 1 ]; then printf 'Volume: %s [MUTED]\n' "$volume"; else printf 'Volume: %s\n' "$volume"; fi
    ;;
  get-sink-volume)
    read -r volume < "$DUCK_TEST_ROOT/volume"
    printf 'Volume: front-left: 65536 / %s%% / 0.00 dB\n' "$volume"
    ;;
  get-sink-mute)
    read -r muted < "$DUCK_TEST_ROOT/muted"
    if [ "$muted" = 1 ]; then printf 'Mute: yes\n'; else printf 'Mute: no\n'; fi
    ;;
  set-volume|set-sink-volume)
    if [ -f "$DUCK_TEST_ROOT/fail" ]; then printf 'Audio unavailable\n' >&2; exit 1; fi
    if [ -f "$DUCK_TEST_ROOT/fail-once" ]; then /bin/rm "$DUCK_TEST_ROOT/fail-once"; exit 1; fi
    printf '%s\n' "${3%\%}" > "$DUCK_TEST_ROOT/volume"
    ;;
  set-mute|set-sink-mute)
    if [ -f "$DUCK_TEST_ROOT/fail-mute" ]; then exit 1; fi
    printf '%s\n' "$3" > "$DUCK_TEST_ROOT/muted"
    ;;
  *) exit 1 ;;
esac
"#;

struct Desktop {
    dir: TempDir,
}

impl Desktop {
    fn new(backend: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for name in ["tools", "runtime", "home"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let audio = dir.path().join("tools").join(backend);
        fs::write(&audio, FAKE_AUDIO).unwrap();
        fs::set_permissions(audio, fs::Permissions::from_mode(0o755)).unwrap();
        let result = Self { dir };
        result.write(
            "volume",
            if backend == "pactl" {
                "165\n"
            } else {
                "1.65\n"
            },
        );
        result.write("muted", "1\n");
        result.write("serial", "original-42\n");
        result
    }

    fn path(&self, path: &str) -> std::path::PathBuf {
        self.dir.path().join(path)
    }
    fn write(&self, path: &str, text: &str) {
        fs::write(self.path(path), text).unwrap();
    }
    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.path(path)).unwrap()
    }
    fn state(&self) -> std::path::PathBuf {
        self.path("runtime/dictation-volume-duck/state.env")
    }
    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .env_clear()
            .env("HOME", self.path("home"))
            .env("PATH", self.path("tools"))
            .env("XDG_RUNTIME_DIR", self.path("runtime"))
            .env("DUCK_TEST_ROOT", self.dir.path())
            .env("VOLUME_DUCK_FADE_IN_MS", "0")
            .env("VOLUME_DUCK_FADE_OUT_MS", "0");
        command
    }

    fn call(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn ok(&self, args: &[&str]) -> Output {
        let out = self.call(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }

    fn frame(&self, frame: &[u8]) -> Value {
        let mut child = self
            .command()
            .args(["--native-host", "chrome-extension://test/"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(frame).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let length = u32::from_ne_bytes(output.stdout[..4].try_into().unwrap()) as usize;
        assert_eq!(output.stdout.len(), length + 4, "Exactly one framed reply");
        serde_json::from_slice(&output.stdout[4..]).unwrap()
    }

    fn message(&self, message: Value) -> Value {
        let bytes = serde_json::to_vec(&message).unwrap();
        let mut frame = (bytes.len() as u32).to_ne_bytes().to_vec();
        frame.extend(bytes);
        self.frame(&frame)
    }

    fn seed(&self, text: &str) {
        self.ok(&["status"]);
        fs::write(self.state(), text).unwrap();
    }
}

#[test]
fn clients_share_one_duck_and_restore_original_volume_and_mute() {
    let desktop = Desktop::new("wpctl");
    assert_eq!(desktop.ok(&[]).stdout, b"idle\n");
    desktop.ok(&["start", "tab-1"]);
    assert_eq!(desktop.read("volume"), "0.4950\n");
    desktop.ok(&["start", "tab-1", "0.1"]);
    desktop.ok(&["start", "tab-2"]);
    assert_eq!(desktop.read("volume"), "0.4950\n");
    desktop.ok(&["stop", "unknown"]);
    desktop.ok(&["stop", "tab-1"]);
    assert!(desktop.state().exists());
    assert_eq!(
        desktop.ok(&["status"]).stdout,
        b"ducked clients=tab-2 original_backend=wpctl original_volume=1.65 original_muted=1\n"
    );
    desktop.write("muted", "0\n");
    desktop.ok(&["stop", "tab-2"]);
    assert_eq!(desktop.read("volume"), "1.6500\n");
    assert_eq!(desktop.read("muted"), "1\n");
    assert!(!desktop.state().exists());
}

#[test]
fn pactl_fallback_preserves_mute_and_scales_percentages() {
    let desktop = Desktop::new("pactl");
    desktop.ok(&["start", "browser", "0.2"]);
    assert_eq!(desktop.read("volume"), "33.000\n");
    assert!(
        fs::read_to_string(desktop.state())
            .unwrap()
            .contains("ORIGINAL_SINK=original.sink")
    );
    desktop.ok(&["stop", "browser"]);
    assert_eq!(desktop.read("volume"), "165.000\n");
    assert_eq!(desktop.read("muted"), "1\n");
}

#[test]
fn native_dictation_state_keeps_its_sink_and_last_client() {
    let desktop = Desktop::new("wpctl");
    desktop.seed("CLIENTS=dictate\nORIGINAL_BACKEND=wpctl\nORIGINAL_MUTED=0\nORIGINAL_SINK=42\nORIGINAL_SINK_IDENTITY=original-42\nORIGINAL_VOLUME=0.8\n");
    desktop.write("volume", "0.24\n");
    desktop.ok(&["start", "chatgpt-dictate-tab-123"]);
    let state = fs::read_to_string(desktop.state()).unwrap();
    assert!(state.contains("CLIENTS='dictate chatgpt-dictate-tab-123'"));
    assert!(state.contains("ORIGINAL_SINK_IDENTITY=original-42"));
    desktop.ok(&["stop", "dictate"]);
    assert_eq!(desktop.read("volume"), "0.24\n");
    desktop.ok(&["stop", "chatgpt-dictate-tab-123"]);
    assert_eq!(desktop.read("volume"), "0.8000\n");
    assert_eq!(desktop.read("muted"), "0\n");
}

#[test]
fn legacy_bash_state_migrates_without_reducing_volume_again() {
    let desktop = Desktop::new("wpctl");
    desktop.seed("ORIGINAL_BACKEND=wpctl\nORIGINAL_VOLUME=0.75\nORIGINAL_MUTED=1\nCLIENTS=old-tab\\ second-tab\n");
    desktop.write("volume", "0.225\n");
    desktop.ok(&["start", "new-tab"]);
    assert_eq!(desktop.read("volume"), "0.225\n");
    let state = fs::read_to_string(desktop.state()).unwrap();
    assert!(state.contains("ORIGINAL_SINK=42"));
    assert!(state.contains("CLIENTS='old-tab second-tab new-tab'"));
    desktop.ok(&["reset"]);
    assert_eq!(desktop.read("volume"), "0.7500\n");
}

#[test]
fn shell_text_in_state_is_never_executed() {
    let desktop = Desktop::new("wpctl");
    let payload = format!(
        "ORIGINAL_BACKEND=wpctl\nORIGINAL_VOLUME=$(touch {})\nORIGINAL_MUTED=0\nCLIENTS=tab\n",
        desktop.path("injected").display()
    );
    desktop.seed(&payload);
    let result = desktop.call(&["reset"]);
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(desktop.state()).unwrap(), payload);
    assert!(!desktop.path("injected").exists());
    assert!(!desktop.path("calls").exists());
}

#[test]
fn concurrent_browser_requests_do_not_lose_clients() {
    let desktop = Desktop::new("wpctl");
    let mut starts: Vec<Child> = (0..12)
        .map(|i| {
            desktop
                .command()
                .args(["start", &format!("tab-{i}")])
                .spawn()
                .unwrap()
        })
        .collect();
    for child in &mut starts {
        assert!(child.wait().unwrap().success());
    }
    let state = fs::read_to_string(desktop.state()).unwrap();
    let clients =
        shell_words::split(state.lines().find(|s| s.starts_with("CLIENTS=")).unwrap()).unwrap();
    assert_eq!(
        clients[0]
            .strip_prefix("CLIENTS=")
            .unwrap()
            .split_whitespace()
            .count(),
        12
    );
    assert_eq!(desktop.read("volume"), "0.4950\n");
    let mut stops: Vec<Child> = (0..12)
        .map(|i| {
            desktop
                .command()
                .args(["stop", &format!("tab-{i}")])
                .spawn()
                .unwrap()
        })
        .collect();
    for child in &mut stops {
        assert!(child.wait().unwrap().success());
    }
    assert_eq!(desktop.read("volume"), "1.6500\n");
    assert!(!desktop.state().exists());
}

#[test]
fn failed_start_rolls_back_or_keeps_a_recoverable_original() {
    let desktop = Desktop::new("wpctl");
    desktop.write("fail-once", "");
    assert!(!desktop.call(&["start", "tab"]).status.success());
    assert_eq!(desktop.read("volume"), "1.6500\n");
    assert!(!desktop.state().exists());
    desktop.write("fail", "");
    assert!(!desktop.call(&["start", "tab"]).status.success());
    assert!(
        fs::read_to_string(desktop.state())
            .unwrap()
            .contains("CLIENTS=''")
    );
    fs::remove_file(desktop.path("fail")).unwrap();
    desktop.ok(&["start", "tab"]);
    assert_eq!(desktop.read("volume"), "0.4950\n");
    desktop.ok(&["stop", "tab"]);
    assert_eq!(desktop.read("volume"), "1.6500\n");
}

#[test]
fn failed_restore_and_reset_keep_state_until_success() {
    let desktop = Desktop::new("wpctl");
    desktop.ok(&["start", "tab"]);
    desktop.write("fail", "");
    assert!(!desktop.call(&["stop", "tab"]).status.success());
    assert!(desktop.state().exists());
    assert!(!desktop.call(&["reset"]).status.success());
    assert!(desktop.state().exists());
    fs::remove_file(desktop.path("fail")).unwrap();
    desktop.write("fail-mute", "");
    assert!(!desktop.call(&["reset"]).status.success());
    assert!(desktop.state().exists());
    fs::remove_file(desktop.path("fail-mute")).unwrap();
    desktop.ok(&["reset"]);
    assert_eq!(desktop.read("volume"), "1.6500\n");
    assert!(!desktop.state().exists());
}

#[test]
fn changed_default_does_not_redirect_restoration_and_reused_ids_are_rejected() {
    let desktop = Desktop::new("wpctl");
    desktop.ok(&["start", "tab"]);
    desktop.write("calls", "");
    desktop.write("serial", "replacement-42\n");
    assert!(!desktop.call(&["stop", "tab"]).status.success());
    assert!(!desktop.read("calls").contains("set-volume"));
    assert!(desktop.state().exists());
    desktop.write("serial", "original-42\n");
    desktop.ok(&["reset"]);
    let calls = desktop.read("calls");
    assert!(calls.contains("inspect 42\n"));
    assert!(!calls.contains("@DEFAULT_AUDIO_SINK@"));
}

#[test]
fn fades_use_configured_steps_and_end_at_exact_target() {
    let desktop = Desktop::new("wpctl");
    let began = Instant::now();
    let result = desktop
        .command()
        .env("VOLUME_DUCK_FADE_IN_MS", "180")
        .env("VOLUME_DUCK_FADE_STEPS", "3")
        .args(["start", "tab", "0.5"])
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(began.elapsed() >= Duration::from_millis(110));
    let calls = desktop.read("calls");
    let sets = calls
        .lines()
        .filter(|s| s.starts_with("set-volume"))
        .collect::<Vec<_>>();
    assert_eq!(
        sets,
        [
            "set-volume 42 1.3750",
            "set-volume 42 1.1000",
            "set-volume 42 0.8250"
        ]
    );
    assert_eq!(desktop.read("volume"), "0.8250\n");
}

#[test]
fn native_messages_preserve_reply_shape_and_exact_framing() {
    let desktop = Desktop::new("wpctl");
    assert_eq!(
        desktop.message(json!({"action":" STATUS "})),
        json!({"ok":true,"status":0,"stdout":"idle"})
    );
    assert_eq!(
        desktop.message(json!({"action":"start","client":"tab-1","factor":0.2})),
        json!({"ok":true,"status":0})
    );
    assert_eq!(desktop.read("volume"), "0.3300\n");
    let status = desktop.message(json!({"action":"status"}));
    assert!(status["stdout"].as_str().unwrap().contains("clients=tab-1"));
    assert_eq!(
        desktop.message(json!({"action":"stop","client":"tab-1"})),
        json!({"ok":true,"status":0})
    );
    assert_eq!(desktop.read("volume"), "1.6500\n");
    let invalid = desktop.message(json!({"action":"launch é🦀"}));
    assert_eq!(
        invalid,
        json!({"ok":false,"error":"Unsupported action: launch é🦀"})
    );
}

#[test]
fn native_input_errors_are_framed_without_audio_changes() {
    let desktop = Desktop::new("wpctl");
    let mut partial = 12u32.to_ne_bytes().to_vec();
    partial.extend(b"{}");
    let mut invalid_json = 1u32.to_ne_bytes().to_vec();
    invalid_json.push(0xff);
    for frame in [
        vec![],
        vec![1, 0],
        65_537u32.to_ne_bytes().to_vec(),
        partial,
        invalid_json,
    ] {
        let result = desktop.frame(&frame);
        assert_eq!(result["ok"], false);
        assert!(result["error"].is_string());
    }
    assert_eq!(desktop.message(json!([]))["ok"], false);
    assert!(!desktop.path("calls").exists());
}

#[test]
fn defaults_sanitize_clients_and_reject_invalid_factors() {
    let desktop = Desktop::new("wpctl");
    let result = desktop.message(json!({"action":"start","client":"tab /;$(id)"}));
    assert_eq!(result["ok"], true);
    assert!(
        fs::read_to_string(desktop.state())
            .unwrap()
            .contains("CLIENTS=tab_____id_")
    );
    desktop.ok(&["reset"]);
    assert_eq!(desktop.message(json!({"action":"start"}))["ok"], true);
    assert!(
        fs::read_to_string(desktop.state())
            .unwrap()
            .contains("CLIENTS=chatgpt-dictate")
    );
    desktop.ok(&["reset"]);
    for factor in ["NaN", "inf", "bad"] {
        assert_eq!(
            desktop.call(&["start", "tab", factor]).status.code(),
            Some(2)
        );
        assert!(!desktop.state().exists());
    }
}

#[test]
fn backend_and_lock_timeouts_return_framed_errors() {
    let desktop = Desktop::new("wpctl");
    desktop.write("hang", "");
    let began = Instant::now();
    let result = desktop.message(json!({"action":"start"}));
    assert_eq!(result["ok"], false);
    assert!(result["stderr"].as_str().unwrap().contains("timed out"));
    assert!(began.elapsed() < Duration::from_secs(3));
    assert!(!desktop.state().exists());
    fs::remove_file(desktop.path("hang")).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(desktop.path("runtime/dictation-volume-duck/lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let began = Instant::now();
    let result = desktop.message(json!({"action":"status"}));
    assert_eq!(result["ok"], false);
    assert!(result["stderr"].as_str().unwrap().contains("busy"));
    assert!(began.elapsed() < Duration::from_secs(4));
}

#[test]
fn unsafe_state_and_runtime_links_are_refused() {
    let desktop = Desktop::new("wpctl");
    desktop.ok(&["status"]);
    desktop.write("outside", "unchanged");
    symlink(desktop.path("outside"), desktop.state()).unwrap();
    assert!(!desktop.call(&["start"]).status.success());
    assert_eq!(desktop.read("outside"), "unchanged");
    fs::remove_file(desktop.state()).unwrap();
    fs::remove_dir_all(desktop.path("runtime/dictation-volume-duck")).unwrap();
    symlink(
        desktop.path("home"),
        desktop.path("runtime/dictation-volume-duck"),
    )
    .unwrap();
    assert!(!desktop.call(&["status"]).status.success());
    assert!(!Path::new(&desktop.path("home/lock")).exists());
}

#[test]
fn missing_backend_and_bad_audio_output_leave_no_state() {
    let desktop = Desktop::new("wpctl");
    desktop.write("volume", "NaN\n");
    assert!(!desktop.call(&["start"]).status.success());
    assert!(!desktop.state().exists());
    fs::remove_file(desktop.path("tools/wpctl")).unwrap();
    let result = desktop.message(json!({"action":"start"}));
    assert_eq!(result["ok"], false);
    assert!(result["stderr"].as_str().unwrap().contains("Install wpctl"));
}
