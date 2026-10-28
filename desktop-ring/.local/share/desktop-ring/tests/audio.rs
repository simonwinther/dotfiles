//! Real libpulse protocol tests against a private, hardware-free server.
//! Every client receives an explicit private socket; no host audio is accessed.
use desktop_ring::{
    audio::AudioAction,
    core::{Action, Request, Response},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

fn program(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("missing test dependency: {name}"))
}
fn signal(child: &Child, name: &str) {
    assert!(
        Command::new(program("sh"))
            .args(["-c", "kill -\"$1\" \"$2\"", "ring-audio-test", name])
            .arg(child.id().to_string())
            .status()
            .unwrap()
            .success()
    );
}
struct Fixture {
    child: Child,
    server: Option<Child>,
    dir: PathBuf,
}
impl Fixture {
    fn prepare(moves: bool) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ring-audio-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(dir.join("pulse-runtime")).unwrap();
        fs::create_dir(dir.join("bin")).unwrap();
        // Prove the production backend never falls back to external commands.
        fs::write(
            dir.join("bin/pactl"),
            format!(
                "#!{}\necho unexpected >> \"$XDG_RUNTIME_DIR/pactl-called\"\nexit 99\n",
                program("sh").display()
            ),
        )
        .unwrap();
        fs::set_permissions(dir.join("bin/pactl"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.join("client.conf"), "autospawn = no\nenable-shm = no\n").unwrap();
        fs::write(dir.join("server.pa"), format!(
            "load-module module-native-protocol-unix socket={} auth-anonymous=1\n\
             load-module module-null-sink sink_name=speaker sink_properties=device.description=Speaker\n\
             load-module module-null-sink sink_name=headset sink_properties=device.description=Headset\n\
             load-module module-null-source source_name=mic description=Microphone\n\
             set-default-sink speaker\nset-default-source mic\n\
             set-sink-volume speaker 32768\nset-sink-volume headset 32768\nset-source-volume mic 32768\n",
            dir.join("pulse.sock").display())).unwrap();
        fs::write(
            dir.join("config.json"),
            json!({
                "monitors":false,"audio":true,"bluetooth":false,"power":false,"move_streams":moves,
            })
            .to_string(),
        )
        .unwrap();
        fs::write(dir.join("server.log"), "").unwrap();
        dir
    }
    fn launch(dir: PathBuf, server: Option<Child>) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_desktop-ring"))
            .arg("daemon")
            .env("PATH", dir.join("bin"))
            .env(
                "PULSE_SERVER",
                format!("unix:{}", dir.join("pulse.sock").display()),
            )
            .env("PULSE_CLIENTCONFIG", dir.join("client.conf"))
            .env("HOME", &dir)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/no-session-bus", dir.display()),
            )
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                format!("unix:path={}/no-system-bus", dir.display()),
            )
            .env("DESKTOP_RING_CONFIG", dir.join("config.json"))
            .env("XDG_RUNTIME_DIR", &dir)
            .env("XDG_STATE_HOME", &dir)
            .stdout(Stdio::null())
            .stderr(File::create(dir.join("ring.log")).unwrap())
            .spawn()
            .unwrap();
        Self { child, server, dir }
    }
    async fn new(moves: bool) -> Self {
        let dir = Self::prepare(moves);
        let server = Self::start_server(&dir);
        let fixture = Self::launch(dir, Some(server));
        fixture.ready().await;
        fixture
    }
    async fn unresponsive() -> (Self, UnixStream) {
        let dir = Self::prepare(false);
        let listener = tokio::net::UnixListener::bind(dir.join("pulse.sock")).unwrap();
        let fixture = Self::launch(dir, None);
        let (stream, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("libpulse should connect to the private socket")
            .unwrap();
        (fixture, stream)
    }
    fn start_server(dir: &Path) -> Child {
        let _ = fs::remove_file(dir.join("pulse.sock"));
        Command::new(program("pulseaudio"))
            .args([
                "-n",
                "--daemonize=no",
                "--use-pid-file=no",
                "--exit-idle-time=-1",
                "--disable-shm=yes",
                "--log-level=warning",
            ])
            .arg(format!("--file={}", dir.join("server.pa").display()))
            .env_remove("PULSE_SERVER")
            .env("HOME", dir)
            .env("PULSE_RUNTIME_PATH", dir.join("pulse-runtime"))
            .env("XDG_RUNTIME_DIR", dir)
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}/no-session-bus", dir.display()),
            )
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                format!("unix:path={}/no-system-bus", dir.display()),
            )
            .stdout(Stdio::null())
            .stderr(File::create(dir.join("server.log")).unwrap())
            .spawn()
            .unwrap()
    }
    fn pactl(&self, args: &[&str]) -> String {
        let output = Command::new(program("pactl"))
            .args(args)
            .env(
                "PULSE_SERVER",
                format!("unix:{}", self.dir.join("pulse.sock").display()),
            )
            .env("PULSE_CLIENTCONFIG", self.dir.join("client.conf"))
            .env("HOME", &self.dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "pactl {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn list(&self, kind: &str) -> Vec<Value> {
        serde_json::from_str(&self.pactl(&["--format=json", "list", kind])).unwrap()
    }
    async fn stream(&self) -> UnixStream {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(stream) =
                    UnixStream::connect(self.dir.join("desktop-ring/control.sock")).await
                {
                    return stream;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("private daemon socket")
    }
    async fn request(&self, request: Request) -> Response {
        tokio::time::timeout(Duration::from_secs(8), async {
            let mut stream = self.stream().await;
            let mut data = serde_json::to_vec(&request).unwrap();
            data.push(b'\n');
            stream.write_all(&data).await.unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).await.unwrap();
            serde_json::from_str(&line).unwrap()
        })
        .await
        .unwrap_or_else(|_| panic!("audio request timed out: {}", self.log()))
    }
    fn log(&self) -> String {
        format!(
            "ring: {}\nserver: {}",
            fs::read_to_string(self.dir.join("ring.log")).unwrap(),
            fs::read_to_string(self.dir.join("server.log")).unwrap()
        )
    }
    async fn until(&self, predicate: impl Fn(&Response) -> bool) -> Response {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let state = self.request(Request::Status).await;
                if predicate(&state) {
                    return state;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("audio state did not converge: {}", self.log()))
    }
    async fn ready(&self) {
        self.until(|state| {
            state.snapshot.audio.error.is_none() && state.snapshot.audio.outputs.len() == 2
        })
        .await;
    }
    async fn act(&self, action: AudioAction) -> Response {
        self.request(Request::Act {
            action: Action::Audio { action },
        })
        .await
    }
    async fn exited(&mut self) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert!(status.success(), "{}", self.log());
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("audio worker should wake for shutdown");
        assert!(!self.dir.join("desktop-ring/control.sock").exists());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn actions_read_back_volume_mute_and_default_through_one_native_connection() {
    let mut fixture = Fixture::new(false).await;
    let clients = || {
        fixture
            .list("clients")
            .into_iter()
            .filter(|c| c["properties"]["application.name"] == "Desktop Ring")
            .collect::<Vec<_>>()
    };
    let initial = clients();
    assert_eq!(initial.len(), 1);
    let state = fixture.request(Request::Status).await.snapshot.audio;
    assert_eq!(state.inputs.len(), 1, "monitor sources must be filtered");
    assert_eq!(state.inputs[0].name, "mic");
    for input in [false, true] {
        for (delta, expected) in [(5, 55), (i32::MAX, 100), (i32::MIN, 0), (50, 50)] {
            let result = fixture.act(AudioAction::Volume { input, delta }).await;
            assert!(result.ok, "{:?}", result.error);
            let devices = if input {
                result.snapshot.audio.inputs
            } else {
                result.snapshot.audio.outputs
            };
            assert_eq!(devices.iter().find(|d| d.active).unwrap().volume, expected);
        }
        for muted in [true, false] {
            let result = fixture.act(AudioAction::Mute { input }).await;
            assert!(result.ok, "{:?}", result.error);
            let devices = if input {
                result.snapshot.audio.inputs
            } else {
                result.snapshot.audio.outputs
            };
            assert_eq!(devices.iter().find(|d| d.active).unwrap().muted, muted);
        }
    }
    let result = fixture
        .act(AudioAction::Select {
            input: false,
            name: "headset".into(),
        })
        .await;
    assert!(result.ok, "{:?}", result.error);
    assert_eq!(
        result
            .snapshot
            .audio
            .outputs
            .iter()
            .find(|d| d.active)
            .unwrap()
            .name,
        "headset"
    );
    assert_eq!(
        initial[0]["index"],
        clients()[0]["index"],
        "reuse the same context"
    );
    assert!(!fixture.dir.join("pactl-called").exists());
    let children = fs::read_to_string(format!(
        "/proc/{}/task/{}/children",
        fixture.child.id(),
        fixture.child.id()
    ))
    .unwrap();
    assert!(
        children.trim().is_empty(),
        "audio must not spawn children: {children}"
    );
    signal(&fixture.child, "INT");
    fixture.exited().await;
}

#[tokio::test]
async fn subscriptions_track_external_defaults_hotplug_and_disconnected_selections() {
    let fixture = Fixture::new(false).await;
    fixture.pactl(&["set-default-sink", "headset"]);
    fixture
        .until(|r| {
            r.snapshot
                .audio
                .outputs
                .iter()
                .any(|d| d.active && d.name == "headset")
        })
        .await;
    let module = fixture.pactl(&["load-module", "module-null-sink", "sink_name=hotplug"]);
    fixture.until(|r| r.snapshot.audio.outputs.len() == 3).await;
    fixture.pactl(&["unload-module", module.trim()]);
    fixture.until(|r| r.snapshot.audio.outputs.len() == 2).await;
    for name in ["hotplug", "bad\0name"] {
        let response = fixture
            .act(AudioAction::Select {
                input: false,
                name: name.into(),
            })
            .await;
        assert!(!response.ok);
        assert!(
            response
                .error
                .unwrap()
                .contains("audio device disconnected")
        );
        assert_eq!(response.snapshot.audio.outputs.len(), 2);
    }
    let response = fixture
        .act(AudioAction::Volume {
            input: false,
            delta: 5,
        })
        .await;
    assert!(response.ok);
    assert_eq!(
        response
            .snapshot
            .audio
            .outputs
            .iter()
            .find(|d| d.active)
            .unwrap()
            .name,
        "headset"
    );
}

#[tokio::test]
async fn restarting_the_private_server_recovers_without_restarting_the_daemon() {
    let mut fixture = Fixture::new(false).await;
    let mut server = fixture.server.take().unwrap();
    server.kill().unwrap();
    server.wait().unwrap();
    let state = fixture.until(|r| r.snapshot.audio.error.is_some()).await;
    assert!(state.snapshot.audio.outputs.is_empty());
    let response = fixture.act(AudioAction::Mute { input: false }).await;
    assert!(!response.ok);
    fixture.server = Some(Fixture::start_server(&fixture.dir));
    fixture.ready().await;
    let response = fixture.act(AudioAction::Mute { input: true }).await;
    assert!(response.ok, "{}", fixture.log());
    assert!(response.snapshot.audio.inputs[0].muted);
}

#[tokio::test]
async fn stalled_operations_time_out_and_can_reconnect() {
    let fixture = Fixture::new(false).await;
    signal(fixture.server.as_ref().unwrap(), "STOP");
    let start = Instant::now();
    let response = fixture.act(AudioAction::Mute { input: false }).await;
    assert!(!response.ok);
    assert!(response.error.unwrap().contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(7));
    assert!(response.snapshot.audio.error.is_some());
    signal(fixture.server.as_ref().unwrap(), "CONT");
    fixture.ready().await;
}

#[tokio::test]
async fn shutdown_wakes_an_inflight_operation_and_joins_the_worker() {
    let mut fixture = Fixture::new(false).await;
    signal(fixture.server.as_ref().unwrap(), "STOP");
    let mut stream = fixture.stream().await;
    let mut data = serde_json::to_vec(&Request::Act {
        action: Action::Audio {
            action: AudioAction::Mute { input: false },
        },
    })
    .unwrap();
    data.push(b'\n');
    stream.write_all(&data).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    signal(&fixture.child, "TERM");
    fixture.exited().await;
}

#[tokio::test]
async fn stalled_connection_setup_has_a_deadline() {
    let (mut fixture, _stream) = Fixture::unresponsive().await;
    fixture.until(|r| r.snapshot.audio.error.is_some()).await;
    assert!(fixture.log().contains("PipeWire request timed out"));
    signal(&fixture.child, "TERM");
    fixture.exited().await;
}

#[tokio::test]
async fn shutdown_also_wakes_connection_setup() {
    let (mut fixture, _stream) = Fixture::unresponsive().await;
    signal(&fixture.child, "TERM");
    fixture.exited().await;
}

#[tokio::test]
async fn cancelled_native_operations_release_their_callback_once() {
    use libpulse_binding::{context, mainloop::standard::Mainloop};
    use std::sync::Arc;
    struct Capture(Arc<AtomicUsize>);
    impl Drop for Capture {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let fixture = Fixture::new(false).await;
    let mut mainloop = Mainloop::new().unwrap();
    let mut context = context::Context::new(&mainloop, "Cancellation regression").unwrap();
    context
        .connect(
            Some(&format!(
                "unix:{}",
                fixture.dir.join("pulse.sock").display()
            )),
            context::FlagSet::NOAUTOSPAWN,
            None,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while context.get_state() != context::State::Ready {
        assert!(
            Instant::now() < deadline,
            "private native context should connect"
        );
        mainloop.iterate(false);
        std::thread::sleep(Duration::from_millis(1));
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let capture = Capture(drops.clone());
    let mut operation = context.introspect().get_server_info(move |_| {
        std::hint::black_box(&capture);
    });
    // Cancel before iterating, so the completion callback cannot run.
    operation.cancel();
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "cancelled callback must be freed"
    );
    operation.cancel();
    drop(operation);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        1,
        "callback must be freed exactly once"
    );
    let capture = Capture(drops.clone());
    let mut completed = context.introspect().get_server_info(move |_| {
        std::hint::black_box(&capture);
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while drops.load(Ordering::SeqCst) != 2 {
        assert!(Instant::now() < deadline, "normal callback should complete");
        mainloop.iterate(false);
        std::thread::sleep(Duration::from_millis(1));
    }
    completed.cancel();
    drop(completed);
    assert_eq!(
        drops.load(Ordering::SeqCst),
        2,
        "completed callback must not be freed twice"
    );
    context.disconnect();
}

struct PrivateStream(Child);
impl Drop for PrivateStream {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn private_stream(fixture: &Fixture, input: bool, role: &str) -> PrivateStream {
    let mut command = Command::new(program("pacat"));
    command
        .args(["--raw", "--rate=8000", "--channels=1"])
        .arg(if input { "--record" } else { "--playback" })
        .arg(if input {
            "--device=mic"
        } else {
            "--device=speaker"
        })
        .arg(format!("--property=application.name=ring-{role}"))
        .env(
            "PULSE_SERVER",
            format!("unix:{}", fixture.dir.join("pulse.sock").display()),
        )
        .env("PULSE_CLIENTCONFIG", fixture.dir.join("client.conf"))
        .env("HOME", &fixture.dir)
        .stdin(File::open("/dev/zero").unwrap())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if role == "filter" {
        command.arg("--property=media.role=filter");
    }
    if role == "virtual" {
        command.arg("--property=node.virtual=true");
    }
    PrivateStream(command.spawn().unwrap())
}

#[tokio::test]
async fn optional_stream_moves_use_fresh_indices_and_skip_internal_streams() {
    for moves in [false, true] {
        let fixture = Fixture::new(moves).await;
        fixture.pactl(&["load-module", "module-null-source", "source_name=mic2"]);
        fixture.until(|r| r.snapshot.audio.inputs.len() == 2).await;
        for input in [false, true] {
            // The selected device is already the default before these streams
            // open on another device. Reselecting it isolates our explicit
            // moves from the server's automatic default-change policy.
            let target = if input { "mic2" } else { "headset" };
            fixture.pactl(&[
                if input {
                    "set-default-source"
                } else {
                    "set-default-sink"
                },
                target,
            ]);
            let _streams: Vec<_> = ["normal", "filter", "virtual"]
                .iter()
                .map(|role| private_stream(&fixture, input, role))
                .collect();
            let kind = if input {
                "source-outputs"
            } else {
                "sink-inputs"
            };
            tokio::time::timeout(Duration::from_secs(3), async {
                while fixture.list(kind).len() != 3 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("private stream startup");
            let device_kind = if input { "sources" } else { "sinks" };
            let target_index = fixture
                .list(device_kind)
                .into_iter()
                .find(|d| d["name"] == target)
                .unwrap()["index"]
                .clone();
            let response = fixture
                .act(AudioAction::Select {
                    input,
                    name: target.into(),
                })
                .await;
            assert!(response.ok, "{:?}", response.error);
            for stream in fixture.list(kind) {
                let normal = stream["properties"]["application.name"] == "ring-normal";
                assert_eq!(
                    stream[if input { "source" } else { "sink" }] == target_index,
                    moves && normal,
                    "moves={moves}, input={input}, stream={stream}"
                );
            }
        }
    }
}
