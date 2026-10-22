pub mod config;
pub mod menu;
use crate::{
    audio::{self, AudioAction, AudioState},
    bluetooth::{self, BluetoothAction, BluetoothState},
    monitors::{self, DisplayState, Position},
    power::{self, PowerState},
    profiles::Preferences,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot, watch},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Layout { position: Position },
    Cycle,
    Audio { action: AudioAction },
    Bluetooth { action: BluetoothAction },
    Suspend,
    Rescan,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub displays: DisplayState,
    pub audio: AudioState,
    pub bluetooth: BluetoothState,
    #[serde(default)]
    pub power: PowerState,
    pub busy: Option<String>,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status,
    Subscribe,
    Act { action: Action },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub error: Option<String>,
    pub snapshot: Snapshot,
}
pub enum Event {
    Displays,
    Audio,
    Bluetooth,
    Resume,
    Command(Action, oneshot::Sender<Result<()>>),
    BluetoothDone(Result<()>),
    PowerDone(Result<()>),
}
pub fn socket_path() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR missing")?)
            .join("desktop-ring/control.sock"),
    )
}
pub async fn client(req: &Request) -> Result<Response> {
    tokio::time::timeout(std::time::Duration::from_secs(12), async {
        let mut stream = UnixStream::connect(socket_path()?)
            .await
            .context("desktop-ring service is unavailable")?;
        stream.write_all(&serde_json::to_vec(req)?).await?;
        stream.write_all(b"\n").await?;
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).await?;
        Ok(serde_json::from_str(&line)?)
    })
    .await
    .context("desktop-ring request timed out")?
}
async fn serve(
    stream: UnixStream,
    mut state: watch::Receiver<Snapshot>,
    tx: mpsc::Sender<Event>,
) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    use tokio::io::AsyncReadExt;
    let n = BufReader::new(read.take(65536))
        .read_line(&mut line)
        .await?;
    ensure!(n > 0 && n < 65536, "invalid request size");
    let req: Request = serde_json::from_str(&line)?;
    let subscription = matches!(req, Request::Subscribe);
    let error = if let Request::Act { action } = req {
        let (reply, rx) = oneshot::channel();
        tx.send(Event::Command(action, reply)).await?;
        rx.await?.err().map(|e| e.to_string())
    } else {
        None
    };
    loop {
        let response = Response {
            ok: error.is_none(),
            error: error.clone(),
            snapshot: state.borrow_and_update().clone(),
        };
        let mut data = serde_json::to_vec(&response)?;
        data.push(b'\n');
        write.write_all(&data).await?;
        if !subscription {
            return Ok(());
        }
        state.changed().await?;
    }
}
struct Manager {
    config: config::Config,
    state: Snapshot,
    preferences: Preferences,
    preferences_error: Option<String>,
    rejected: Option<String>,
    tx: mpsc::Sender<Event>,
    bt: Option<zbus::Connection>,
}
impl Manager {
    async fn displays(&mut self) -> Result<()> {
        let ms = monitors::ipc::discover().await?;
        self.state.displays.monitors = ms.clone();
        let setup = monitors::setup_key(&ms);
        let position = setup
            .as_ref()
            .and_then(|k| self.preferences.layouts.get(k))
            .copied()
            .unwrap_or_default();
        let switchable = self.config.laptop
            && ms.len() == 2
            && ms.iter().filter(|m| m.internal()).count() == 1
            && setup.is_some();
        let fixed = monitors::fixed_plan(&self.config.fixed_profiles, &ms);
        let (profile, rules) = match fixed {
            Ok(Some(p)) => p,
            Ok(None) if switchable => (
                "Laptop + external".into(),
                monitors::configured_laptop_plan(
                    &ms,
                    position,
                    self.config.internal_scale,
                    &self.config.laptop_preferences,
                )?,
            ),
            Ok(None) if self.config.laptop && ms.len() == 1 && ms[0].internal() => {
                let mut rule = monitors::current_rule(&ms[0]);
                rule.scale = self.config.internal_scale;
                ("Laptop panel".into(), vec![rule])
            }
            Ok(None) => ("Safe fallback".into(), monitors::fallback_plan(&ms)),
            Err(e) => {
                self.state.displays = DisplayState {
                    monitors: ms,
                    profile: "Unapplied profile".into(),
                    setup,
                    diagnostic: Some(e.to_string()),
                    ..Default::default()
                };
                return Ok(());
            }
        };
        let fingerprint = serde_json::to_string(&(&setup, &rules))?;
        let mut diagnostic = self.preferences_error.clone();
        if setup.is_none() && ms.len() > 1 {
            diagnostic=Some("Displays have indistinguishable identifiers; preserving their layout. Add unique EDIDs/serials before saving preferences.".into());
        }
        if self.rejected.as_ref() == Some(&fingerprint) {
            diagnostic = self.state.displays.diagnostic.clone().or(diagnostic);
        }
        if self.rejected.as_ref() != Some(&fingerprint)
            && !rules.is_empty()
            && let Err(e) = monitors::ipc::apply(&rules, &ms).await
        {
            diagnostic = Some(e.to_string());
            self.rejected = Some(fingerprint);
        }
        self.state.displays = DisplayState {
            monitors: monitors::ipc::discover().await.unwrap_or(ms),
            profile,
            setup,
            position: if switchable { Some(position) } else { None },
            switchable,
            diagnostic,
        };
        Ok(())
    }
    async fn audio(&mut self) {
        match audio::discover().await {
            Ok(a) => self.state.audio = a,
            Err(e) => {
                self.state.audio = AudioState {
                    error: Some(e.to_string()),
                    ..Default::default()
                };
            }
        }
    }
    async fn bluetooth(&mut self) {
        if self.bt.as_ref().is_some_and(|c| c.is_closed()) {
            self.bt = None;
        }
        if self.bt.is_none() {
            self.bt = zbus::Connection::system().await.ok();
        }
        match &self.bt {
            Some(conn) => match bluetooth::discover(conn).await {
                Ok(b) => self.state.bluetooth = b,
                Err(e) => {
                    self.state.bluetooth = BluetoothState {
                        error: Some(e.to_string()),
                        ..Default::default()
                    };
                }
            },
            None => {
                self.state.bluetooth = BluetoothState {
                    error: Some("System D-Bus unavailable".into()),
                    ..Default::default()
                };
            }
        }
    }
    async fn action(&mut self, action: Action) -> Result<()> {
        self.state.message = None;
        match action {
            Action::Layout { position } => {
                ensure!(
                    self.config.monitors && self.state.displays.switchable,
                    "this monitor setup has one fixed arrangement"
                );
                ensure!(
                    self.preferences_error.is_none(),
                    "repair the preferences file before saving a new layout"
                );
                let ms = monitors::ipc::discover().await?;
                let setup = monitors::setup_key(&ms)
                    .context("ambiguous monitor identities; cannot save layout")?;
                ensure!(
                    self.state.displays.setup.as_ref() == Some(&setup),
                    "displays changed; select the layout again"
                );
                let plan = monitors::configured_laptop_plan(
                    &ms,
                    position,
                    self.config.internal_scale,
                    &self.config.laptop_preferences,
                )?;
                monitors::ipc::apply(&plan, &ms).await?;
                let mut preferences = self.preferences.clone();
                preferences.version = 1;
                preferences.layouts.insert(setup, position);
                preferences.save()?;
                self.preferences = preferences;
                self.rejected = None;
                self.displays().await?;
            }
            Action::Cycle => {
                let position = self
                    .state
                    .displays
                    .position
                    .context("no laptop layouts for this setup")?
                    .next();
                Box::pin(self.action(Action::Layout { position })).await?;
            }
            Action::Audio { action } => {
                ensure!(self.config.audio, "audio module disabled");
                self.audio().await;
                audio::act(&action, &self.state.audio, self.config.move_streams).await?;
                self.audio().await;
            }
            Action::Bluetooth { action } => {
                ensure!(self.config.bluetooth, "Bluetooth module disabled");
                ensure!(
                    self.state.busy.is_none(),
                    "a Bluetooth operation is already in progress"
                );
                let conn = self.bt.clone().context("Bluetooth D-Bus unavailable")?;
                let state = self.state.bluetooth.clone();
                let tx = self.tx.clone();
                self.state.busy = Some(match &action {
                    BluetoothAction::Connect { path }
                    | BluetoothAction::Disconnect { path }
                    | BluetoothAction::Power { path, .. } => path.clone(),
                });
                tokio::spawn(async move {
                    let result = bluetooth::act(&conn, &action, &state).await;
                    let _ = tx.send(Event::BluetoothDone(result)).await;
                });
            }
            Action::Rescan => {
                let new = config::load()?;
                ensure!(
                    (new.monitors, new.audio, new.bluetooth)
                        == (
                            self.config.monitors,
                            self.config.audio,
                            self.config.bluetooth
                        ),
                    "module changes require restarting desktop-ring.service"
                );
                self.config = new;
                self.rejected = None;
                if self.config.monitors {
                    self.displays().await?;
                }
                if self.config.audio {
                    self.audio().await;
                }
                if self.config.bluetooth {
                    self.bluetooth().await;
                }
                self.state.power = if self.config.power {
                    power::discover().await
                } else {
                    PowerState::default()
                };
            }
            Action::Suspend => {
                ensure!(self.config.power, "system controls disabled");
                ensure!(
                    self.state.busy.is_none(),
                    "a device operation is already in progress"
                );
                power::prepare().await?;
                let tx = self.tx.clone();
                self.state.busy = Some("sleep".into());
                tokio::spawn(async move {
                    let _ = tx.send(Event::PowerDone(power::suspend().await)).await;
                });
            }
        }
        Ok(())
    }
}
pub async fn daemon(config: config::Config) -> Result<()> {
    let path = socket_path()?;
    let parent = path.parent().unwrap();
    std::fs::create_dir_all(parent)?;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    if path.exists() {
        ensure!(
            UnixStream::connect(&path).await.is_err(),
            "desktop-ring is already running"
        );
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let (tx, mut rx) = mpsc::channel(64);
    let (updates, state) = watch::channel(Snapshot::default());
    let preferences = Preferences::load();
    let preferences_error = preferences
        .as_ref()
        .err()
        .map(|e| format!("Preferences: {e}"));
    if let Some(e) = &preferences_error {
        eprintln!("{e}");
    }
    let mut manager = Manager {
        config: config.clone(),
        state: Snapshot::default(),
        preferences: preferences.unwrap_or_default(),
        preferences_error,
        rejected: None,
        tx: tx.clone(),
        bt: None,
    };
    if config.power {
        manager.state.power = power::discover().await;
        updates.send_replace(manager.state.clone());
    }
    if config.monitors {
        tokio::spawn(monitors::ipc::watch(tx.clone()));
    }
    if config.monitors || config.power {
        tokio::spawn(bluetooth::resume_watch(tx.clone()));
    }
    if config.audio {
        tokio::spawn(audio::watch(tx.clone()));
    }
    if config.bluetooth {
        tokio::spawn(bluetooth::watch(tx.clone()));
    }
    let accept_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let tx = accept_tx.clone();
                    let state = state.clone();
                    tokio::spawn(async move {
                        if let Err(e) = serve(stream, state, tx).await {
                            eprintln!("client: {e}");
                        }
                    });
                }
                Err(e) => {
                    eprintln!("control socket: {e}");
                    return;
                }
            }
        }
    });
    let stop = tokio::signal::ctrl_c();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            _=&mut stop=>break,
            Some(event)=rx.recv()=>{
                match event {
                    Event::Displays=>{if manager.config.monitors&& let Err(e)=manager.displays().await {manager.state.displays.diagnostic=Some(e.to_string());eprintln!("displays: {e}");}},
                    Event::Audio=>{if manager.config.audio{manager.audio().await;}},
                    Event::Bluetooth=>{if manager.config.bluetooth{manager.bluetooth().await;}},
                    Event::Resume=>{
                        manager.rejected=None;
                        if manager.config.monitors{let _=manager.displays().await;}
                        if manager.config.audio{manager.audio().await;}
                        if manager.config.bluetooth{manager.bluetooth().await;}
                        if manager.config.power{manager.state.power=power::discover().await;}
                    },
                    Event::Command(action,reply)=>{
                        let result=manager.action(action).await;
                        if let Err(e)=&result{manager.state.message=Some(e.to_string());eprintln!("action: {e}");}
                        updates.send_replace(manager.state.clone());let _=reply.send(result);
                    },
                    Event::BluetoothDone(result)=>{
                        manager.state.busy=None;
                        manager.state.message=Some(match result{Ok(())=>"Bluetooth updated. Audio devices appear separately in Audio Output / Microphone.".into(),Err(e)=>format!("Bluetooth: {e}")});
                        manager.bluetooth().await;if manager.config.audio{manager.audio().await;}
                    },
                    Event::PowerDone(result)=>{
                        manager.state.busy=None;
                        manager.state.message=result.err().map(|e|format!("Sleep: {e}"));
                        if let Some(message)=manager.state.message.clone(){
                            eprintln!("{message}");
                            tokio::spawn(async move {
                                let _=tokio::process::Command::new("notify-send")
                                    .args(["--app-name=Desktop Ring","--urgency=critical","Sleep failed",&message])
                                    .kill_on_drop(true).output().await;
                            });
                        }
                    }
                }
                let old=serde_json::to_vec(&*updates.borrow())?;let new=serde_json::to_vec(&manager.state)?;
                if old!=new{updates.send_replace(manager.state.clone());}
            }
        }
    }
    std::fs::remove_file(path)?;
    Ok(())
}
