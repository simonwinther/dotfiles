//! End-to-end sessions use synthetic PCM, fake desktop tools, and loopback servers.
//! These tests never open a microphone, contact OpenAI, or touch the desktop clipboard.
use dictate::storage::{self, Identity};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Output, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
    sync::oneshot,
    time::{Instant, sleep, timeout},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

struct Desktop {
    root: PathBuf,
}
impl Desktop {
    fn new(endpoint: &str, extra: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("dictate-session-test-{}", storage::generation()));
        for folder in ["tools", "config/dictate", "runtime", "state"] {
            storage::private_dir(&root.join(folder)).unwrap();
        }
        let script = |name: &str, text: &str| {
            let path = root.join("tools").join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        };
        script(
            "pw-record",
            "#!/bin/sh\ntrap 'exit 0' INT TERM\nwhile :; do cat \"$DICTATE_TEST_PCM\" || exit 1; sleep 0.02; done\n",
        );
        script(
            "wl-copy",
            "#!/bin/sh\nif test -f \"$DICTATE_TEST_COPY_FAIL\"; then exit 1; fi\ncat > \"$DICTATE_TEST_CLIPBOARD\"\n",
        );
        script("notify-send", "#!/bin/sh\nexit 0\n");
        script("hyprctl", "#!/bin/sh\nexit 1\n");
        let pcm: Vec<_> = (0..600)
            .flat_map(|i| {
                let value = ((i as f64 / 12.).sin() * 12000.) as i16;
                value.to_le_bytes()
            })
            .collect();
        fs::write(root.join("microphone.pcm"), pcm).unwrap();
        fs::write(root.join("config/dictate/config.env"),format!("OPENAI_API_KEY=test-key-for-loopback-only\nOPENAI_REALTIME_URL='{endpoint}'\nOPENAI_API_URL=http://127.0.0.1:1/transcriptions\nDICTIONARY_FILE=''\nSHOW_LOADING=0\nDUCK_VOLUME=0\nAUTO_PASTE=0\nFFMPEG_PREPROCESS=0\nMIN_RECORDING_MS=100\nMIN_AUDIO_BYTES=1\nSKIP_SILENCE=0\nMAX_RECORDING_MS=5000\nHTTP_CONNECT_TIMEOUT_S=1\nLIVE_FINAL_TIMEOUT_S=0.2\nLAST_LOG_FILE={}\nLAST_TRANSCRIPT_FILE={}\n{extra}\n",root.join("last.log").display(),root.join("last.txt").display())).unwrap();
        Self { root }
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_dictate"));
        c.env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("tools").display()),
            )
            .env("DICTATE_TEST_PCM", self.root.join("microphone.pcm"))
            .env("DICTATE_TEST_COPY_FAIL", self.root.join("fail-copy"))
            .env("DICTATE_TEST_CLIPBOARD", self.root.join("clipboard.txt"))
            .env("DICTATE_TEST_VOLUME", self.root.join("volume.txt"))
            .env("DICTATE_TEST_PASTE", self.root.join("paste.txt"));
        c
    }
    async fn call(&self, args: &[&str]) -> Output {
        timeout(Duration::from_secs(8), self.command().args(args).output())
            .await
            .unwrap()
            .unwrap()
    }
    async fn ok(&self, args: &[&str]) -> String {
        let output = self.call(args).await;
        assert!(
            output.status.success(),
            "{args:?}: {}\nlog: {}",
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(self.root.join("last.log")).unwrap_or_default()
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
    async fn idle(&self) {
        let until = Instant::now() + Duration::from_secs(5);
        while self
            .root
            .join("runtime/dictate/native-session.json")
            .exists()
        {
            assert!(
                Instant::now() < until,
                "session did not stop: {}",
                fs::read_to_string(self.root.join("last.log")).unwrap_or_default()
            );
            sleep(Duration::from_millis(15)).await;
        }
    }
    fn recovery(&self) -> Option<Value> {
        let base = self.root.join("state/dictate/recovery");
        let pointer: Value =
            serde_json::from_slice(&fs::read(base.join("latest.json")).ok()?).ok()?;
        serde_json::from_slice(
            &fs::read(
                base.join(pointer["generation"].as_str()?)
                    .join("metadata.json"),
            )
            .ok()?,
        )
        .ok()
    }
    fn state(&self) -> Value {
        serde_json::from_slice(
            &fs::read(self.root.join("runtime/dictate/native-session.json")).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        if let Ok(bytes) = fs::read(self.root.join("runtime/dictate/native-session.json"))
            && let Ok(s) = serde_json::from_slice::<Value>(&bytes)
        {
            if let Ok(owner) = serde_json::from_value::<Identity>(s["owner"].clone()) {
                owner.signal(libc::SIGKILL);
            }
            if let Ok(recorder) = serde_json::from_value::<Identity>(s["recorder"].clone()) {
                recorder.signal(libc::SIGKILL);
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
async fn realtime(
    final_text: Option<&'static str>,
) -> (
    String,
    oneshot::Receiver<()>,
    tokio::task::JoinHandle<usize>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (first_tx, first_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let _ = ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        let mut first = Some(first_tx);
        let mut commits = 0;
        while let Some(Ok(message)) = ws.next().await {
            let Ok(text) = message.to_text() else {
                continue;
            };
            let v: Value = serde_json::from_str(text).unwrap();
            if v["type"] == "input_audio_buffer.append"
                && let Some(tx) = first.take()
            {
                let _ = tx.send(());
            }
            if v["type"] == "input_audio_buffer.commit" {
                commits += 1;
                ws.send(Message::Text(
                    json!({"type":"input_audio_buffer.committed","item_id":"ours"}).to_string(),
                ))
                .await
                .unwrap();
                ws.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"ours","delta":"provisional"}).to_string())).await.unwrap();
                if let Some(text) = final_text {
                    ws.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"ours","transcript":text}).to_string())).await.unwrap();
                }
            }
        }
        commits
    });
    (url, first_rx, task)
}

#[tokio::test]
async fn live_session_preserves_shortcut_flow_and_delivers_final_text() {
    let (url, first, server) =
        realtime(Some("i'm testing PyTorch comma next new line 1e-4.")).await;
    let desktop = Desktop::new(&url, "");
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(150)).await;
    assert!(desktop.ok(&["status"]).await.contains("recording (live)"));
    assert!(!desktop.call(&["batch", "start"]).await.status.success());
    desktop.ok(&["live", "toggle"]).await;
    desktop.idle().await;
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "I'm testing PyTorch, next\n1e-4."
    );
    assert!(desktop.recovery().is_none());
    assert_eq!(server.await.unwrap(), 1);
}
#[tokio::test]
async fn cancelling_streaming_never_commits_or_changes_clipboard() {
    let (url, first, server) = realtime(Some("Must not paste")).await;
    let desktop = Desktop::new(&url, "");
    fs::write(desktop.root.join("clipboard.txt"), "previous clipboard").unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    desktop.ok(&["cancel"]).await;
    desktop.idle().await;
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "previous clipboard"
    );
    assert!(desktop.recovery().is_none());
    assert_eq!(server.await.unwrap(), 0);
}
#[tokio::test]
async fn final_timeout_preserves_audio_but_never_pastes_partial_text() {
    let (url, first, server) = realtime(None).await;
    let desktop = Desktop::new(&url, "");
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(150)).await;
    desktop.ok(&["live", "stop"]).await;
    desktop.idle().await;
    assert!(!desktop.root.join("clipboard.txt").exists());
    let recovery = desktop
        .recovery()
        .expect("audio must survive network failure");
    assert!(recovery["transcript"].is_null());
    assert_eq!(recovery["settings"]["TRANSCRIBE_MODEL"], "gpt-transcribe");
    assert_eq!(recovery["settings"]["RECORD_RATE"], "24000");
    assert!(recovery["settings"].get("OPENAI_API_KEY").is_none());
    assert_eq!(server.await.unwrap(), 1);
    desktop.ok(&["batch", "discard"]).await;
    assert!(desktop.recovery().is_none());
}
#[tokio::test]
async fn failed_clipboard_retries_cached_text_without_a_second_upload() {
    let (url, first, server) = realtime(Some("Cached final text.")).await;
    let desktop = Desktop::new(&url, "");
    fs::write(desktop.root.join("fail-copy"), "").unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(150)).await;
    desktop.ok(&["live", "stop"]).await;
    desktop.idle().await;
    assert_eq!(
        desktop.recovery().unwrap()["transcript"],
        "Cached final text."
    );
    assert_eq!(server.await.unwrap(), 1);
    fs::remove_file(desktop.root.join("fail-copy")).unwrap();
    desktop.ok(&["batch", "retry"]).await;
    desktop.idle().await;
    // The batch URL is deliberately unreachable. Success proves the cached result was used.
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "Cached final text."
    );
    assert!(desktop.recovery().is_none());
}
#[tokio::test]
async fn batch_capture_uploads_once_and_automatic_limit_stops_recording() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/transcriptions", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut data = Vec::new();
        let mut buffer = [0u8; 8192];
        let header_end;
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buffer[..n]);
            if let Some(end) = data.windows(4).position(|s| s == b"\r\n\r\n") {
                header_end = end + 4;
                break;
            }
        }
        let headers = String::from_utf8_lossy(&data[..header_end]);
        let length = headers
            .lines()
            .find_map(|s| {
                s.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|s| s.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        while data.len() < header_end + length {
            let n = stream.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            data.extend_from_slice(&buffer[..n]);
        }
        assert!(data.windows(4).any(|s| s == b"RIFF"));
        assert!(String::from_utf8_lossy(&data).contains("gpt-transcribe"));
        let response = r#"{"text":"Batch result."}"#;
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
    });
    let desktop = Desktop::new(
        "ws://127.0.0.1:1",
        &format!("OPENAI_API_URL={endpoint}\nMAX_RECORDING_MS=300"),
    );
    desktop.ok(&["batch", "start"]).await;
    desktop.idle().await;
    server.await.unwrap();
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "Batch result."
    );
}
#[tokio::test]
async fn killed_worker_recovers_recording_on_next_status() {
    let (url, first, server) = realtime(None).await;
    let desktop = Desktop::new(&url, "");
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(60)).await;
    let state = desktop.state();
    let owner: Identity = serde_json::from_value(state["owner"].clone()).unwrap();
    owner.signal(libc::SIGKILL);
    sleep(Duration::from_millis(30)).await;
    assert!(desktop.ok(&["status"]).await.contains("recovery available"));
    assert!(desktop.recovery().is_some());
    assert!(!desktop.root.join("clipboard.txt").exists());
    server.await.unwrap();
}

#[tokio::test]
async fn terminal_paste_targets_focused_window_and_uses_lua_provider() {
    let (url, first, server) = realtime(Some("Paste into terminal.")).await;
    let desktop = Desktop::new(&url, "AUTO_PASTE=1\nPASTE_DELAY=0");
    fs::write(desktop.root.join("tools/hyprctl"),"#!/bin/sh\ncase \"$1\" in\nactivewindow) echo '{\"class\":\"com.mitchellh.ghostty\",\"address\":\"0x42\"}' ;;\nstatus) echo 'configProvider: lua' ;;\ndispatch) printf '%s' \"$2\" > \"$DICTATE_TEST_PASTE\"; echo ok ;;\n*) exit 1 ;;\nesac\n").unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(150)).await;
    desktop.ok(&["live", "stop"]).await;
    desktop.idle().await;
    let dispatched = fs::read_to_string(desktop.root.join("paste.txt")).unwrap();
    assert!(dispatched.contains("hl.dsp.send_shortcut"));
    assert!(dispatched.contains("CTRL SHIFT"));
    assert!(dispatched.contains("address:0x42"));
    server.await.unwrap();
}

#[tokio::test]
async fn native_volume_ducks_and_restores_the_original_sink() {
    let (url, first, server) = realtime(None).await;
    let desktop = Desktop::new(
        &url,
        "DUCK_VOLUME=1\nVOLUME_DUCK_FADE_IN_MS=0\nVOLUME_DUCK_FADE_OUT_MS=0",
    );
    fs::write(desktop.root.join("volume.txt"), "0.75").unwrap();
    let wpctl = desktop.root.join("tools/wpctl");
    fs::write(&wpctl,"#!/bin/sh\ncase \"$1\" in\ninspect) printf 'id 42, type PipeWire:Interface:Node\\n    object.serial = \"stable-serial-42\"\\n' ;;\nget-volume) printf 'Volume: '; cat \"$DICTATE_TEST_VOLUME\" ;;\nset-volume) printf '%s' \"$3\" > \"$DICTATE_TEST_VOLUME\" ;;\nset-mute) exit 0 ;;\n*) exit 1 ;;\nesac\n").unwrap();
    fs::set_permissions(&wpctl, fs::Permissions::from_mode(0o755)).unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    let limit = Instant::now() + Duration::from_secs(1);
    loop {
        if fs::read_to_string(desktop.root.join("volume.txt")).unwrap() == "0.2250" {
            break;
        }
        assert!(Instant::now() < limit, "volume was not ducked");
        sleep(Duration::from_millis(10)).await;
    }
    desktop.ok(&["cancel"]).await;
    desktop.idle().await;
    assert_eq!(
        fs::read_to_string(desktop.root.join("volume.txt")).unwrap(),
        "0.7500"
    );
    assert!(
        !desktop
            .root
            .join("runtime/dictation-volume-duck/state.env")
            .exists()
    );
    assert_eq!(server.await.unwrap(), 0);
}

#[tokio::test]
async fn interrupted_batch_header_is_repaired_for_recovery() {
    let desktop = Desktop::new("ws://127.0.0.1:1", "");
    desktop.ok(&["batch", "start"]).await;
    sleep(Duration::from_millis(80)).await;
    let state = desktop.state();
    let owner: Identity = serde_json::from_value(state["owner"].clone()).unwrap();
    owner.signal(libc::SIGKILL);
    sleep(Duration::from_millis(30)).await;
    desktop.ok(&["status"]).await;
    let base = desktop.root.join("state/dictate/recovery");
    let pointer: Value =
        serde_json::from_slice(&fs::read(base.join("latest.json")).unwrap()).unwrap();
    let wav = dictate::audio::read_wav(
        &base
            .join(pointer["generation"].as_str().unwrap())
            .join("recording.wav"),
    )
    .unwrap();
    assert!(!wav.samples.is_empty());
    assert!(
        desktop.recovery().unwrap()["recording_ms"]
            .as_u64()
            .unwrap()
            > 0
    );
}
