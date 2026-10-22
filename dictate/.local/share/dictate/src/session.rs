use crate::{
    audio::{self, Levels},
    config::{self, Config},
    desktop,
    network::{self, Chunk},
    storage::{self, Identity, Lock, Recovery, Session},
    text::Processor,
    ui::View,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    os::fd::AsRawFd,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    process::Command,
    sync::{mpsc, oneshot, watch},
    time::{Instant, timeout},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Record,
    Stop,
    Cancel,
}
type Shared = Arc<Mutex<Session>>;
type Settings = Arc<Mutex<Config>>;
#[derive(Default, Serialize, Deserialize)]
pub struct Request {
    pub action: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub model: String,
}
#[derive(Serialize, Deserialize)]
pub struct Launch {
    pub config: Config,
    pub mode: String,
    pub language: String,
    pub retry: bool,
}
struct Capture {
    duration: u64,
    bytes: u64,
    db: f64,
}

#[derive(Clone, Copy, Default)]
struct CaptureProgress {
    bytes: u64,
    ended: Option<bool>,
}

/// Read queued audio from the recovery journal, so network backpressure never
/// prevents the recorder from draining or handling stop and recording limits.
async fn forward_audio(
    path: PathBuf,
    mut progress: watch::Receiver<CaptureProgress>,
    sender: mpsc::Sender<Chunk>,
) -> Result<()> {
    let mut source = None;
    let mut sent = 0;
    loop {
        let snapshot = *progress.borrow_and_update();
        while sent < snapshot.bytes {
            if source.is_none() {
                source = Some(storage::read_file(&path)?);
            }
            let length = (snapshot.bytes - sent).min(4800) as usize;
            let mut bytes = vec![0; length];
            source.as_mut().unwrap().read_exact(&mut bytes)?;
            if sender.send(Chunk::Audio(bytes)).await.is_err() {
                return Ok(());
            }
            sent += length as u64;
        }
        if let Some(valid) = snapshot.ended {
            let _ = sender.send(Chunk::End(valid)).await;
            return Ok(());
        }
        if progress.changed().await.is_err() {
            let _ = sender.send(Chunk::End(false)).await;
            return Ok(());
        }
    }
}

fn phase(state: &Shared, view: &watch::Sender<View>, value: &str) -> Result<()> {
    {
        let mut s = state.lock().unwrap();
        s.phase = value.into();
        s.save()?;
    }
    view.send_modify(|v| v.phase = value.into());
    Ok(())
}
async fn cancelled(rx: &mut watch::Receiver<Control>) {
    loop {
        if *rx.borrow() == Control::Cancel {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}
fn command_children(command: &mut Command) {
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        command.pre_exec(move || {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            if libc::getppid() != parent {
                return Err(std::io::Error::other("Session owner exited"));
            }
            Ok(())
        });
    }
}

pub async fn send(request: &Request) -> Result<String> {
    let operation = async {
        let mut socket = UnixStream::connect(config::runtime_dir().join("control.sock")).await?;
        socket.write_all(&serde_json::to_vec(request)?).await?;
        socket.write_all(b"\n").await?;
        let mut response = String::new();
        BufReader::new(socket)
            .take(16384)
            .read_to_string(&mut response)
            .await?;
        let data: serde_json::Value = serde_json::from_str(&response)?;
        ensure!(
            data["ok"] == true,
            "{}",
            data["message"].as_str().unwrap_or("Control request failed")
        );
        Ok::<_, anyhow::Error>(data["message"].as_str().unwrap_or("ok").to_owned())
    };
    timeout(Duration::from_secs(2), operation)
        .await
        .context("Dictation control timed out")?
}
pub async fn action(
    c: Config,
    mode: &str,
    action: &str,
    language: &str,
    model: &str,
) -> Result<()> {
    if let Some((legacy, owner)) = storage::legacy_active() {
        match action {
            "status" => {
                println!("recording ({legacy}, Python)");
                return Ok(());
            }
            "stop" | "toggle" if legacy == mode && mode == "live" => {
                owner.signal(libc::SIGUSR1);
                return Ok(());
            }
            "cancel" => {
                owner.signal(libc::SIGTERM);
                return Ok(());
            }
            _ => bail!(
                "A Python dictation session is still active; finish it before starting Rust dictation"
            ),
        }
    }
    let active = Session::active();
    match action {
        "status" => {
            if let Some(s) = active {
                println!("{} ({})", s.phase, s.mode);
            } else {
                if Session::load().is_some()
                    && let Ok(_lock) = Lock::acquire()
                    && storage::recover_stale(&c)?
                {
                    let _ = desktop::duck(&c, false).await;
                }
                println!(
                    "idle{}",
                    if Recovery::load().is_some() {
                        " (recovery available: dictate batch retry)"
                    } else {
                        ""
                    }
                );
            }
            Ok(())
        }
        "discard" => {
            let _lock = Lock::acquire()?;
            ensure!(active.is_none(), "Dictation is active");
            println!(
                "{}",
                if Recovery::discard(None)? {
                    "Recovery discarded"
                } else {
                    "No recovery item"
                }
            );
            Ok(())
        }
        "cancel" | "stop" => {
            if active.is_none() {
                if action == "cancel" {
                    return Ok(());
                }
                bail!("Dictate is not recording");
            }
            let message = send(&Request {
                action: action.into(),
                language: language.into(),
                model: model.into(),
            })
            .await?;
            println!("{message}");
            Ok(())
        }
        "toggle" if active.is_some() => {
            let s = active.unwrap();
            ensure!(s.mode == mode, "{} dictation is active", s.mode);
            println!(
                "{}",
                send(&Request {
                    action: "stop".into(),
                    language: language.into(),
                    model: model.into()
                })
                .await?
            );
            Ok(())
        }
        "once" => {
            start(c.clone(), mode, language, model, false).await?;
            println!("Press Enter to stop and transcribe; close stdin to cancel.");
            let mut line = String::new();
            let mut input = BufReader::new(tokio::io::stdin());
            tokio::select! {
                read=input.read_line(&mut line)=>{
                    let action=if read?==0{"cancel"}else{"stop"};
                    if Session::active().is_some(){send(&Request{action:action.into(),..Default::default()}).await?;}
                }
                _=async{while Session::active().is_some(){tokio::time::sleep(Duration::from_millis(50)).await;}}=>return Ok(()),
            }
            while Session::active().is_some() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok(())
        }
        "start" | "toggle" | "retry" => start(c, mode, language, model, action == "retry").await,
        _ => bail!("Unsupported {mode} action: {action}"),
    }
}

pub async fn start(
    mut c: Config,
    mode: &str,
    language: &str,
    model: &str,
    retry: bool,
) -> Result<()> {
    let lock = Lock::acquire()?;
    ensure!(
        Session::active().is_none() && storage::legacy_active().is_none(),
        "Dictate is already active"
    );
    if storage::recover_stale(&c)? {
        let _ = desktop::duck(&c, false).await;
    }
    if !model.is_empty() {
        c.set(
            if mode == "live" {
                "LIVE_MODEL"
            } else {
                "TRANSCRIBE_MODEL"
            },
            model,
        );
        c.set("SESSION_MODEL_OVERRIDE", model);
    }
    if mode == "batch" && !retry && !language.is_empty() {
        c.set("OPENAI_LANGUAGES", language);
        c.set("OPENAI_LANGUAGE", "");
    }
    if retry {
        ensure!(mode == "batch", "Retry is a batch action");
        ensure!(
            Recovery::load().is_some(),
            "No recording is available to retry"
        );
    }
    c.validate()?;
    c.key()?;
    let p = Processor::load(&c)?;
    if mode == "live" {
        network::live_update(&c, &p, language)?;
    } else if !retry {
        network::batch_fields(&c, &p, language)?;
    }
    ensure!(desktop::exists("wl-copy"), "Missing wl-copy");
    if !retry {
        ensure!(
            desktop::exists("pw-record") && desktop::exists("stdbuf"),
            "Missing pw-record or stdbuf"
        );
    }
    storage::write_private(&c.path("LAST_LOG_FILE"), b"")?;
    let log = storage::private_file(&c.path("LAST_LOG_FILE"), true)?;
    let fd = lock.0.as_raw_fd();
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["_worker", &fd.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(&serde_json::to_vec(&Launch {
            config: c.clone(),
            mode: mode.into(),
            language: language.into(),
            retry,
        })?)
        .await?;
    drop(input);
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    let result = timeout(Duration::from_secs(5), reader.read_line(&mut ready)).await;
    if !matches!(result,Ok(Ok(n)) if n>0) || ready.trim() != "READY" {
        let _ = child.kill().await;
        bail!("Dictation could not start; see {}", c.get("LAST_LOG_FILE"));
    }
    drop(lock);
    println!(
        "{}",
        if retry {
            "Retrying saved dictation"
        } else if mode == "live" {
            "Live dictation recording. Run `dictate live stop` to paste."
        } else {
            "Batch dictation recording. Run `dictate batch stop` to paste."
        }
    );
    Ok(())
}

pub async fn worker(fd: i32) -> Result<()> {
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
    }
    let lock = unsafe { Lock::inherited(fd) };
    let mut input = String::new();
    tokio::io::stdin()
        .take(1024 * 1024)
        .read_to_string(&mut input)
        .await?;
    let launch: Launch = serde_json::from_str(&input)?;
    let c = launch.config.clone();
    let stamp = storage::now_ms();
    let directory = config::runtime_dir()
        .join("sessions")
        .join(storage::generation());
    storage::private_dir(&directory)?;
    let live = launch.mode == "live";
    let session = Session {
        owner: Identity::get(std::process::id()).context("Cannot identify session process")?,
        recorder: None,
        mode: launch.mode.clone(),
        phase: "starting".into(),
        start_ms: stamp,
        audio_file: directory.join(if live {
            "recording.pcm"
        } else {
            "recording.wav"
        }),
        settings: if live {
            c.live_recovery(&launch.language)?
        } else {
            c.snapshot()
        },
        recording_ms: 0,
        transcript: None,
        retain_source: false,
    };
    session.save()?;
    let socket = config::runtime_dir().join("control.sock");
    if fs::symlink_metadata(&socket).is_ok() {
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    let state = Arc::new(Mutex::new(session));
    let settings = Arc::new(Mutex::new(c.clone()));
    let rate = c
        .get("OPENAI_COST_USD_PER_MINUTE")
        .parse::<f64>()
        .ok()
        .unwrap_or(if live {
            0.
        } else {
            match c.get("TRANSCRIBE_MODEL") {
                "gpt-transcribe" => 0.0045,
                "gpt-4o-mini-transcribe" => 0.003,
                _ => 0.006,
            }
        });
    let (view_tx, view_rx) = watch::channel(View {
        start_ms: stamp,
        live,
        cost_rate: if c.yes("SHOW_COST_ESTIMATE") {
            rate
        } else {
            0.
        },
        currency: c.get("OPENAI_COST_CURRENCY").into(),
        exchange: c.number("OPENAI_COST_USD_TO_DKK"),
        ..Default::default()
    });
    let (control_tx, control_rx) = watch::channel(Control::Record);
    let (ready_tx, ready_rx) = oneshot::channel();
    let overlay = if c.yes("SHOW_LOADING") {
        Some(tokio::spawn(overlay(view_rx, c.clone())))
    } else {
        None
    };
    let tracking = c
        .yes("SHOW_LOADING")
        .then(|| tokio::spawn(track_monitor(view_tx.clone())));
    let mut task = tokio::spawn(pipeline(
        launch,
        state.clone(),
        settings.clone(),
        view_tx.clone(),
        control_tx.clone(),
        control_rx,
        ready_tx,
    ));
    let mut ready = Some(ready_rx);
    let mut sigstop =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1())?;
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let result = loop {
        tokio::select! {
            value=async{ready.as_mut().unwrap().await},if ready.is_some()=>{
                ready=None;if value.is_ok(){println!("READY");std::io::stdout().flush()?;}
            }
            result=&mut task=>{
                if let Some(rx)=&mut ready&& rx.try_recv().is_ok(){println!("READY");std::io::stdout().flush()?;}
                break result.context("Dictation task failed")?;
            }
            connection=listener.accept()=>{if let Ok((stream,_))=connection{handle_control(stream,&state,&settings,&view_tx,&control_tx).await;}}
            _=sigstop.recv()=>{let _=request_control(&Request{action:"stop".into(),..Default::default()},&state,&settings,&view_tx,&control_tx);}
            _=sigterm.recv()=>{let _=request_control(&Request{action:"cancel".into(),..Default::default()},&state,&settings,&view_tx,&control_tx);}
            _=sigint.recv()=>{let _=request_control(&Request{action:"cancel".into(),..Default::default()},&state,&settings,&view_tx,&control_tx);}
        }
    };
    if let Some(tracking) = tracking {
        tracking.abort();
    }
    view_tx.send_modify(|v| v.phase = "finished".into());
    if let Some(mut overlay) = overlay
        && timeout(Duration::from_millis(500), &mut overlay)
            .await
            .is_err()
    {
        overlay.abort();
    }
    let _ = fs::remove_file(socket);
    if !state.lock().unwrap().retain_source {
        let _ = fs::remove_file(Session::path());
    }
    drop(lock);
    result
}
async fn handle_control(
    stream: UnixStream,
    state: &Shared,
    settings: &Settings,
    view: &watch::Sender<View>,
    control: &watch::Sender<Control>,
) {
    let operation = async {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        (&mut reader).take(16384).read_line(&mut line).await?;
        let result = serde_json::from_str::<Request>(&line)
            .map_err(anyhow::Error::from)
            .and_then(|r| request_control(&r, state, settings, view, control));
        let reply = match result {
            Ok(message) => json!({"ok":true,"message":message}),
            Err(e) => json!({"ok":false,"message":e.to_string()}),
        };
        reader
            .get_mut()
            .write_all(&serde_json::to_vec(&reply)?)
            .await?;
        Ok::<_, anyhow::Error>(())
    };
    let _ = timeout(Duration::from_millis(500), operation).await;
}
fn request_control(
    r: &Request,
    state: &Shared,
    settings: &Settings,
    view: &watch::Sender<View>,
    control: &watch::Sender<Control>,
) -> Result<String> {
    let mut state = state.lock().unwrap();
    match r.action.as_str() {
        "stop" => {
            ensure!(
                matches!(state.phase.as_str(), "starting" | "recording"),
                "Dictate is busy ({})",
                state.phase
            );
            if state.mode == "batch" {
                let mut c = settings.lock().unwrap();
                let mut candidate = c.clone();
                if !r.model.is_empty() {
                    candidate.set("TRANSCRIBE_MODEL", &r.model);
                }
                if !r.language.is_empty() {
                    candidate.set("OPENAI_LANGUAGES", &r.language);
                    candidate.set("OPENAI_LANGUAGE", "");
                }
                network::batch_fields(&candidate, &Processor::load(&candidate)?, "")?;
                *c = candidate;
            }
            state.phase = "stopping".into();
            state.save()?;
            view.send_modify(|v| v.phase = "processing".into());
            control.send_replace(Control::Stop);
            Ok("Finishing dictation".into())
        }
        "cancel" => {
            ensure!(
                !matches!(state.phase.as_str(), "delivering" | "finished"),
                "Delivery has started; cancellation is too late"
            );
            control.send_replace(Control::Cancel);
            Ok("Dictation cancelled".into())
        }
        "status" => Ok(format!("{} ({})", state.phase, state.mode)),
        _ => bail!("Unknown control action"),
    }
}

fn recorder_diagnostics(audio: &Path) -> Result<String> {
    let mut file = storage::read_file(&audio.with_extension("err"))?;
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(String::new());
    }
    let limit = length.min(4096);
    file.seek(SeekFrom::End(-(limit as i64)))?;
    let mut bytes = Vec::with_capacity(limit as usize);
    Read::take(file, limit).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}

async fn capture(
    c: Config,
    live: bool,
    state: Shared,
    view: watch::Sender<View>,
    mut control: watch::Receiver<Control>,
    sender: mpsc::Sender<Chunk>,
    ready: oneshot::Sender<()>,
) -> Result<Capture> {
    let rate = if live {
        24000
    } else {
        c.millis("RECORD_RATE") as u32
    };
    let channels = if live {
        1
    } else {
        c.millis("RECORD_CHANNELS") as usize
    };
    let format = if live { "s16" } else { c.get("RECORD_FORMAT") };
    let audio = state.lock().unwrap().audio_file.clone();
    let mut file = storage::private_file(&audio, false)?;
    let (progress, progress_rx) = watch::channel(CaptureProgress::default());
    let forwarding = live.then(|| tokio::spawn(forward_audio(audio.clone(), progress_rx, sender)));
    if !live {
        file.write_all(&audio::wav_header(0, rate, channels as u16, format)?)?;
    }
    let err = storage::private_file(&audio.with_extension("err"), false)?;
    let mut command = Command::new("stdbuf");
    command
        .args([
            "-o0",
            "pw-record",
            "--raw",
            "--rate",
            &rate.to_string(),
            "--channels",
            &channels.to_string(),
            "--format",
            format,
            "--latency",
            c.get("RECORD_LATENCY"),
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(err))
        .kill_on_drop(true);
    command_children(&mut command);
    let mut child = command.spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    {
        let mut s = state.lock().unwrap();
        s.recorder = child.id().and_then(Identity::get);
        s.phase = "recording".into();
        s.save()?;
    }
    view.send_modify(|v| v.phase = "recording".into());
    let _ = ready.send(());
    let began = Instant::now();
    let max = c.millis("MAX_RECORDING_MS");
    let mut stopping = false;
    let mut forced = None;
    let mut bytes = 0u64;
    let mut meter = Levels::new(rate);
    let mut buffer = [0u8; 4800];
    let mut pending = Vec::new();
    let frame = audio::sample_bytes(format) * channels;
    let mut last_frame = Instant::now() - Duration::from_secs(1);
    let result:Result<()>=async{
        loop{
            if !stopping && *control.borrow()!=Control::Record{
                stopping=true;forced=Some(Instant::now()+Duration::from_secs(2));
                if let Some(pid)=child.id(){unsafe{libc::kill(pid as i32,libc::SIGINT);}}
                view.send_modify(|v|{v.phase="processing".into();v.recording_ms=began.elapsed().as_millis() as u64;});
            }
            let remaining=forced.map(|d|d.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_secs(86400));
            let maximum_wait=if max>0 && !stopping{(began+Duration::from_millis(max)).saturating_duration_since(Instant::now())}else{Duration::from_secs(86400)};
            tokio::select!{
                n=stdout.read(&mut buffer)=>{
                    let n=n?;if n==0{break;}pending.extend_from_slice(&buffer[..n]);let len=pending.len()/frame*frame;if len==0{continue;}
                    let chunk=&pending[..len];file.write_all(chunk)?;bytes+=len as u64;meter.append(chunk,format,channels);
                    if last_frame.elapsed()>=Duration::from_millis(16){let bars=meter.bars();view.send_modify(|v|v.bars=bars);last_frame=Instant::now();}
                    if live{progress.send_replace(CaptureProgress{bytes,ended:None});}pending.drain(..len);
                }
                _=control.changed()=>(),
                _=tokio::time::sleep(remaining),if forced.is_some()=>{
                    child.start_kill()?;
                    timeout(Duration::from_secs(1),child.wait()).await.context("Microphone recorder could not be reaped")??;
                    bail!("Microphone recorder did not stop");
                },
                _=tokio::time::sleep(maximum_wait),if max>0 && !stopping=>(),
            }
            if !stopping && max>0 && began.elapsed().as_millis()>=max as u128{
                // Capture owns this timer even when the network is still connecting.
                stopping=true;forced=Some(Instant::now()+Duration::from_secs(2));if let Some(pid)=child.id(){unsafe{libc::kill(pid as i32,libc::SIGINT);}}
                view.send_modify(|v|{v.phase="processing".into();v.recording_ms=began.elapsed().as_millis() as u64;});
            }
        }
        // A recorder may close stdout without exiting. Keep its shutdown bounded
        // even though there is no longer any audio to drain or select on.
        let grace=forced.map(|d|d.saturating_duration_since(Instant::now())).unwrap_or(Duration::from_millis(200));
        let status=match timeout(grace,child.wait()).await{
            Ok(status)=>status?,
            Err(_)=>{
                child.start_kill()?;
                timeout(Duration::from_secs(1),child.wait()).await.context("Microphone recorder could not be reaped")??;
                bail!("Microphone recorder did not exit after closing its output");
            }
        };
        ensure!(stopping,"Microphone recorder exited unexpectedly ({status})");
        // pw-record 1.6.x returns 1 after a normal requested SIGINT because only
        // playback drains mark its exit successful. Actual stream failures write
        // diagnostics; do not confuse a quiet, requested stop with those failures.
        let quiet_pipewire_stop=status.code()==Some(1) && recorder_diagnostics(&audio)?.is_empty();
        ensure!(status.success() || quiet_pipewire_stop || status.code()==Some(128+libc::SIGINT) || std::os::unix::process::ExitStatusExt::signal(&status)==Some(libc::SIGINT),"Microphone recorder failed: {status}");
        Ok(())
    }.await;
    let result = result.map_err(|error| {
        let message = match recorder_diagnostics(&audio) {
            Ok(details) if !details.is_empty() => format!("{error:#}; recorder: {details}"),
            _ => format!("{error:#}"),
        };
        anyhow::anyhow!(message)
    });
    if !live {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&audio::wav_header(bytes, rate, channels as u16, format)?)?;
    }
    file.sync_all()?;
    let duration = meter.samples * 1000 / rate as u64;
    {
        let mut s = state.lock().unwrap();
        s.recording_ms = duration;
        s.save()?;
    }
    progress.send_replace(CaptureProgress {
        bytes,
        ended: Some(result.is_ok()),
    });
    if let Some(forwarding) = forwarding {
        forwarding.await.context("Audio forwarding task failed")??;
    }
    result?;
    Ok(Capture {
        duration,
        bytes,
        db: meter.db(),
    })
}

async fn pipeline(
    launch: Launch,
    state: Shared,
    settings: Settings,
    view: watch::Sender<View>,
    control: watch::Sender<Control>,
    mut controls: watch::Receiver<Control>,
    ready: oneshot::Sender<()>,
) -> Result<()> {
    let mut c = launch.config.clone();
    let live = launch.mode == "live";
    let p = Processor::load(&c)?;
    let mut transcript = None::<String>;
    let mut recovery_id = None::<String>;
    let mut retry_path = None::<PathBuf>;
    let directory = state
        .lock()
        .unwrap()
        .audio_file
        .parent()
        .unwrap()
        .to_path_buf();
    let ducker = if !launch.retry {
        let duck_config = c.clone();
        let mut rx = controls.clone();
        let mut ui = view.subscribe();
        Some(tokio::spawn(async move {
            let stopped = async {
                while *rx.borrow() == Control::Record
                    && matches!(ui.borrow().phase.as_str(), "starting" | "recording")
                {
                    tokio::select! {
                        changed=rx.changed()=>{if changed.is_err(){break;}},
                        changed=ui.changed()=>{if changed.is_err(){break;}},
                    }
                }
            };
            tokio::pin!(stopped);
            tokio::select! {
                biased;
                _=&mut stopped=>(),
                _=desktop::duck(&duck_config, true)=>stopped.await,
            }
            let _ = desktop::duck(&duck_config, false).await;
        }))
    } else {
        None
    };
    let outcome:Result<()>=async{
        let duration;
        let text;
        if launch.retry {
            let saved=Recovery::load().context("No recovery recording")?;recovery_id=Some(saved.generation.clone());
            let copied=directory.join("recording.wav");
            { let mut source=storage::read_file(&saved.audio_file)?;let mut target=storage::private_file(&copied,false)?;std::io::copy(&mut source,&mut target)?;target.sync_all()?; }
            retry_path=Some(copied.clone());
            let request_keys=["TRANSCRIBE_MODEL","OPENAI_LANGUAGES","OPENAI_LANGUAGE","OPENAI_PROMPT","OPENAI_KEYWORDS","DICTIONARY_FILE","TEXT_CLEANUP","PUNCTUATION_MODE","RECORD_RATE","RECORD_CHANNELS","RECORD_FORMAT","FFMPEG_PREPROCESS","TRIM_SILENCE","SILENCE_AUTO_THRESHOLD","SILENCE_THRESHOLD_DB","SILENCE_SPEECH_MARGIN_DB","SILENCE_MIN_PAUSE","SILENCE_KEEP","SPEEDUP_FACTOR","PREPROCESS_MIN_MS","UPLOAD_CODEC","SKIP_SILENCE"];
            let requested_model=c.get("TRANSCRIBE_MODEL").to_owned();
            for key in request_keys{if let Some(value)=saved.settings.get(key){c.set(key,value);}}
            // Overrides are recorded separately from the default model by the launcher.
            let explicit_model=c.get("SESSION_MODEL_OVERRIDE").to_owned();
            if !explicit_model.is_empty(){c.set("TRANSCRIBE_MODEL",if explicit_model=="1"{&requested_model}else{&explicit_model});}
            let changed=(!launch.language.is_empty() && c.languages(false,"")?!=vec![launch.language.to_lowercase()]) || (!explicit_model.is_empty() && saved.settings.get("TRANSCRIBE_MODEL").map(String::as_str)!=Some(c.get("TRANSCRIBE_MODEL")));
            if !launch.language.is_empty(){c.set("OPENAI_LANGUAGES",&launch.language);c.set("OPENAI_LANGUAGE","");}
            {let mut s=state.lock().unwrap();s.audio_file=copied.clone();s.settings=c.snapshot();s.recording_ms=saved.recording_ms;s.save()?;}
            c.validate()?;let p=Processor::load(&c)?;network::batch_fields(&c,&p,"")?;phase(&state,&view,"processing")?;let _=ready.send(());duration=saved.recording_ms;
            if let Some(cached)=saved.transcript.filter(|_|!changed){text=cached;}else{
                let path=tokio::select!{r=audio::preprocess(&copied,duration,&c)=>r?,_=cancelled(&mut controls)=>return Ok(())};
                text=tokio::select!{r=network::batch(&c,&p,"",&path)=>p.process(&r?),_=cancelled(&mut controls)=>return Ok(())};
            }
        }else{
            let (sender,receiver)=mpsc::channel(1024);
            let mut capture_task=tokio::spawn(capture(c.clone(),live,state.clone(),view.clone(),controls.clone(),sender,ready));
            if live{
                let result=tokio::select!{r=network::live(&c,&p,&launch.language,receiver,&view)=>Some(r),_=cancelled(&mut controls)=>None};
                if result.as_ref().is_none_or(|r|r.is_err())&& *controls.borrow()!=Control::Cancel{control.send_replace(Control::Stop);}
                let captured=(&mut capture_task).await.context("Capture task failed")?;
                if result.is_none(){return Ok(());}
                // Capture's detailed cause replaces only its generic network marker.
                // Genuine network errors keep priority over recorder shutdown errors.
                let raw=match result.unwrap(){
                    Err(error) if error.is::<network::CaptureFailed>()=>return Err(captured.err().unwrap_or(error)),
                    result=>result?,
                };
                let captured=captured?;duration=captured.duration;text=p.process(&raw);
            }else{
                // Batch capture does not send audio into this bounded channel.
                drop(receiver);
                let captured=tokio::select!{r=&mut capture_task=>r.context("Capture task failed")??,_=cancelled(&mut controls)=>{let _=capture_task.await;return Ok(());}};
                duration=captured.duration;
                if duration<c.millis("MIN_RECORDING_MS") || captured.bytes<c.millis("MIN_AUDIO_BYTES") || (c.yes("SKIP_SILENCE") && captured.db<c.number("SILENCE_MAX_DB")){storage::log(&c,"event=skipped reason=short_or_silent");return Ok(());}
                c=settings.lock().unwrap().clone();
                if !launch.language.is_empty() && c.get("OPENAI_LANGUAGES")==launch.config.get("OPENAI_LANGUAGES"){c.set("OPENAI_LANGUAGES",&launch.language);c.set("OPENAI_LANGUAGE","");}
                {let mut s=state.lock().unwrap();s.settings=c.snapshot();s.save()?;}
                phase(&state,&view,"processing")?;let path=state.lock().unwrap().audio_file.clone();
                let path=tokio::select!{r=audio::preprocess(&path,duration,&c)=>r?,_=cancelled(&mut controls)=>return Ok(())};
                text=tokio::select!{r=network::batch(&c,&p,"",&path)=>p.process(&r?),_=cancelled(&mut controls)=>return Ok(())};
            }
        }
        if *controls.borrow()==Control::Cancel{return Ok(());}
        if text.is_empty(){storage::log(&c,"event=skipped reason=empty_transcript");return Ok(());}
        transcript=Some(text.clone());storage::write_private(&c.path("LAST_TRANSCRIPT_FILE"),text.as_bytes())?;
        {let mut s=state.lock().unwrap();s.transcript=Some(text.clone());s.save()?;}
        // No await between checking cancellation and entering delivery. Controls now reject cancellation.
        if *controls.borrow()==Control::Cancel{return Ok(());}
        phase(&state,&view,"delivering")?;view.send_modify(|v|{v.transcript=text.clone();v.recording_ms=duration;});
        desktop::copy(&text).await.context("Clipboard failed; transcript saved for retry")?;
        if c.yes("AUTO_PASTE") {tokio::time::sleep(Duration::from_secs_f64(c.number("PASTE_DELAY"))).await;desktop::paste(&c).await.context("Automatic paste failed; transcript remains on clipboard")?;}
        if let Some(id)=&recovery_id{Recovery::discard(Some(id))?;}
        storage::log(&c,&format!("event=done transcript_chars={} recording_ms={duration}",text.chars().count()));Ok(())
    }.await;
    let cancelled = *controls.borrow() == Control::Cancel;
    let mut retained = false;
    if let Err(error) = &outcome
        && !cancelled
    {
        let s = state.lock().unwrap().clone();
        let source = retry_path.as_ref().unwrap_or(&s.audio_file);
        if source.is_file()
            && fs::metadata(source).is_ok_and(|m| m.len() > if live { 0 } else { 44 })
        {
            let reason = error
                .to_string()
                .replace(c.get("OPENAI_API_KEY"), "[redacted]");
            match Recovery::save(
                source,
                s.settings.clone(),
                s.recording_ms,
                transcript.clone().or(s.transcript),
                &reason,
                live.then_some(24000),
            ) {
                Ok(_) => {
                    storage::log(&c, &format!("event=failed error={reason} recovery=saved"));
                    desktop::notify(
                        "Dictate failed",
                        &format!("{reason}\nRecording saved: dictate batch retry"),
                    )
                    .await;
                }
                Err(e) => {
                    retained = true;
                    storage::log(
                        &c,
                        &format!(
                            "event=recovery_failed error={e} source={}",
                            source.display()
                        ),
                    );
                    desktop::notify(
                        "Dictate failed",
                        &format!("{reason}\nOriginal audio retained at {}", source.display()),
                    )
                    .await;
                }
            }
        } else {
            storage::log(&c, &format!("event=failed error={error}"));
            desktop::notify("Dictate failed", &error.to_string()).await;
        }
    }
    view.send_modify(|v| v.phase = "finished".into());
    if let Some(mut ducker) = ducker {
        control.send_replace(if cancelled {
            Control::Cancel
        } else {
            Control::Stop
        });
        let _ = timeout(Duration::from_secs(5), &mut ducker).await;
    }
    if !retained && !c.yes("KEEP_AUDIO") {
        let _ = fs::remove_dir_all(&directory);
    }
    if retained {
        // Keep interrupted-source metadata for the next recovery attempt.
        {
            let mut s = state.lock().unwrap();
            s.retain_source = true;
            s.save()?;
        }
        storage::log(&c, "event=source_retained");
    }
    if cancelled { Ok(()) } else { outcome }
}

async fn overlay(mut view: watch::Receiver<View>, c: Config) {
    let mut child = match Command::new(desktop::overlay_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(p) => p,
        Err(e) => {
            storage::log(&c, &format!("event=overlay_unavailable error={e}"));
            return;
        }
    };
    let mut input = child.stdin.take().unwrap();
    loop {
        let snapshot = view.borrow_and_update().clone();
        let data = match serde_json::to_vec(&snapshot) {
            Ok(v) => v,
            Err(_) => break,
        };
        if !matches!(
            timeout(Duration::from_millis(100), async {
                input.write_all(&data).await?;
                input.write_all(b"\n").await
            })
            .await,
            Ok(Ok(()))
        ) {
            break;
        }
        if snapshot.phase == "finished" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(16)).await;
        if view.changed().await.is_err() {
            break;
        }
    }
    drop(input);
    if timeout(Duration::from_millis(200), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}
/// Group a burst of desktop events into one refresh of the current monitor.
/// Reading during the debounce window avoids accumulating a stale event backlog.
async fn next_monitor_change<R: tokio::io::AsyncBufRead + Unpin>(
    lines: &mut tokio::io::Lines<R>,
) -> std::io::Result<bool> {
    while let Some(line) = lines.next_line().await? {
        if ![
            "activewindowv2",
            "focusedmon",
            "workspace",
            "movewindow",
            "monitor",
            "configreloaded",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        {
            continue;
        }
        let until = Instant::now() + Duration::from_millis(80);
        loop {
            if Instant::now() >= until {
                return Ok(true);
            }
            match tokio::time::timeout_at(until, lines.next_line()).await {
                Ok(Ok(Some(_))) => (),
                Ok(Err(error)) => return Err(error),
                Ok(Ok(None)) | Err(_) => return Ok(true),
            }
        }
    }
    Ok(false)
}

async fn track_monitor(view: watch::Sender<View>) {
    if let Some(name) = desktop::focused_monitor().await {
        view.send_modify(|v| v.monitor = name);
    }
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return;
    };
    let Some(signature) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") else {
        return;
    };
    let path = PathBuf::from(runtime)
        .join("hypr")
        .join(signature)
        .join(".socket2.sock");
    loop {
        if let Ok(socket) = UnixStream::connect(&path).await {
            let mut lines = BufReader::new(socket).lines();
            while let Ok(true) = next_monitor_change(&mut lines).await {
                if let Some(name) = desktop::focused_monitor().await {
                    view.send_if_modified(|v| {
                        if v.monitor == name {
                            false
                        } else {
                            v.monitor = name;
                            true
                        }
                    });
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn desktop_event_bursts_request_one_monitor_refresh() {
        let (input, mut output) = tokio::io::duplex(16 * 1024);
        let mut lines = BufReader::new(input).lines();
        output
            .write_all("activewindowv2>>0x1\nworkspace>>1\n".repeat(100).as_bytes())
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_secs(1), next_monitor_change(&mut lines))
                .await
                .unwrap()
                .unwrap()
        );
        assert!(
            timeout(Duration::from_millis(20), next_monitor_change(&mut lines))
                .await
                .is_err(),
            "the entire burst must be drained rather than replayed as stale monitor changes"
        );
        output.write_all(b"activewindowv2>>0x2\n").await.unwrap();
        drop(output);
        assert!(next_monitor_change(&mut lines).await.unwrap());
        assert!(!next_monitor_change(&mut lines).await.unwrap());
    }

    #[tokio::test]
    async fn unrelated_desktop_events_do_not_request_monitor_refresh() {
        let (input, mut output) = tokio::io::duplex(1024);
        output
            .write_all(b"windowtitle>>text\nurgent>>0x1\n")
            .await
            .unwrap();
        drop(output);
        assert!(
            !next_monitor_change(&mut BufReader::new(input).lines())
                .await
                .unwrap()
        );
    }
}
