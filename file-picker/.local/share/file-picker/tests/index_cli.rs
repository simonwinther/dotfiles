use std::{
    env, fs,
    io::{BufRead, BufReader},
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn program(name: &str) -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("Test dependency missing from PATH: {name}"))
}

fn script(path: &Path, body: &str) {
    fs::write(
        path,
        format!("#!{}\nset -eu\n{body}\n", program("sh").display()),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fixture {
    temp: TempDir,
    root: PathBuf,
    database: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("files");
        fs::create_dir(&root).unwrap();
        let database = temp.path().join("cache/index.sqlite3");
        Self {
            temp,
            root,
            database,
        }
    }

    fn command(&self) -> Command {
        let binary = env!("CARGO_BIN_EXE_file-picker-native");
        let mut command = Command::new(binary);
        command
            .args(["--root"])
            .arg(&self.root)
            .env("FILE_PICKER_INDEX", &self.database)
            .env("FILE_PICKER_EXECUTABLE", binary);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        let output = self.command().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn write(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, "fixture").unwrap();
        path
    }

    fn paths(&self) -> Vec<Vec<u8>> {
        self.run(&["--cached-list"])
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(<[u8]>::to_vec)
            .collect()
    }
}

type Frame = (Vec<u8>, Vec<Vec<u8>>);

struct Feed {
    child: Child,
    frames: mpsc::Receiver<Frame>,
}

impl Feed {
    fn start(fixture: &Fixture) -> Self {
        let mut child = fixture
            .command()
            .arg("--index-stream")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, frames) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut record = || {
                let mut bytes = Vec::new();
                if reader.read_until(0, &mut bytes).ok()? == 0 {
                    return None;
                }
                if bytes.pop() != Some(0) {
                    return None;
                }
                Some(bytes)
            };
            while let Some(header) = record() {
                let mut paths = Vec::new();
                if matches!(header.as_slice(), b"cached" | b"fresh") {
                    loop {
                        let Some(path) = record() else {
                            return;
                        };
                        if path.is_empty() {
                            break;
                        }
                        paths.push(path);
                    }
                } else if header == b"warning" {
                    let Some(error) = record() else {
                        return;
                    };
                    paths.push(error);
                }
                if sender.send((header, paths)).is_err() {
                    return;
                }
            }
        });
        Self { child, frames }
    }

    fn until(&self, wanted: &[u8]) -> Vec<Vec<u8>> {
        for _ in 0..20 {
            let (header, paths) = self
                .frames
                .recv_timeout(Duration::from_secs(5))
                .expect("Index feed timed out");
            assert_ne!(header, b"warning", "Index warning: {paths:?}");
            if header == wanted {
                return paths;
            }
        }
        panic!("Expected index feed frame: {wanted:?}");
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn cached_cli_reads_work_without_fd_and_removal_never_deletes_a_file() {
    let fixture = Fixture::new();
    let file = fixture.write("ø quoted ' file\n.pdf");
    let update: serde_json::Value =
        serde_json::from_slice(&fixture.run(&["--index-update"]).stdout).unwrap();
    assert_eq!(update["added"], 1);
    let empty_path = fixture.temp.path().join("empty-bin");
    fs::create_dir(&empty_path).unwrap();
    let output = fixture
        .command()
        .arg("--cached-list")
        .env("PATH", empty_path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, [file.as_os_str().as_bytes(), b"\0"].concat());
    let removed = fixture
        .command()
        .arg("--index-remove")
        .arg(&file)
        .output()
        .unwrap();
    assert!(removed.status.success());
    assert!(fixture.paths().is_empty());
    assert!(file.exists());
    fixture.run(&["--index-update"]);
    assert_eq!(fixture.paths().len(), 1);
    fixture.run(&["--index-clear"]);
    assert!(fixture.paths().is_empty());
    assert!(file.exists());
}

#[test]
fn feed_shows_cache_then_applies_external_updates_removals_and_clear() {
    let fixture = Fixture::new();
    let old = fixture.write("old.txt");
    fixture.run(&["--index-update"]);
    let feed = Feed::start(&fixture);
    assert_eq!(feed.until(b"cached"), [old.as_os_str().as_bytes()]);
    feed.until(b"ready");
    fs::remove_file(old).unwrap();
    let new = fixture.write("new.txt");
    fixture.run(&["--index-update"]);
    assert_eq!(feed.until(b"fresh"), [new.as_os_str().as_bytes()]);
    let output = fixture
        .command()
        .arg("--index-remove")
        .arg(&new)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(feed.until(b"fresh").is_empty());
    assert!(new.exists());
    fixture.run(&["--index-update"]);
    assert_eq!(feed.until(b"fresh"), [new.as_os_str().as_bytes()]);
    fixture.run(&["--index-clear"]);
    assert!(feed.until(b"fresh").is_empty());
}

#[test]
fn interrupted_refresh_preserves_the_previous_complete_index() {
    let fixture = Fixture::new();
    let file = fixture.write("keep.txt");
    fixture.run(&["--index-update"]);
    let shim_dir = fixture.temp.path().join("bin");
    fs::create_dir(&shim_dir).unwrap();
    let marker = fixture.temp.path().join("scan-started");
    let shim = shim_dir.join("fd");
    script(
        &shim,
        "printf started > \"$PICKER_TEST_MARKER\"\nexec \"$PICKER_TEST_SLEEP\" 10",
    );
    let mut refresh = fixture
        .command()
        .arg("--index-update")
        .env("PATH", &shim_dir)
        .env("PICKER_TEST_MARKER", &marker)
        .env("PICKER_TEST_SLEEP", program("sleep"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while !marker.exists() {
        if start.elapsed() > Duration::from_secs(3) {
            let _ = refresh.kill();
            let _ = refresh.wait();
            panic!("Scan did not start");
        }
        thread::sleep(Duration::from_millis(5));
    }
    refresh.kill().unwrap();
    refresh.wait().unwrap();
    assert_eq!(fixture.paths(), [file.as_os_str().as_bytes()]);
}

#[test]
fn corrupt_cache_falls_back_to_live_results_without_replacing_the_database() {
    let fixture = Fixture::new();
    let file = fixture.write("found.txt");
    fs::create_dir_all(fixture.database.parent().unwrap()).unwrap();
    fs::write(&fixture.database, b"not a sqlite database").unwrap();
    let output = fixture.run(&["--index-stream"]);
    assert!(output.stdout.starts_with(b"warning\0"));
    assert!(
        output
            .stdout
            .windows(file.as_os_str().as_bytes().len())
            .any(|window| window == file.as_os_str().as_bytes())
    );
    assert!(output.stdout.ends_with(b"\0\0ready\0"));
    assert_eq!(
        fs::read(&fixture.database).unwrap(),
        b"not a sqlite database"
    );
    assert!(Path::new(&file).exists());
}

#[test]
fn cached_reader_can_open_while_another_connection_holds_a_write_transaction() {
    let fixture = Fixture::new();
    let file = fixture.write("available.txt");
    fixture.run(&["--index-update"]);
    let mut writer = rusqlite::Connection::open(&fixture.database).unwrap();
    let transaction = writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transaction.execute("DELETE FROM files", []).unwrap();
    assert_eq!(fixture.paths(), [file.as_os_str().as_bytes()]);
}

#[test]
fn background_refresh_starts_even_while_cached_output_is_blocked() {
    let fixture = Fixture::new();
    fixture.write("real.txt");
    fixture.run(&["--index-update"]);
    let connection = rusqlite::Connection::open(&fixture.database).unwrap();
    connection.execute("WITH RECURSIVE seq(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM seq WHERE n<5000)
        INSERT INTO files(scope,path,changed_sec,changed_nsec)
        SELECT (SELECT id FROM scopes LIMIT 1),CAST(printf('/tmp/old-cached-long-filename-%080d',n) AS BLOB),0,0 FROM seq", []).unwrap();
    let shim_dir = fixture.temp.path().join("bin");
    fs::create_dir(&shim_dir).unwrap();
    let marker = fixture.temp.path().join("refresh-started");
    let shim = shim_dir.join("fd");
    script(
        &shim,
        "printf started > \"$PICKER_TEST_MARKER\"\nexec \"$PICKER_TEST_FD\" \"$@\"",
    );
    let mut feed = fixture
        .command()
        .arg("--index-stream")
        .env("PATH", &shim_dir)
        .env("PICKER_TEST_MARKER", &marker)
        .env("PICKER_TEST_FD", program("fd"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // Do not consume stdout: the cache exceeds pipe capacity.
    let deadline = Instant::now() + Duration::from_secs(3);
    while !marker.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    let started = marker.exists();
    feed.kill().unwrap();
    feed.wait().unwrap();
    assert!(started, "Cached output blocked the background refresh");
    fixture.run(&["--index-update"]);
    assert_eq!(fixture.paths().len(), 1);
}
