use crate::{config::Config, storage, text::Processor};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::AUTHORIZATION},
        protocol::WebSocketConfig,
    },
};

pub enum Chunk {
    Audio(Vec<u8>),
    End(bool),
}
pub fn keywords(c: &Config, p: &Processor, live: bool) -> Result<Vec<String>> {
    let csv = if live {
        c.inherited("LIVE_KEYWORDS", "OPENAI_KEYWORDS")
    } else {
        c.get("OPENAI_KEYWORDS")
    };
    let mut words = Vec::<String>::new();
    for word in csv
        .split(',')
        .chain(p.dictionary.terms.iter().map(String::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ensure!(
            !word.contains(['<', '>', '\r', '\n']),
            "Keywords must be single lines without < or >"
        );
        if !words
            .iter()
            .any(|v| v.to_lowercase() == word.to_lowercase())
        {
            words.push(word.into());
        }
    }
    Ok(words)
}
pub fn live_update(c: &Config, p: &Processor, language: &str) -> Result<Value> {
    ensure!(
        c.get("LIVE_MODEL") == "gpt-live-transcribe",
        "Unsupported LIVE_MODEL: {}",
        c.get("LIVE_MODEL")
    );
    ensure!(
        matches!(
            c.get("LIVE_DELAY"),
            "minimal" | "low" | "medium" | "high" | "xhigh"
        ),
        "Invalid LIVE_DELAY"
    );
    let mut transcription = json!({"model":c.get("LIVE_MODEL"),"delay":c.get("LIVE_DELAY")});
    let languages = c.languages(true, language)?;
    if !languages.is_empty() {
        transcription["languages"] = json!(languages);
    }
    let words = keywords(c, p, true)?;
    if !words.is_empty() {
        transcription["keywords"] = json!(words);
    }
    let prompt = c.inherited("LIVE_PROMPT", "OPENAI_PROMPT");
    if !prompt.is_empty() {
        transcription["prompt"] = json!(prompt);
    }
    Ok(
        json!({"type":"session.update","session":{"type":"transcription","audio":{"input":{"format":{"type":"audio/pcm","rate":24000},"transcription":transcription,"turn_detection":null}}}}),
    )
}
pub fn batch_fields(c: &Config, p: &Processor, language: &str) -> Result<Vec<(String, String)>> {
    let model = c.get("TRANSCRIBE_MODEL");
    ensure!(
        matches!(
            model,
            "gpt-transcribe" | "gpt-4o-transcribe" | "gpt-4o-mini-transcribe" | "whisper-1"
        ),
        "Unsupported TRANSCRIBE_MODEL: {model}"
    );
    let languages = c.languages(false, language)?;
    let modern = model == "gpt-transcribe";
    if !modern {
        ensure!(
            languages.len() <= 1 && languages.iter().all(|l| l.len() == 2),
            "{model} accepts one two-letter language hint; use --language or gpt-transcribe"
        );
        ensure!(
            c.get("OPENAI_KEYWORDS").is_empty(),
            "OPENAI_KEYWORDS requires gpt-transcribe"
        );
    }
    let mut fields = vec![
        ("model".into(), model.into()),
        ("response_format".into(), "json".into()),
    ];
    let mut prompt = c.get("OPENAI_PROMPT").to_owned();
    if !modern && !p.dictionary.terms.is_empty() {
        if !prompt.is_empty() {
            prompt.push('\n');
        }
        prompt.push_str(&p.dictionary.terms.join(", "));
    }
    if !prompt.is_empty() {
        fields.push(("prompt".into(), prompt));
    }
    if modern {
        for word in keywords(c, p, false)? {
            fields.push(("keywords[]".into(), word));
        }
        for lang in languages {
            fields.push(("languages[]".into(), lang));
        }
    } else if let Some(lang) = languages.first() {
        fields.push(("language".into(), lang.clone()));
    }
    Ok(fields)
}
fn check_event(v: &Value) -> Result<()> {
    if matches!(
        v["type"].as_str(),
        Some("error" | "conversation.item.input_audio_transcription.failed")
    ) {
        bail!(
            "{}",
            v["error"]["message"]
                .as_str()
                .unwrap_or("Realtime transcription failed")
        );
    }
    Ok(())
}

pub async fn live(
    c: &Config,
    p: &Processor,
    language: &str,
    mut audio: mpsc::Receiver<Chunk>,
    view: &watch::Sender<crate::ui::View>,
) -> Result<String> {
    let update = live_update(c, p, language)?;
    let mut request = c.get("OPENAI_REALTIME_URL").into_client_request()?;
    request.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", c.key()?))?,
    );
    let cfg = WebSocketConfig {
        max_message_size: Some(4 * 1024 * 1024),
        max_frame_size: Some(4 * 1024 * 1024),
        ..Default::default()
    };
    let (mut ws, _) = timeout(
        c.seconds("HTTP_CONNECT_TIMEOUT_S"),
        connect_async_with_config(request, Some(cfg), true),
    )
    .await
    .context("Realtime connection timed out")??;
    timeout(c.seconds("HTTP_CONNECT_TIMEOUT_S"), async {
        ws.send(Message::Text(update.to_string())).await?;
        loop {
            let message = ws
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("Realtime connection closed during setup"))??;
            if let Message::Text(text) = message {
                let v: Value = serde_json::from_str(&text)?;
                check_event(&v)?;
                if v["type"] == "session.updated" {
                    return Ok::<_, anyhow::Error>(());
                }
            }
        }
    })
    .await
    .context("Realtime session setup timed out")??;
    storage::log(c, "event=live_connected");
    let mut sent = 0usize;
    let mut committed = None::<String>;
    let mut completed = BTreeMap::<String, String>::new();
    let mut partials = BTreeMap::<String, String>::new();
    let mut deadline = None::<Instant>;
    let mut ended = false;
    let mut first = true;
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            bail!("Timed out waiting for final transcription; partial text was not delivered");
        }
        if let Some(id) = &committed
            && let Some(text) = completed.remove(id)
        {
            view.send_modify(|v| v.transcript = p.preview(&text));
            return Ok(text);
        }
        let wait = deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
            .unwrap_or(std::time::Duration::from_secs(86400));
        tokio::select! {
            frame=audio.recv(),if !ended=>{
                match frame {
                    Some(Chunk::Audio(bytes))=>{
                        ensure!(bytes.len()%2==0,"Partial PCM16 sample");sent+=bytes.len();
                        let event=json!({"type":"input_audio_buffer.append","audio":STANDARD.encode(bytes)});
                        timeout(c.seconds("HTTP_CONNECT_TIMEOUT_S"),ws.send(Message::Text(event.to_string()))).await.context("Realtime upload stalled")??;
                    }
                    Some(Chunk::End(valid))=>{
                        ensure!(valid,"Microphone recorder exited unexpectedly");
                        if sent as u64*1000<48000*c.millis("MIN_RECORDING_MS").max(100){return Ok(String::new());}
                        ws.send(Message::Text(json!({"type":"input_audio_buffer.commit"}).to_string())).await?;
                        ended=true;deadline=Some(Instant::now()+c.seconds("LIVE_FINAL_TIMEOUT_S"));storage::log(c,&format!("event=live_commit audio_bytes={sent}"));
                    }
                    None=>bail!("Audio capture ended before a completed turn"),
                }
            }
            message=ws.next()=>{
                match message.ok_or_else(||anyhow::anyhow!("Realtime connection closed before the final transcript"))?? {
                    Message::Text(text)=>{
                        let v:Value=serde_json::from_str(&text)?;check_event(&v)?;
                        let id=v["item_id"].as_str().unwrap_or("").to_owned();
                        match v["type"].as_str().unwrap_or(""){
                            "input_audio_buffer.committed"=>{ensure!(!id.is_empty(),"Missing committed item id");committed=Some(id);},
                            "conversation.item.input_audio_transcription.delta"=>{
                                if first{storage::log(c,"event=live_first_delta");first=false;}
                                let preview=partials.entry(id).or_default();preview.push_str(v["delta"].as_str().unwrap_or(""));
                                ensure!(preview.len()<=4*1024*1024,"Transcript exceeded size limit");
                                view.send_modify(|view|view.transcript=p.preview(preview));
                            }
                            "conversation.item.input_audio_transcription.completed"=>{
                                let text=v["transcript"].as_str().ok_or_else(||anyhow::anyhow!("Missing final transcript"))?;
                                completed.insert(id,text.into());ensure!(completed.len()<=32,"Too many unsolicited transcript items");
                            }
                            _=>(),
                        }
                    }
                    Message::Close(_)=>bail!("Realtime connection closed before the final transcript"),
                    Message::Ping(data)=>ws.send(Message::Pong(data)).await?,
                    _=>(),
                }
            }
            _=tokio::time::sleep(wait),if deadline.is_some()=>bail!("Timed out waiting for final transcription; partial text was not delivered"),
        }
    }
}
pub async fn batch(c: &Config, p: &Processor, language: &str, path: &Path) -> Result<String> {
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        matches!(
            extension.as_str(),
            "flac" | "mp3" | "mp4" | "mpeg" | "mpga" | "m4a" | "ogg" | "wav" | "webm"
        ),
        "Unsupported audio format: {extension}"
    );
    let fields = batch_fields(c, p, language)?;
    let file = storage::read_file(path)?;
    let length = file.metadata()?.len();
    let reader = tokio::fs::File::from_std(file);
    let part = reqwest::multipart::Part::stream_with_length(
        reqwest::Body::wrap_stream(tokio_util::io::ReaderStream::new(reader)),
        length,
    )
    .file_name(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
    )
    .mime_str(if extension == "wav" {
        "audio/wav"
    } else {
        "application/octet-stream"
    })?;
    let mut form = reqwest::multipart::Form::new().part("file", part);
    for (k, v) in fields {
        form = form.text(k, v);
    }
    let client = reqwest::Client::builder()
        .connect_timeout(c.seconds("HTTP_CONNECT_TIMEOUT_S"))
        .timeout(c.seconds("HTTP_MAX_TIME_S"))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let started = Instant::now();
    storage::log(c, "event=request_started");
    let mut response = client
        .post(c.get("OPENAI_API_URL"))
        .bearer_auth(c.key()?)
        .multipart(form)
        .send()
        .await?;
    let success = response.status().is_success();
    let status = response.status();
    let limit = if success { 4 * 1024 * 1024 } else { 64 * 1024 };
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= limit,
            "Transcription response exceeded its size limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    let v: Value = serde_json::from_slice(&bytes).context("Invalid JSON transcription response")?;
    if !success {
        bail!(
            "Transcription HTTP {status}: {}",
            v["error"]["message"].as_str().unwrap_or("Request rejected")
        );
    }
    let text = v["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing text in transcription response"))?;
    storage::log(
        c,
        &format!(
            "event=request_finished request_ms={}",
            started.elapsed().as_millis()
        ),
    );
    Ok(text.into())
}
