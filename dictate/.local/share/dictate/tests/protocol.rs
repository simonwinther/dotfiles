use base64::{Engine, engine::general_purpose::STANDARD};
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
    sync::{mpsc, oneshot, watch},
    time::{Duration, timeout},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn config(url: String) -> Config {
    let mut c = Config::default();
    c.set("OPENAI_API_KEY", "test-key-never-use-on-the-network");
    c.set("DICTIONARY_FILE", "");
    c.set("OPENAI_REALTIME_URL", url);
    c.set("MIN_RECORDING_MS", "100");
    c.set("LIVE_FINAL_TIMEOUT_S", "0.08");
    c.set(
        "LAST_LOG_FILE",
        format!("/tmp/dictate-protocol-test-{}.log", std::process::id()),
    );
    c
}
#[tokio::test]
async fn streams_before_stop_drains_audio_and_matches_committed_item() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let (first_tx, first_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let update: Value =
            serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        assert_eq!(update["session"]["audio"]["input"]["format"]["rate"], 24000);
        assert!(update["session"]["audio"]["input"]["turn_detection"].is_null());
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        let mut bytes = Vec::new();
        let mut first = Some(first_tx);
        let mut commits = 0;
        while let Some(Ok(message)) = ws.next().await {
            let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            match v["type"].as_str().unwrap() {
                "input_audio_buffer.append" => {
                    bytes.extend(STANDARD.decode(v["audio"].as_str().unwrap()).unwrap());
                    if let Some(first) = first.take() {
                        first.send(()).unwrap();
                    }
                    ws.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"ours","delta":"Draft words"}).to_string())).await.unwrap();
                }
                "input_audio_buffer.commit" => {
                    commits += 1;
                    // The final event may arrive before the commit acknowledgement.
                    for event in [
                        json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"other","transcript":"Wrong text"}),
                        json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"ours","transcript":"Final corrected text."}),
                        json!({"type":"input_audio_buffer.committed","item_id":"ours"}),
                    ] {
                        ws.send(Message::Text(event.to_string())).await.unwrap();
                    }
                    break;
                }
                kind => panic!("Unexpected event: {kind}"),
            }
        }
        (bytes, commits)
    });
    let (tx, rx) = mpsc::channel(4);
    let (view, _) = watch::channel(View::default());
    let p = Processor::load(&c).unwrap();
    let sender = tokio::spawn(async move {
        tx.send(Chunk::Audio([1, 2].repeat(2400))).await.unwrap();
        timeout(Duration::from_secs(1), first_rx)
            .await
            .unwrap()
            .unwrap();
        tx.send(Chunk::Audio([3, 4].repeat(3000))).await.unwrap();
        tx.send(Chunk::End(true)).await.unwrap();
    });
    let result = timeout(Duration::from_secs(2), network::live(&c, &p, "", rx, &view))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result, "Final corrected text.");
    assert_eq!(view.borrow().transcript, result);
    sender.await.unwrap();
    let (bytes, commits) = server.await.unwrap();
    assert_eq!(bytes, [[1, 2].repeat(2400), [3, 4].repeat(3000)].concat());
    assert_eq!(commits, 1);
}

#[tokio::test]
async fn final_timeout_does_not_deliver_partial_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        while let Some(Ok(message)) = ws.next().await {
            let v: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if v["type"] == "input_audio_buffer.commit" {
                ws.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"ours","delta":"Partial text"}).to_string())).await.unwrap();
                tokio::time::sleep(Duration::from_millis(200)).await;
                break;
            }
        }
    });
    let (tx, rx) = mpsc::channel(4);
    tx.send(Chunk::Audio(vec![0; 4800])).await.unwrap();
    tx.send(Chunk::End(true)).await.unwrap();
    let (view, _) = watch::channel(View::default());
    let p = Processor::load(&c).unwrap();
    let error = network::live(&c, &p, "", rx, &view).await.unwrap_err();
    assert!(error.to_string().contains("partial text was not delivered"));
    server.await.unwrap();
}
#[tokio::test]
async fn too_short_audio_is_never_committed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        let mut commits = 0;
        while let Some(Ok(message)) = ws.next().await {
            if let Ok(text) = message.to_text() {
                let v: Value = serde_json::from_str(text).unwrap();
                if v["type"] == "input_audio_buffer.commit" {
                    commits += 1;
                }
            }
        }
        commits
    });
    let (tx, rx) = mpsc::channel(4);
    tx.send(Chunk::Audio(vec![0; 100])).await.unwrap();
    tx.send(Chunk::End(true)).await.unwrap();
    let (view, _) = watch::channel(View::default());
    let p = Processor::load(&c).unwrap();
    assert!(
        network::live(&c, &p, "", rx, &view)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.await.unwrap(), 0);
}
#[tokio::test]
async fn api_failure_interrupts_a_running_capture() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = config(format!("ws://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        ws.next().await.unwrap().unwrap();
        ws.send(Message::Text(json!({"type":"session.updated"}).to_string()))
            .await
            .unwrap();
        ws.send(Message::Text(
            json!({"type":"error","error":{"message":"Model unavailable"}}).to_string(),
        ))
        .await
        .unwrap();
    });
    let (_tx, rx) = mpsc::channel(4);
    let (view, _) = watch::channel(View::default());
    let p = Processor::load(&c).unwrap();
    assert!(
        timeout(Duration::from_secs(1), network::live(&c, &p, "", rx, &view))
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("Model unavailable")
    );
    server.await.unwrap();
}
