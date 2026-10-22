use crate::{config::Config, storage, text::Processor};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{Sink, SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, timeout, timeout_at},
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

/// The capture task has the detailed cause; distinguish this from API failures.
#[derive(Debug)]
pub(crate) struct CaptureFailed(&'static str);

impl std::fmt::Display for CaptureFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for CaptureFailed {}

pub fn keywords(c: &Config, p: &Processor, live: bool) -> Result<Vec<String>> {
    let csv = if live {
        c.inherited("LIVE_KEYWORDS", "OPENAI_KEYWORDS")
    } else {
        c.get("OPENAI_KEYWORDS")
    };
    let mut words = Vec::<String>::new();
    let mut seen = BTreeSet::new();
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
        if seen.insert(word.to_lowercase()) {
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

const FINAL_TIMEOUT: &str =
    "Timed out waiting for final transcription; partial text was not delivered";
const TRANSCRIPT_LIMIT: usize = 4 * 1024 * 1024;
const TRANSCRIPT_ITEMS: usize = 32;
const PREVIEW_CHARS: usize = 8000;

enum TranscriptItem {
    Partial(String),
    Final(String),
}
impl TranscriptItem {
    fn text(&self) -> &str {
        match self {
            Self::Partial(text) | Self::Final(text) => text,
        }
    }
}
#[derive(Default)]
struct LiveTranscripts {
    items: BTreeMap<String, TranscriptItem>,
    bytes: usize,
}
impl LiveTranscripts {
    fn reserve(&mut self, id: &str, old_length: usize, new_length: usize) -> Result<()> {
        ensure!(!id.is_empty(), "Missing transcript item id");
        let new_item = !self.items.contains_key(id);
        ensure!(
            !new_item || self.items.len() < TRANSCRIPT_ITEMS,
            "Too many unsolicited transcript items"
        );
        let bytes = self.bytes - old_length + new_length + if new_item { id.len() } else { 0 };
        ensure!(bytes <= TRANSCRIPT_LIMIT, "Transcript exceeded size limit");
        self.bytes = bytes;
        Ok(())
    }
    fn delta(&mut self, id: &str, delta: &str) -> Result<Option<&str>> {
        if matches!(self.items.get(id), Some(TranscriptItem::Final(_))) {
            return Ok(None);
        }
        let old_length = self.items.get(id).map_or(0, |item| item.text().len());
        self.reserve(id, old_length, old_length + delta.len())?;
        let TranscriptItem::Partial(text) = self
            .items
            .entry(id.into())
            .or_insert_with(|| TranscriptItem::Partial(String::new()))
        else {
            unreachable!()
        };
        text.push_str(delta);
        Ok(Some(text))
    }
    fn complete(&mut self, id: &str, text: &str) -> Result<()> {
        let old_length = self.items.get(id).map_or(0, |item| item.text().len());
        self.reserve(id, old_length, text.len())?;
        self.items
            .insert(id.into(), TranscriptItem::Final(text.into()));
        Ok(())
    }
    fn take_final(&mut self, id: &str) -> Option<String> {
        if !matches!(self.items.get(id), Some(TranscriptItem::Final(_))) {
            return None;
        }
        let Some(TranscriptItem::Final(text)) = self.items.remove(id) else {
            unreachable!()
        };
        self.bytes -= id.len() + text.len();
        Some(text)
    }
}

fn live_preview(p: &Processor, text: &str) -> String {
    // Only the last 4000 characters are displayed. Retain extra context for spoken
    // punctuation, and avoid rescanning the whole recording on every delta.
    let start = text
        .char_indices()
        .rev()
        .nth(PREVIEW_CHARS - 1)
        .map_or(0, |(index, _)| index);
    p.preview(&text[start..])
}

async fn live_send<S>(
    ws: &mut S,
    message: Message,
    c: &Config,
    final_deadline: Option<Instant>,
) -> Result<()>
where
    S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let upload_deadline = Instant::now() + c.seconds("HTTP_CONNECT_TIMEOUT_S");
    let deadline = final_deadline.map_or(upload_deadline, |d| d.min(upload_deadline));
    timeout_at(deadline, ws.send(message))
        .await
        .with_context(|| {
            if final_deadline.is_some_and(|d| Instant::now() >= d) {
                FINAL_TIMEOUT
            } else {
                "Realtime upload stalled"
            }
        })??;
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
            match message {
                Message::Text(text) => {
                    let v: Value = serde_json::from_str(&text)?;
                    check_event(&v)?;
                    if v["type"] == "session.updated" {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Message::Close(_) => bail!("Realtime connection closed during setup"),
                _ => (),
            }
        }
    })
    .await
    .context("Realtime session setup timed out")??;
    storage::log(c, "event=live_connected");
    let mut sent = 0usize;
    let mut committed = None::<String>;
    let mut transcripts = LiveTranscripts::default();
    let mut deadline = None::<Instant>;
    let mut ended = false;
    let mut first = true;
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            bail!(FINAL_TIMEOUT);
        }
        if let Some(id) = &committed
            && let Some(text) = transcripts.take_final(id)
        {
            view.send_modify(|v| v.transcript = live_preview(p, &text));
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
                        live_send(&mut ws, Message::Text(event.to_string()), c, deadline).await?;
                    }
                    Some(Chunk::End(valid))=>{
                        if !valid{return Err(CaptureFailed("Microphone recorder exited unexpectedly").into());}
                        if sent as u64*1000<48000*c.millis("MIN_RECORDING_MS").max(100){return Ok(String::new());}
                        ended=true;deadline=Some(Instant::now()+c.seconds("LIVE_FINAL_TIMEOUT_S"));
                        live_send(&mut ws, Message::Text(json!({"type":"input_audio_buffer.commit"}).to_string()), c, deadline).await?;
                        storage::log(c,&format!("event=live_commit audio_bytes={sent}"));
                    }
                    None=>return Err(CaptureFailed("Audio capture ended before a completed turn").into()),
                }
            }
            message=ws.next()=>{
                match message.ok_or_else(||anyhow::anyhow!("Realtime connection closed before the final transcript"))?? {
                    Message::Text(text)=>{
                        let v:Value=serde_json::from_str(&text)?;check_event(&v)?;
                        let id=v["item_id"].as_str().unwrap_or("").to_owned();
                        match v["type"].as_str().unwrap_or(""){
                            "input_audio_buffer.committed"=>{
                                ensure!(ended,"Unexpected transcript commit before recording stopped");
                                ensure!(!id.is_empty(),"Missing committed item id");
                                ensure!(committed.as_ref().is_none_or(|previous| previous == &id),"Conflicting transcript commit acknowledgements");
                                committed=Some(id);
                            },
                            "conversation.item.input_audio_transcription.delta"=>{
                                if first{storage::log(c,"event=live_first_delta");first=false;}
                                let delta=v["delta"].as_str().ok_or_else(||anyhow::anyhow!("Missing transcript delta"))?;
                                if let Some(preview)=transcripts.delta(&id,delta)? {
                                    view.send_modify(|view|view.transcript=live_preview(p,preview));
                                }
                            }
                            "conversation.item.input_audio_transcription.completed"=>{
                                let text=v["transcript"].as_str().ok_or_else(||anyhow::anyhow!("Missing final transcript"))?;
                                transcripts.complete(&id,text)?;
                            }
                            _=>(),
                        }
                    }
                    Message::Close(_)=>bail!("Realtime connection closed before the final transcript"),
                    Message::Ping(data)=>live_send(&mut ws, Message::Pong(data), c, deadline).await?,
                    _=>(),
                }
            }
            _=tokio::time::sleep(wait),if deadline.is_some()=>bail!(FINAL_TIMEOUT),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn transcript_storage_limits_all_items_and_releases_replaced_partials() {
        let mut transcripts = LiveTranscripts::default();
        let half = "a".repeat(TRANSCRIPT_LIMIT / 2);
        transcripts.delta("ours", &half).unwrap();
        assert!(transcripts.delta("other", &half).is_err());
        // The final replaces the partial, rather than retaining both copies.
        transcripts.complete("ours", "Final").unwrap();
        assert_eq!(transcripts.bytes, "ours".len() + "Final".len());
        assert!(transcripts.delta("ours", "Stale draft").unwrap().is_none());
        transcripts.delta("other", &half).unwrap();
        assert_eq!(transcripts.take_final("ours").unwrap(), "Final");
        assert_eq!(transcripts.bytes, "other".len() + half.len());
        assert!(transcripts.delta("", "Missing id").is_err());

        let mut transcripts = LiveTranscripts::default();
        for index in 0..TRANSCRIPT_ITEMS {
            transcripts.delta(&index.to_string(), "word").unwrap();
        }
        assert!(transcripts.complete("extra", "text").is_err());
        assert!(transcripts.delta("extra", "text").is_err());
        // An existing item remains writable when all item slots are occupied.
        transcripts.complete("0", "Final").unwrap();
        assert_eq!(transcripts.take_final("0").unwrap(), "Final");
    }

    #[test]
    fn vocabulary_context_reaches_live_and_batch_without_duplicate_hints() {
        let mut c = Config::default();
        c.set("DICTIONARY_FILE", "");
        c.set(
            "OPENAI_PROMPT",
            "English and Danish data science discussion.",
        );
        c.set("OPENAI_KEYWORDS", " PyTorch, ROC-AUC, pytorch ");
        let mut p = Processor::load(&c).unwrap();
        p.dictionary.terms = vec![
            "PyTorch".into(),
            " cross-validation ".into(),
            "ROC-AUC".into(),
        ];
        let hints = vec!["PyTorch", "ROC-AUC", "cross-validation"];
        let update = live_update(&c, &p, "").unwrap();
        let live = &update["session"]["audio"]["input"]["transcription"];
        assert_eq!(live["keywords"], json!(hints));
        assert_eq!(live["prompt"], c.get("OPENAI_PROMPT"));
        let fields = batch_fields(&c, &p, "").unwrap();
        assert_eq!(
            fields
                .iter()
                .filter(|(key, _)| key == "keywords[]")
                .map(|(_, value)| value.as_str())
                .collect::<Vec<_>>(),
            hints
        );
        assert!(fields.contains(&("prompt".into(), c.get("OPENAI_PROMPT").into())));
        // Explicit live overrides still take precedence, including an empty prompt.
        c.set("LIVE_PROMPT", "");
        c.set("LIVE_KEYWORDS", "JAX");
        let update = live_update(&c, &p, "").unwrap();
        let live = &update["session"]["audio"]["input"]["transcription"];
        assert!(live.get("prompt").is_none());
        assert_eq!(
            live["keywords"],
            json!(["JAX", "PyTorch", "cross-validation", "ROC-AUC"])
        );
    }

    #[tokio::test]
    async fn stalled_control_writes_respect_upload_and_final_deadlines() {
        for (message, final_deadline) in [
            (Message::Pong(vec![1]), None),
            (
                Message::Text(json!({"type":"input_audio_buffer.commit"}).to_string()),
                Some(Instant::now()),
            ),
        ] {
            let mut c = Config::default();
            c.set("HTTP_CONNECT_TIMEOUT_S", "0.01");
            let mut stalled = Box::pin(futures_util::sink::unfold((), |(), _: Message| {
                std::future::pending::<std::result::Result<(), tokio_tungstenite::tungstenite::Error>>(
                )
            }));
            let error = timeout(
                Duration::from_secs(1),
                live_send(&mut stalled, message, &c, final_deadline),
            )
            .await
            .expect("stalled writes must have a deadline")
            .unwrap_err();
            assert_eq!(
                error.to_string(),
                if final_deadline.is_some() {
                    FINAL_TIMEOUT
                } else {
                    "Realtime upload stalled"
                }
            );
        }
    }
}
