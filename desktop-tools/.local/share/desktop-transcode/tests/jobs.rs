use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

const BINARY: &str = env!("CARGO_BIN_EXE_desktop-transcode-native");
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    bash: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "transcode-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::create_dir(root.join("tools")).unwrap();
        let output = Command::new("bash")
            .args(["-c", "command -v bash"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let fixture = Self {
            root,
            bash: String::from_utf8(output.stdout).unwrap().trim().into(),
        };
        fs::write(fixture.path("input file.png"), "original input").unwrap();
        fixture.tool("wl-copy", "cat > \"$RUN/clipboard\"");
        fixture.tool("notify-send", "printf '%s\\n' \"$*\" >> \"$RUN/notices\"");
        fixture.encoder(
            r#"
out="${@: -1}"
printf '%s\0' "$@" > "$RUN/args"
printf partial > "$out"
printf '%s\n' "$$" > "$RUN/encoder-pid"
touch "$RUN/ready"
if [[ ${BLOCK:-0} == 1 ]]; then
  while [[ ! -f "$RUN/go" ]]; do sleep 0.02; done
fi
printf complete > "$out"
"#,
        );
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn tool(&self, name: &str, body: &str) {
        let path = self.path(&format!("tools/{name}"));
        fs::write(&path, format!("#!{}\nset -eu\n{body}\n", self.bash)).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn encoder(&self, body: &str) {
        self.tool("magick", body);
        self.tool("ffmpeg", body);
    }

    fn command(&self, kind: &str, format: &str, resolution: &str) -> Command {
        let mut command = Command::new(BINARY);
        command
            .args([kind])
            .arg(self.path("input file.png"))
            .args([format, resolution])
            .env("PATH", self.tools_path())
            .env("RUN", &self.root)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }

    fn tools_path(&self) -> String {
        format!(
            "{}:/usr/bin:/bin:{}",
            self.path("tools").display(),
            Path::new(&self.bash).parent().unwrap().display()
        )
    }

    fn start(&self) -> Running {
        Running(
            self.command("picture", "jpg", "low")
                .env("BLOCK", "1")
                .spawn()
                .unwrap(),
        )
    }

    fn release(&self) {
        fs::write(self.path("go"), "").unwrap();
    }

    fn assert_clean(&self) {
        for entry in fs::read_dir(&self.root).unwrap() {
            assert!(
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".desktop-transcode-")
            );
        }
        assert_eq!(
            fs::read(self.path("input file.png")).unwrap(),
            b"original input"
        );
    }

    fn assert_no_success(&self) {
        assert!(!self.path("clipboard").exists());
        let notices = fs::read_to_string(self.path("notices")).unwrap_or_default();
        assert!(!notices.contains("Transcoded to"));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Running(Child);

impl Running {
    fn signal(&self, number: i32) {
        assert_eq!(unsafe { libc::kill(self.0.id() as i32, number) }, 0);
    }

    fn finish(&mut self) -> Output {
        wait_until(|| self.0.try_wait().unwrap().is_some());
        // Keep the guard armed until we have reaped the child and read errors.
        let mut stderr = String::new();
        use std::io::Read;
        self.0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        Output {
            status: self.0.wait().unwrap(),
            stdout: vec![],
            stderr: stderr.into_bytes(),
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGTERM);
            }
            let _ = self.0.wait();
        }
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "Timed out waiting for transcode job"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_failed(output: &Output) {
    assert!(!output.status.success(), "Job unexpectedly succeeded");
}

#[test]
fn output_is_invisible_until_complete_then_copied_and_notified() {
    let fixture = Fixture::new();
    let mut job = fixture.start();
    wait_until(|| fixture.path("ready").exists());
    assert!(!fixture.path("input file-low.jpg").exists());
    fixture.assert_no_success();
    fixture.release();
    let output = job.finish();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.path("input file-low.jpg")).unwrap(),
        b"complete"
    );
    assert_eq!(
        fs::read_to_string(fixture.path("clipboard")).unwrap(),
        format!("file://{}\n", fixture.path("input file-low.jpg").display())
    );
    assert!(
        fs::read_to_string(fixture.path("notices"))
            .unwrap()
            .contains("Transcoded to low jpg")
    );
    fixture.assert_clean();
}

#[test]
fn failed_empty_and_missing_encoder_outputs_leave_no_partial_files() {
    for body in [
        "printf partial > \"${@: -1}\"; touch \"${@: -1}.delegate\"; exit 9",
        "touch \"${@: -1}\"",
        "exit 0",
    ] {
        let fixture = Fixture::new();
        fixture.encoder(body);
        assert_failed(
            &fixture
                .command("picture", "png", "medium")
                .output()
                .unwrap(),
        );
        assert!(!fixture.path("input file-medium.png").exists());
        fixture.assert_no_success();
        fixture.assert_clean();
    }
}

#[test]
fn existing_files_directories_and_dangling_links_are_refused_before_encoding() {
    for kind in ["file", "directory", "link"] {
        let fixture = Fixture::new();
        let output = fixture.path("input file-low.jpg");
        match kind {
            "file" => fs::write(&output, "keep").unwrap(),
            "directory" => fs::create_dir(&output).unwrap(),
            _ => symlink(fixture.path("absent"), &output).unwrap(),
        }
        assert_failed(&fixture.command("picture", "jpg", "low").output().unwrap());
        assert!(!fixture.path("ready").exists());
        assert!(fs::symlink_metadata(&output).is_ok());
        if kind == "file" {
            assert_eq!(fs::read_to_string(output).unwrap(), "keep");
        }
        fixture.assert_no_success();
        fixture.assert_clean();
    }
}

#[test]
fn output_created_during_encoding_is_never_replaced_or_followed() {
    for kind in ["file", "directory", "link"] {
        let fixture = Fixture::new();
        let mut job = fixture.start();
        wait_until(|| fixture.path("ready").exists());
        let output = fixture.path("input file-low.jpg");
        match kind {
            "file" => fs::write(&output, "winner").unwrap(),
            "directory" => fs::create_dir(&output).unwrap(),
            _ => symlink(fixture.path("absent"), &output).unwrap(),
        }
        fixture.release();
        assert_failed(&job.finish());
        assert!(fs::symlink_metadata(&output).is_ok());
        assert!(!fixture.path("absent").exists());
        if kind == "file" {
            assert_eq!(fs::read_to_string(output).unwrap(), "winner");
        }
        fixture.assert_no_success();
        fixture.assert_clean();
    }
}

#[test]
fn concurrent_jobs_have_exactly_one_winner() {
    let fixture = Fixture::new();
    let mut jobs = Vec::new();
    for run in ["one", "two"] {
        fs::create_dir(fixture.path(run)).unwrap();
        let child = fixture
            .command("picture", "jpg", "low")
            .env("BLOCK", "1")
            .env("RUN", fixture.path(run))
            .spawn()
            .unwrap();
        jobs.push(Running(child));
    }
    wait_until(|| fixture.path("one/ready").exists() && fixture.path("two/ready").exists());
    assert!(!fixture.path("input file-low.jpg").exists());
    for run in ["one", "two"] {
        fs::write(fixture.path(&format!("{run}/go")), "").unwrap();
    }
    let outcomes: Vec<_> = jobs.iter_mut().map(Running::finish).collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    assert_ne!(
        fixture.path("one/clipboard").exists(),
        fixture.path("two/clipboard").exists()
    );
    assert_eq!(
        fs::read(fixture.path("input file-low.jpg")).unwrap(),
        b"complete"
    );
    fixture.assert_clean();
}

fn running(pid: &Path) -> bool {
    let pid = fs::read_to_string(pid).unwrap();
    let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid.trim())) else {
        return false;
    };
    !stat
        .rsplit_once(')')
        .unwrap()
        .1
        .trim_start()
        .starts_with('Z')
}

#[test]
fn cancellation_stops_stubborn_encoders_and_delegates_before_cleanup() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let fixture = Fixture::new();
        fixture.encoder(
            r#"
trap '' INT TERM HUP
printf partial > "${@: -1}"
printf '%s\n' "$$" > "$RUN/encoder-pid"
(
  printf '%s\n' "$BASHPID" > "$RUN/delegate-pid"
  while :; do sleep 0.02; done
) &
touch "$RUN/ready"
while :; do sleep 0.02; done
"#,
        );
        let mut job = fixture.start();
        wait_until(|| fixture.path("ready").exists() && fixture.path("delegate-pid").exists());
        job.signal(signal);
        let output = job.finish();
        assert_eq!(output.status.code(), Some(128 + signal));
        wait_until(|| {
            !running(&fixture.path("encoder-pid")) && !running(&fixture.path("delegate-pid"))
        });
        assert!(!fixture.path("input file-low.jpg").exists());
        fixture.assert_no_success();
        fixture.assert_clean();
    }
}

#[test]
fn codec_options_and_literal_input_names_are_preserved() {
    for (kind, format, resolution, expected) in [
        ("picture", "jpg", "high", "3160x>"),
        ("picture", "png", "medium", "png:compression-level=9"),
        ("video", "mp4", "4k", "libx265"),
        ("video", "mp4", "1080p", "libx264"),
        (
            "video",
            "gif",
            "720p",
            "fps=10,scale=-2:720:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse",
        ),
    ] {
        let fixture = Fixture::new();
        let output = fixture.command(kind, format, resolution).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let arguments = fs::read(fixture.path("args")).unwrap();
        let arguments: Vec<_> = arguments
            .split(|byte| *byte == 0)
            .map(|bytes| String::from_utf8_lossy(bytes))
            .collect();
        assert!(arguments.iter().any(|arg| arg == expected));
        assert!(
            arguments
                .iter()
                .any(|arg| arg.as_ref() == fixture.path("input file.png").to_str().unwrap())
        );
        if kind == "video" {
            assert!(arguments.iter().any(|arg| arg == "-nostdin"));
            assert!(arguments.iter().any(|arg| arg == "-n"));
        }
        fixture.assert_clean();
    }
    let fixture = Fixture::new();
    let alias = fixture.path("-alias $(touch unexpected).png");
    symlink(fixture.path("input file.png"), &alias).unwrap();
    let output = Command::new(BINARY)
        .current_dir(&fixture.root)
        .args([
            "picture",
            alias.file_name().unwrap().to_str().unwrap(),
            "jpg",
            "low",
        ])
        .env("PATH", fixture.tools_path())
        .env("RUN", &fixture.root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.path("-alias $(touch unexpected)-low.jpg").exists());
    assert!(!fixture.path("unexpected").exists());
    fixture.assert_clean();
}

#[test]
fn invalid_choices_do_not_start_encoders_or_create_files() {
    for (kind, format, resolution) in [
        ("picture", "mp4", "low"),
        ("video", "mp4", "low"),
        ("picture", "jpg", "../escape"),
        ("other", "jpg", "low"),
    ] {
        let fixture = Fixture::new();
        assert_failed(&fixture.command(kind, format, resolution).output().unwrap());
        assert!(!fixture.path("ready").exists());
        fixture.assert_no_success();
        fixture.assert_clean();
    }
}

#[test]
fn clipboard_failure_preserves_the_completed_output_and_reports_its_location() {
    let fixture = Fixture::new();
    fixture.tool("wl-copy", "exit 2");
    let output = fixture.command("picture", "jpg", "low").output().unwrap();
    assert_failed(&output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("but could not copy it"));
    assert_eq!(
        fs::read(fixture.path("input file-low.jpg")).unwrap(),
        b"complete"
    );
    fixture.assert_no_success();
    fixture.assert_clean();
}
