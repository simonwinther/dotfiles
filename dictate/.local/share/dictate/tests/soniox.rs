use dictate::{
    config::Config,
    network::{self, Chunk},
    text::Processor,
    ui::View,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{mpsc, watch},
    time::{Duration, timeout},
};
use tokio_tungstenite::{accept_hdr_async, tungstenite::Message};

fn config(endpoint: String) -> Config {
    let mut c = Config::default();
    c.set("LIVE_PROVIDER", "soniox");
    c.set("SONIOX_API_KEY", "test-soniox-key");
    c.set("SONIOX_REALTIME_URL", endpoint);
    c.set("DICTIONARY_FILE", "");
    c.set("MIN_RECORDING_MS", "100");
    c.set("LIVE_FINAL_TIMEOUT_S", "0.1");
    c.set(
        "LAST_LOG_FILE",
        format!("/tmp/dictate-soniox-{}.log", std::process::id()),
    );
    c
}

#[test]
fn carries_language_and_vocabulary_without_credentials_in_start_or_snapshot() {
    let mut c = config("ws://127.0.0.1:1".into());
    c.set("OPENAI_PROMPT", "English and Danish scientific dictation");
    c.set("LIVE_KEYWORDS", "PyTorch,MeanFlows");
    let start = network::live_update(&c, &Processor::load(&c).unwrap(), "").unwrap();
    assert_eq!(start["model"], "stt-rt-v6");
    assert_eq!(start["audio_format"], "pcm_s16le");
    assert_eq!(start["sample_rate"], 24000);
    assert_eq!(start["num_channels"], 1);
    assert_eq!(start["language_hints"], json!(["en", "da"]));
    assert_eq!(start["context"]["terms"], json!(["PyTorch", "MeanFlows"]));
    assert_eq!(start["context"]["text"], c.get("OPENAI_PROMPT"));
    assert!(!start.to_string().contains("test-soniox-key"));
    assert!(!c.snapshot().contains_key("SONIOX_API_KEY"));
    assert!(!c.live_recovery("").unwrap().contains_key("SONIOX_API_KEY"));
    assert!(c.live_key().is_ok());
    assert!(c.key().is_err());
    let override_ = network::live_update(&c, &Processor::load(&c).unwrap(), "da").unwrap();
    assert_eq!(override_["language_hints"], json!(["da"]));
}

#[tokio::test]
#[allow(
    clippy::result_large_err,
    reason = "The handshake callback uses tungstenite's response type."
)]
async fn previews_before_stop_replaces_drafts_and_flushes_all_audio() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_hdr_async(
            stream,
            |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                assert_eq!(request.headers()["authorization"], "Bearer test-soniox-key");
                Ok(response)
            },
        )
        .await
        .unwrap();
        let start: Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert!(start.get("api_key").is_none());
        let mut bytes = Vec::new();
        let mut chunks = 0;
        while let Some(Ok(message)) = ws.next().await {
            match message {
                Message::Binary(audio) => {
                    chunks += 1;
                    bytes.extend(audio);
                    let event = if chunks == 1 {
                        json!({"tokens":[{"text":"Pie torch", "is_final":false}]})
                    } else {
                        json!({"tokens":[{"text":"PyTorch", "is_final":true},{"text":" works", "is_final":false}]})
                    };
                    ws.send(Message::Text(event.to_string())).await.unwrap();
                }
                Message::Text(text) => {
                    assert!(text.is_empty(), "Stop must be an empty text frame");
                    assert_eq!(chunks, 2);
                    ws.send(Message::Text(json!({"tokens":[{"text":" works.","is_final":true},{"text":"<end>","is_final":true}]}).to_string())).await.unwrap();
                    ws.send(Message::Text(
                        json!({"tokens":[],"finished":true}).to_string(),
                    ))
                    .await
                    .unwrap();
                    break;
                }
                other => panic!("Unexpected frame: {other:?}"),
            }
        }
        bytes
    });
    let (tx, rx) = mpsc::channel(4);
    let (view, mut previews) = watch::channel(View::default());
    let sender = tokio::spawn(async move {
        tx.send(Chunk::Audio(vec![1; 4800])).await.unwrap();
        timeout(Duration::from_secs(2), async {
            loop {
                previews.changed().await.unwrap();
                if previews.borrow().transcript == "Pie torch" {
                    break;
                }
            }
        })
        .await
        .unwrap();
        tx.send(Chunk::Audio(vec![2; 4800])).await.unwrap();
        timeout(Duration::from_secs(2), async {
            loop {
                previews.changed().await.unwrap();
                if previews.borrow().transcript == "PyTorch works" {
                    break;
                }
            }
        })
        .await
        .unwrap();
        tx.send(Chunk::End(true)).await.unwrap();
    });
    let p = Processor::load(&c).unwrap();
    let result = timeout(Duration::from_secs(3), network::live(&c, &p, "", rx, &view))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result, "PyTorch works.");
    assert_eq!(view.borrow().transcript, result);
    sender.await.unwrap();
    assert_eq!(
        server.await.unwrap(),
        [vec![1; 4800], vec![2; 4800]].concat()
    );
}

#[tokio::test]
async fn missing_final_response_times_out_without_returning_draft() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        while let Some(Ok(message)) = ws.next().await {
            if matches!(message, Message::Text(ref text) if text.is_empty()) {
                ws.send(Message::Text(
                    json!({"tokens":[{"text":"Draft only", "is_final":false}]}).to_string(),
                ))
                .await
                .unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
                break;
            }
        }
    });
    let (tx, rx) = mpsc::channel(4);
    tx.send(Chunk::Audio(vec![0; 4800])).await.unwrap();
    tx.send(Chunk::End(true)).await.unwrap();
    let (view, _) = watch::channel(View::default());
    let error = network::live(&c, &Processor::load(&c).unwrap(), "", rx, &view)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("partial text was not delivered"));
    server.await.unwrap();
}

#[tokio::test]
async fn authentication_error_is_reported_without_exposing_key() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(
            json!({"error_code":401,"error_message":"Invalid test-soniox-key"}).to_string(),
        ))
        .await
        .unwrap();
    });
    let (_tx, rx) = mpsc::channel(4);
    let (view, _) = watch::channel(View::default());
    let error = network::live(&c, &Processor::load(&c).unwrap(), "", rx, &view)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Soniox: Invalid [redacted]");
    server.await.unwrap();
}

#[tokio::test]
async fn live_evaluation_replays_recording_and_scores_without_sending_reference() {
    let root = std::env::temp_dir().join(format!("dictate-live-eval-{}", std::process::id()));
    std::fs::create_dir_all(root.join("config/dictate")).unwrap();
    let pcm = root.join("recording.pcm");
    std::fs::write(&pcm, vec![0; 9600]).unwrap();
    dictate::audio::wrap_pcm(&pcm, &root.join("recording.wav"), 24000, 1, "s16").unwrap();
    let suite = root.join("suite.toml");
    std::fs::write(&suite, "version = 1\nname = 'Live comparison'\n[[clips]]\nid = 'ml'\naudio = 'recording.wav'\nexpected = 'Machine learning uses PyTorch.'\nterms = ['PyTorch']\n").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    std::fs::write(root.join("config/dictate/config.env"), format!("SONIOX_API_KEY=test-soniox-key\nSONIOX_REALTIME_URL=ws://{}\nDICTIONARY_FILE=''\nMIN_RECORDING_MS=100\nLAST_LOG_FILE={}\n", listener.local_addr().unwrap(), root.join("last.log").display())).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let start = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(!start.contains("Machine learning uses PyTorch"));
        let mut received = 0;
        while let Some(Ok(message)) = ws.next().await {
            match message {
                Message::Binary(bytes) => {
                    received += bytes.len();
                    ws.send(Message::Text(
                        json!({"tokens":[{"text":"Machine", "is_final":false}]}).to_string(),
                    ))
                    .await
                    .unwrap();
                }
                Message::Text(text) if text.is_empty() => {
                    assert_eq!(received, 9600);
                    ws.send(Message::Text(json!({"tokens":[{"text":"Machine learning uses PyTorch.","is_final":true}],"finished":true}).to_string())).await.unwrap();
                    break;
                }
                _ => (),
            }
        }
    });
    let report = root.join("report.json");
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_dictate"))
        .args(["eval", "live"])
        .arg(&suite)
        .args(["--provider", "soniox", "--output"])
        .arg(&report)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let data: Value = serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(data["summary"]["wer"], 0.);
    assert_eq!(data["clips"][0]["terms"]["recall"], 1.);
    let timing = &data["clips"][0]["timing"];
    assert_eq!(timing["audio_ms"], 200.);
    assert!(timing["first_text_ms"].as_f64().unwrap() < timing["session_ms"].as_f64().unwrap());
    assert!(timing["final_after_audio_ms"].as_f64().unwrap() >= 0.);
    server.await.unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
