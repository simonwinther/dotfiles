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
        fs::write(root.join("config/dictate/config.env"),format!("OPENAI_API_KEY=test-key-for-loopback-only\nOPENAI_REALTIME_URL='{endpoint}'\nOPENAI_API_URL=http://127.0.0.1:1/transcriptions\nDICTIONARY_FILE=''\nSHOW_LOADING=0\nDUCK_VOLUME=0\nAUTO_PASTE=0\nFFMPEG_PREPROCESS=0\nMIN_RECORDING_MS=100\nMIN_AUDIO_BYTES=1\nSKIP_SILENCE=0\nMAX_RECORDING_MS=5000\nHTTP_CONNECT_TIMEOUT_S=1\nLIVE_FINAL_TIMEOUT_S=1\nLAST_LOG_FILE={}\nLAST_TRANSCRIPT_FILE={}\n{extra}\n",root.join("last.log").display(),root.join("last.txt").display())).unwrap();
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
            .env("DICTATE_TEST_STOPPED", self.root.join("recorder-stopped"))
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
    async fn wait_audio_ms(&self, milliseconds: u64) {
        let state = self.state();
        let audio = PathBuf::from(state["audio_file"].as_str().unwrap());
        // The test microphone emits mono PCM16 at the mode's default rate.
        let (header, rate) = if state["mode"] == "live" {
            (0, 24_000)
        } else {
            (44, 16_000)
        };
        let minimum_bytes = header + milliseconds * rate * 2 / 1000;
        timeout(Duration::from_secs(2), async {
            while !fs::metadata(&audio).is_ok_and(|m| m.len() >= minimum_bytes) {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("recorder must capture the required audio duration before stopping");
    }
    async fn kill_worker_after_audio(&self, header_bytes: u64) {
        let state = self.state();
        let audio = PathBuf::from(state["audio_file"].as_str().unwrap());
        timeout(Duration::from_secs(2), async {
            while !fs::metadata(&audio).is_ok_and(|m| m.len() > header_bytes) {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("recorder must write audio before interrupting the worker");
        let owner: Identity = serde_json::from_value(state["owner"].clone()).unwrap();
        owner.signal(libc::SIGKILL);
        timeout(Duration::from_secs(2), async {
            while owner.alive() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("worker must exit before stale-session recovery runs");
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
    desktop.wait_audio_ms(100).await;
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
async fn recorder_closing_stdout_without_exiting_preserves_audio_and_is_reaped() {
    let (url, first, server) = realtime(Some("Must not paste")).await;
    let desktop = Desktop::new(&url, "MAX_RECORDING_MS=0");
    fs::write(desktop.root.join("clipboard.txt"), "previous clipboard").unwrap();
    fs::write(
        desktop.root.join("tools/pw-record"),
        "#!/bin/sh\ntrap '' INT TERM\ncat \"$DICTATE_TEST_PCM\"\nprintf '%s' \"$$\" > \"$DICTATE_TEST_STOPPED\"\nexec 1>&-\nexec sleep 60\n",
    )
    .unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(2), desktop.idle())
        .await
        .expect("closed recorder output must not leave the session waiting for exit");
    let pid = fs::read_to_string(desktop.root.join("recorder-stopped"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "recorder must be reaped"
    );
    let recovery = desktop.recovery().expect("captured audio must survive");
    assert!(recovery["transcript"].is_null());
    assert!(recovery["recording_ms"].as_u64().unwrap() > 0);
    assert!(
        recovery["reason"]
            .as_str()
            .unwrap()
            .contains("did not exit after closing its output")
    );
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "previous clipboard"
    );
    assert_eq!(server.await.unwrap(), 0);
}

#[tokio::test]
async fn quiet_recorder_exit_one_before_stop_preserves_audio_without_committing() {
    let (url, first, server) = realtime(Some("Must not paste")).await;
    let desktop = Desktop::new(&url, "");
    fs::write(desktop.root.join("clipboard.txt"), "previous clipboard").unwrap();
    fs::write(
        desktop.root.join("tools/pw-record"),
        "#!/bin/sh\ncat \"$DICTATE_TEST_PCM\"\nsleep 0.05\nexit 1\n",
    )
    .unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    desktop.idle().await;
    let recovery = desktop.recovery().expect("captured audio must survive");
    let reason = recovery["reason"].as_str().unwrap();
    assert!(reason.contains("exited unexpectedly"), "{reason}");
    assert!(reason.contains("exit status: 1"), "{reason}");
    assert!(recovery["transcript"].is_null());
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "previous clipboard"
    );
    assert_eq!(server.await.unwrap(), 0);
}

#[tokio::test]
async fn quiet_pipewire_exit_one_after_requested_stop_delivers_final_text() {
    let (url, first, server) = realtime(Some("Completed normally.")).await;
    let desktop = Desktop::new(&url, "");
    fs::write(
        desktop.root.join("tools/pw-record"),
        "#!/bin/sh\ntrap 'exit 1' INT\nwhile :; do cat \"$DICTATE_TEST_PCM\" || exit 1; sleep 0.02; done\n",
    )
    .unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    desktop.wait_audio_ms(100).await;
    desktop.ok(&["live", "stop"]).await;
    desktop.idle().await;
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "Completed normally."
    );
    assert!(desktop.recovery().is_none());
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn recorder_failure_after_stop_preserves_audio_without_committing() {
    for status in [1, 42] {
        let (url, first, server) = realtime(Some("Must not paste")).await;
        let desktop = Desktop::new(&url, "");
        fs::write(desktop.root.join("clipboard.txt"), "previous clipboard").unwrap();
        fs::write(
            desktop.root.join("tools/pw-record"),
            format!("#!/bin/sh\ntrap 'printf \"PipeWire test: input disconnected\\n\" >&2; exit {status}' INT\nwhile :; do cat \"$DICTATE_TEST_PCM\" || exit 1; sleep 0.02; done\n"),
        )
        .unwrap();
        desktop.ok(&["live", "start"]).await;
        timeout(Duration::from_secs(2), first)
            .await
            .unwrap()
            .unwrap();
        desktop.wait_audio_ms(100).await;
        desktop.ok(&["live", "stop"]).await;
        desktop.idle().await;
        let recovery = desktop
            .recovery()
            .expect("captured audio must survive recorder failure");
        assert!(recovery["transcript"].is_null());
        let reason = recovery["reason"].as_str().unwrap();
        assert!(
            reason.contains(&format!("exit status: {status}")),
            "{reason}"
        );
        assert!(
            reason.contains("PipeWire test: input disconnected"),
            "{reason}"
        );
        assert_eq!(
            fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
            "previous clipboard"
        );
        assert_eq!(server.await.unwrap(), 0);
    }
}

#[tokio::test]
async fn api_failure_keeps_its_cause_when_recorder_shutdown_also_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let desktop = Desktop::new(&format!("ws://{}", listener.local_addr().unwrap()), "");
    fs::write(
        desktop.root.join("tools/pw-record"),
        "#!/bin/sh\ntrap 'exit 42' INT\nwhile :; do cat \"$DICTATE_TEST_PCM\" || exit 1; sleep 0.02; done\n",
    )
    .unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        while let Some(Ok(Message::Text(text))) = ws.next().await {
            let event: Value = serde_json::from_str(&text).unwrap();
            if event["type"] == "input_audio_buffer.append" {
                ws.send(Message::Text(
                    json!({"type":"error","error":{"message":"Model unavailable test"}})
                        .to_string(),
                ))
                .await
                .unwrap();
                break;
            }
        }
    });
    desktop.ok(&["live", "start"]).await;
    desktop.idle().await;
    server.await.unwrap();
    let recovery = desktop.recovery().expect("captured audio must survive");
    let reason = recovery["reason"].as_str().unwrap();
    assert!(reason.contains("Model unavailable test"), "{reason}");
    assert!(!reason.contains("exit status: 42"), "{reason}");
    assert!(recovery["transcript"].is_null());
    assert!(!desktop.root.join("clipboard.txt").exists());
}

#[tokio::test]
async fn stop_drains_recorder_while_network_setup_is_backpressured() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (setup_tx, setup_rx) = oneshot::channel();
    let (resume_tx, resume_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        setup_tx.send(()).unwrap();
        resume_rx.await.unwrap();
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        let mut received = Vec::new();
        while let Some(Ok(message)) = ws.next().await {
            let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            match v["type"].as_str().unwrap() {
                "input_audio_buffer.append" => {
                    use base64::Engine;
                    received.extend(
                        base64::engine::general_purpose::STANDARD
                            .decode(v["audio"].as_str().unwrap())
                            .unwrap(),
                    );
                }
                "input_audio_buffer.commit" => {
                    ws.send(Message::Text(
                        json!({"type":"input_audio_buffer.committed","item_id":"ours"}).to_string(),
                    ))
                    .await
                    .unwrap();
                    ws.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"ours","transcript":"All audio arrived."}).to_string())).await.unwrap();
                    break;
                }
                event => panic!("Unexpected event: {event}"),
            }
        }
        received
    });
    let desktop = Desktop::new(&url, "HTTP_CONNECT_TIMEOUT_S=10\nMAX_RECORDING_MS=0");
    // Fill more than the entire bounded network queue before setup completes.
    fs::write(
        desktop.root.join("microphone.pcm"),
        [1, 2].repeat(4_800 * 600),
    )
    .unwrap();
    fs::write(
        desktop.root.join("tools/pw-record"),
        "#!/bin/sh\ntrap 'printf stopped > \"$DICTATE_TEST_STOPPED\"; exit 0' INT TERM\ncat \"$DICTATE_TEST_PCM\"\nwhile :; do sleep 0.01; done\n",
    )
    .unwrap();
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), setup_rx)
        .await
        .unwrap()
        .unwrap();
    let audio = PathBuf::from(desktop.state()["audio_file"].as_str().unwrap());
    // Saturating the queue processes two minutes of synthetic audio in a debug
    // build. Give that setup work room under parallel load; the actual stop
    // responsiveness assertion below still has a 500 ms deadline.
    timeout(Duration::from_secs(5), async {
        while fs::metadata(&audio).unwrap().len() < 4_800 * 1024 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    desktop.ok(&["live", "stop"]).await;
    timeout(Duration::from_millis(500), async {
        while !desktop.root.join("recorder-stopped").exists()
            || desktop.state()["recording_ms"].as_u64().unwrap() == 0
        {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stop must reach the microphone before network setup resumes");
    let recorded = fs::read(audio).unwrap();
    resume_tx.send(()).unwrap();
    desktop.idle().await;
    assert_eq!(server.await.unwrap(), recorded);
    assert_eq!(
        fs::read_to_string(desktop.root.join("clipboard.txt")).unwrap(),
        "All audio arrived."
    );
}
#[tokio::test]
async fn final_timeout_preserves_audio_but_never_pastes_partial_text() {
    let (url, first, server) = realtime(None).await;
    let desktop = Desktop::new(&url, "LIVE_FINAL_TIMEOUT_S=0.2");
    desktop.ok(&["live", "start"]).await;
    timeout(Duration::from_secs(2), first)
        .await
        .unwrap()
        .unwrap();
    desktop.wait_audio_ms(100).await;
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
    desktop.wait_audio_ms(100).await;
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
    let pcm = fs::read(desktop.root.join("microphone.pcm")).unwrap();
    fs::write(desktop.root.join("microphone.pcm"), pcm.repeat(4)).unwrap();
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
    desktop.kill_worker_after_audio(0).await;
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
    desktop.wait_audio_ms(100).await;
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
async fn cancelling_during_volume_fade_restores_without_waiting_for_fade_in() {
    let (url, first, server) = realtime(None).await;
    let desktop = Desktop::new(
        &url,
        "DUCK_VOLUME=1\nVOLUME_DUCK_FADE_IN_MS=4000\nVOLUME_DUCK_FADE_OUT_MS=0\nVOLUME_DUCK_FADE_STEPS=4",
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
    let saved_volume = desktop.root.join("runtime/dictation-volume-duck/state.env");
    timeout(Duration::from_secs(1), async {
        while !saved_volume.exists()
            || !fs::read_to_string(desktop.root.join("volume.txt"))
                .ok()
                .and_then(|value| value.parse::<f64>().ok())
                .is_some_and(|value| value < 0.75)
        {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fade must have saved the original volume and applied its first step");
    timeout(Duration::from_secs(1), async {
        desktop.ok(&["cancel"]).await;
        desktop.idle().await;
    })
    .await
    .expect("cancelling must interrupt fade-in and restore volume promptly");
    assert_eq!(
        fs::read_to_string(desktop.root.join("volume.txt")).unwrap(),
        "0.7500"
    );
    assert!(!saved_volume.exists());
    assert!(!desktop.root.join("clipboard.txt").exists());
    assert_eq!(server.await.unwrap(), 0);
}

#[tokio::test]
async fn interrupted_batch_header_is_repaired_for_recovery() {
    let desktop = Desktop::new("ws://127.0.0.1:1", "");
    desktop.ok(&["batch", "start"]).await;
    desktop.kill_worker_after_audio(44).await;
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
