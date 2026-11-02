use super::*;

pub(super) fn config(c: &Config, p: &Processor, language: &str) -> Result<Value> {
    ensure!(
        c.get("SONIOX_MODEL") == "stt-rt-v5",
        "Unsupported SONIOX_MODEL: {}",
        c.get("SONIOX_MODEL")
    );
    let mut value = json!({
        "model": c.get("SONIOX_MODEL"),
        "audio_format": "pcm_s16le",
        "sample_rate": 24000,
        "num_channels": 1,
        "enable_endpoint_detection": true,
        "endpoint_latency_adjustment_level": 2,
        "endpoint_sensitivity": 0.3,
        "max_endpoint_delay_ms": 1500,
    });
    let languages = c.languages(true, language)?;
    if !languages.is_empty() {
        value["language_hints"] = json!(languages);
    }
    let words = keywords(c, p, true)?;
    let prompt = c.inherited("LIVE_PROMPT", "OPENAI_PROMPT");
    if !words.is_empty() || !prompt.is_empty() {
        let mut context = json!({});
        if !words.is_empty() {
            context["terms"] = json!(words);
        }
        if !prompt.is_empty() {
            context["text"] = json!(prompt);
        }
        value["context"] = context;
    }
    Ok(value)
}

#[derive(Default)]
struct Transcript {
    confirmed: String,
    pending: String,
}

impl Transcript {
    fn update(&mut self, event: &Value) -> Result<()> {
        let Some(tokens) = event.get("tokens") else {
            return Ok(());
        };
        let tokens = tokens.as_array().context("Invalid transcription tokens")?;
        self.pending.clear();
        for token in tokens {
            let text = token["text"].as_str().context("Missing token text")?;
            let final_ = token["is_final"]
                .as_bool()
                .context("Missing token status")?;
            if matches!(text, "<end>" | "<fin>") {
                continue;
            }
            ensure!(
                self.confirmed.len() + self.pending.len() + text.len() <= TRANSCRIPT_LIMIT,
                "Transcript exceeded size limit"
            );
            if final_ {
                self.confirmed.push_str(text);
            } else {
                self.pending.push_str(text);
            }
        }
        Ok(())
    }
    fn preview(&self, p: &Processor) -> String {
        let start = self
            .confirmed
            .char_indices()
            .rev()
            .nth(PREVIEW_CHARS - 1)
            .map_or(0, |(index, _)| index);
        let text = format!("{}{}", &self.confirmed[start..], self.pending);
        live_preview(p, &text)
    }
}

pub(super) async fn live(
    c: &Config,
    p: &Processor,
    language: &str,
    mut audio: mpsc::Receiver<Chunk>,
    view: &watch::Sender<crate::ui::View>,
) -> Result<String> {
    let start = config(c, p, language)?;
    let mut request = c.get("SONIOX_REALTIME_URL").into_client_request()?;
    let mut authorization = HeaderValue::from_str(&format!("Bearer {}", c.live_key()?))?;
    authorization.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, authorization);
    let cfg = WebSocketConfig {
        max_message_size: Some(TRANSCRIPT_LIMIT),
        max_frame_size: Some(TRANSCRIPT_LIMIT),
        ..Default::default()
    };
    let (mut ws, _) = timeout(
        c.seconds("HTTP_CONNECT_TIMEOUT_S"),
        connect_async_with_config(request, Some(cfg), true),
    )
    .await
    .context("Soniox connection timed out")??;
    live_send(&mut ws, Message::Text(start.to_string()), c, None).await?;
    storage::log(c, "event=live_connected provider=soniox");
    let mut sent = 0usize;
    let mut transcript = Transcript::default();
    let mut deadline = None::<Instant>;
    let mut ended = false;
    let mut first = true;
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            bail!(FINAL_TIMEOUT);
        }
        let wait = deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
            .unwrap_or(std::time::Duration::from_secs(86400));
        tokio::select! {
            frame = audio.recv(), if !ended => {
                match frame {
                    Some(Chunk::Audio(bytes)) => {
                        ensure!(bytes.len() % 2 == 0, "Partial PCM16 sample");
                        sent += bytes.len();
                        live_send(&mut ws, Message::Binary(bytes), c, deadline).await?;
                    }
                    Some(Chunk::End(valid)) => {
                        if !valid {
                            return Err(CaptureFailed("Microphone recorder exited unexpectedly").into());
                        }
                        if sent as u64 * 1000 < 48000 * c.millis("MIN_RECORDING_MS").max(100) {
                            return Ok(String::new());
                        }
                        ended = true;
                        deadline = Some(Instant::now() + c.seconds("LIVE_FINAL_TIMEOUT_S"));
                        // End-of-stream finalizes the remaining audio in one request.
                        // Wait for finished, never paste a provisional transcript.
                        live_send(&mut ws, Message::Text(String::new()), c, deadline).await?;
                        storage::log(c, &format!("event=live_commit provider=soniox audio_bytes={sent}"));
                    }
                    None => return Err(CaptureFailed("Audio capture ended before a completed turn").into()),
                }
            }
            message = ws.next() => {
                match message.context("Soniox connection closed before final transcription")?? {
                    Message::Text(text) => {
                        let event: Value = serde_json::from_str(&text)?;
                        if event.get("error_code").is_some_and(|code| !code.is_null()) {
                            bail!("Soniox: {}", c.redact(event["error_message"].as_str().unwrap_or("Transcription failed")));
                        }
                        transcript.update(&event)?;
                        if first && (!transcript.confirmed.is_empty() || !transcript.pending.is_empty()) {
                            storage::log(c, "event=live_first_delta provider=soniox");
                            first = false;
                        }
                        view.send_modify(|v| v.transcript = transcript.preview(p));
                        if event["finished"] == true {
                            ensure!(ended, "Soniox finished before recording stopped");
                            ensure!(transcript.pending.is_empty(), "Soniox finished with provisional text");
                            storage::log(c, "event=live_final provider=soniox");
                            return Ok(transcript.confirmed);
                        }
                    }
                    Message::Close(_) => bail!("Soniox connection closed before final transcription"),
                    Message::Ping(data) => live_send(&mut ws, Message::Pong(data), c, deadline).await?,
                    _ => (),
                }
            }
            _ = tokio::time::sleep(wait), if deadline.is_some() => bail!(FINAL_TIMEOUT),
        }
    }
}
