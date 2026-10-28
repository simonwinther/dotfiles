//! One libpulse context for discovery, actions, and subscriptions. Only this
//! worker and libpulse's locked event thread ever access native objects.
use super::{AudioAction, AudioState, Device};
use anyhow::{Context as _, Result, bail, ensure};
use libpulse_binding::{
    callbacks::ListResult,
    context::{Context, FlagSet, State, subscribe::InterestMaskSet},
    def::PortAvailable,
    mainloop::threaded::Mainloop,
    operation::Operation,
    proplist::Proplist,
    volume::{ChannelVolumes, Volume},
};
use std::{
    collections::VecDeque,
    fmt,
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

const TIMEOUT: Duration = Duration::from_secs(5);

pub struct Client {
    tx: mpsc::Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

enum Command {
    Discover,
    Act(AudioAction, AudioState, bool),
}
enum Answer {
    State(AudioState),
    Done,
}
struct Request {
    command: Command,
    deadline: Instant,
    reply: oneshot::Sender<Result<Answer>>,
}
enum Message {
    Request(Request),
    ContextChanged,
    Complete(u64, Reply),
    Stop,
}
enum Reply {
    Success(bool),
    Defaults(String, String),
    Devices(Vec<Device>),
    Streams(Vec<u32>),
    Muted(bool),
    Failed,
}

// Protocol timeouts and lost connections discard the entire context. A failed
// device operation alone does not: its state must still be read back.
#[derive(Debug)]
struct Disconnected(String);
impl fmt::Display for Disconnected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Disconnected {}

impl Client {
    pub fn new(changes: watch::Sender<bool>) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let worker_tx = tx.clone();
        let worker = thread::Builder::new()
            .name("ring-audio".into())
            .spawn(move || {
                run(
                    Mailbox {
                        tx: worker_tx,
                        rx,
                        pending: VecDeque::new(),
                        stopping: false,
                        next_id: 0,
                    },
                    changes,
                );
            })?;
        Ok(Self {
            tx,
            worker: Some(worker),
        })
    }

    async fn request(&self, command: Command) -> Result<Answer> {
        let (reply, result) = oneshot::channel();
        self.tx
            .send(Message::Request(Request {
                command,
                deadline: Instant::now() + TIMEOUT,
                reply,
            }))
            .context("audio worker stopped")?;
        tokio::time::timeout(TIMEOUT + Duration::from_secs(1), result)
            .await
            .context("audio worker timed out")?
            .context("audio worker stopped")?
    }

    pub async fn discover(&self) -> Result<AudioState> {
        match self.request(Command::Discover).await? {
            Answer::State(state) => Ok(state),
            Answer::Done => unreachable!(),
        }
    }

    pub async fn act(
        &self,
        action: &AudioAction,
        state: &AudioState,
        move_streams: bool,
    ) -> Result<()> {
        self.request(Command::Act(action.clone(), state.clone(), move_streams))
            .await?;
        Ok(())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Stop is also received during connection setup and operation waits.
        let _ = self.tx.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Mailbox {
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    pending: VecDeque<Request>,
    stopping: bool,
    next_id: u64,
}
impl Mailbox {
    fn receive(&mut self, deadline: Instant) -> Result<Message> {
        // A queued message must not extend a deadline indefinitely.
        ensure!(
            Instant::now() < deadline,
            Disconnected("PipeWire request timed out".into())
        );
        match self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.stopping = true;
                bail!("audio worker stopped")
            }
            Ok(message) => Ok(message),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err(Disconnected("PipeWire request timed out".into()).into())
            }
        }
    }

    fn unavailable(&mut self, until: Instant) {
        for request in self.pending.drain(..) {
            let _ = request
                .reply
                .send(Err(anyhow::anyhow!("audio server unavailable")));
        }
        while Instant::now() < until && !self.stopping {
            match self.receive(until) {
                Ok(Message::Request(request)) => {
                    let _ = request
                        .reply
                        .send(Err(anyhow::anyhow!("audio server unavailable")));
                }
                Err(_) => break,
                _ => {}
            }
        }
    }
}

struct Connection {
    // Context must be destroyed while the mainloop is locked, before stopping it.
    context: Option<Context>,
    mainloop: Mainloop,
}
impl Connection {
    fn locked<T>(&mut self, f: impl FnOnce(&mut Context) -> T) -> T {
        self.mainloop.lock();
        let result = f(self.context.as_mut().unwrap());
        self.mainloop.unlock();
        result
    }

    fn ready(&mut self) -> Result<()> {
        self.locked(|context| {
            if context.get_state() != State::Ready {
                return Err(
                    Disconnected(format!("audio connection lost: {}", context.errno())).into(),
                );
            }
            Ok(())
        })
    }

    fn connect(mail: &mut Mailbox, changes: &watch::Sender<bool>) -> Result<Self> {
        let mainloop = Mainloop::new().context("cannot create audio mainloop")?;
        let mut context = Context::new(&mainloop, "Desktop Ring")
            .context("cannot create libpulse context (PulseAudio 15 or newer required)")?;
        let tx = mail.tx.clone();
        let health = changes.clone();
        context.set_state_callback(Some(Box::new(move || {
            let _ = health.send(false);
            let _ = tx.send(Message::ContextChanged);
        })));
        let events = changes.clone();
        context.set_subscribe_callback(Some(Box::new(move |_, _, _| {
            let _ = events.send(true);
        })));
        let mut connection = Self {
            context: Some(context),
            mainloop,
        };
        // Never autospawn or change the lifetime of the system's audio server.
        connection.locked(|context| context.connect(None, FlagSet::NOAUTOSPAWN, None))?;
        connection.mainloop.start()?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match connection.locked(|context| context.get_state()) {
                State::Ready => break,
                State::Failed | State::Terminated => {
                    connection.ready()?;
                }
                _ => {}
            }
            if let Message::Request(request) = mail.receive(deadline)? {
                mail.pending.push_back(request);
            }
        }
        connection.call(mail, deadline, |context, tx, id| {
            context.subscribe(
                InterestMaskSet::SERVER
                    | InterestMaskSet::SINK
                    | InterestMaskSet::SOURCE
                    | InterestMaskSet::CARD,
                success(tx, id),
            )
        })?;
        // Subscription acknowledgement precedes the initial snapshot.
        let _ = changes.send(true);
        Ok(connection)
    }

    fn call<T: ?Sized>(
        &mut self,
        mail: &mut Mailbox,
        deadline: Instant,
        create: impl FnOnce(&mut Context, mpsc::Sender<Message>, u64) -> Operation<T>,
    ) -> Result<Reply> {
        ensure!(
            Instant::now() < deadline,
            Disconnected("PipeWire request timed out".into())
        );
        let id = mail.next_id;
        mail.next_id = mail.next_id.wrapping_add(1);
        let mut operation = self.locked(|context| {
            ensure!(
                context.get_state() == State::Ready,
                Disconnected("audio connection lost".into())
            );
            Ok(create(context, mail.tx.clone(), id))
        })?;
        let result = self.wait(mail, deadline, id);
        self.mainloop.lock();
        if operation.get_state() != libpulse_binding::operation::State::Done {
            operation.cancel();
        }
        drop(operation);
        self.mainloop.unlock();
        match result? {
            Reply::Success(false) | Reply::Failed => {
                bail!("PipeWire: {}", self.locked(|context| context.errno()))
            }
            reply => Ok(reply),
        }
    }

    fn wait(&mut self, mail: &mut Mailbox, deadline: Instant, id: u64) -> Result<Reply> {
        loop {
            match mail.receive(deadline)? {
                Message::Complete(completed, reply) if completed == id => return Ok(reply),
                Message::ContextChanged => self.ready()?,
                Message::Request(request) => mail.pending.push_back(request),
                _ => {}
            }
        }
    }

    fn discover(&mut self, mail: &mut Mailbox, deadline: Instant) -> Result<AudioState> {
        let Reply::Defaults(output, input) = self.call(mail, deadline, |context, tx, id| {
            context.introspect().get_server_info(move |info| {
                let _ = tx.send(Message::Complete(
                    id,
                    Reply::Defaults(
                        info.default_sink_name.as_deref().unwrap_or("").into(),
                        info.default_source_name.as_deref().unwrap_or("").into(),
                    ),
                ));
            })
        })?
        else {
            unreachable!()
        };
        let mut outputs = Vec::new();
        let Reply::Devices(mut sinks) = self.call(mail, deadline, |context, tx, id| {
            context.introspect().get_sink_info_list(move |row| {
                let reply = match row {
                    ListResult::Item(info) => {
                        outputs.push(describe(
                            info.name.as_deref().unwrap_or(""),
                            info.description.as_deref().unwrap_or(""),
                            &info.proplist,
                            &info.volume,
                            info.mute,
                            info.active_port
                                .as_ref()
                                .is_none_or(|p| p.available != PortAvailable::No),
                        ));
                        return;
                    }
                    ListResult::End => Reply::Devices(std::mem::take(&mut outputs)),
                    ListResult::Error => Reply::Failed,
                };
                let _ = tx.send(Message::Complete(id, reply));
            })
        })?
        else {
            unreachable!()
        };
        let mut inputs = Vec::new();
        let Reply::Devices(mut sources) = self.call(mail, deadline, |context, tx, id| {
            context.introspect().get_source_info_list(move |row| {
                let reply = match row {
                    ListResult::Item(info) => {
                        let name = info.name.as_deref().unwrap_or("");
                        if visible_input(name, &info.proplist, info.monitor_of_sink.is_some()) {
                            inputs.push(describe(
                                name,
                                info.description.as_deref().unwrap_or(""),
                                &info.proplist,
                                &info.volume,
                                info.mute,
                                info.active_port
                                    .as_ref()
                                    .is_none_or(|p| p.available != PortAvailable::No),
                            ));
                        }
                        return;
                    }
                    ListResult::End => Reply::Devices(std::mem::take(&mut inputs)),
                    ListResult::Error => Reply::Failed,
                };
                let _ = tx.send(Message::Complete(id, reply));
            })
        })?
        else {
            unreachable!()
        };
        for (devices, default) in [(&mut sinks, output), (&mut sources, input)] {
            for device in devices.iter_mut() {
                device.active = device.name == default;
            }
            devices.sort_by(|a, b| a.label.cmp(&b.label));
        }
        Ok(AudioState {
            outputs: sinks,
            inputs: sources,
            error: None,
        })
    }

    fn act(
        &mut self,
        mail: &mut Mailbox,
        deadline: Instant,
        action: &AudioAction,
        state: &AudioState,
        move_streams: bool,
    ) -> Result<()> {
        let (AudioAction::Select { input, .. }
        | AudioAction::Volume { input, .. }
        | AudioAction::Mute { input }) = action;
        let devices = if *input {
            &state.inputs
        } else {
            &state.outputs
        };
        let device = match action {
            AudioAction::Select { name, .. } => {
                let device = devices
                    .iter()
                    .find(|d| d.name == *name)
                    .context("audio device disconnected")?;
                ensure!(device.available, "audio route is unavailable");
                device
            }
            _ => devices
                .iter()
                .find(|d| d.active)
                .context("no default audio device")?,
        };
        ensure!(
            !device.name.is_empty() && !device.name.contains('\0'),
            "invalid audio device name"
        );
        if matches!(action, AudioAction::Volume { .. }) {
            ensure!(
                (1..=32).contains(&device.channels),
                "invalid audio channel count"
            );
        }
        // Match pactl's toggle behavior using the server's current mute value,
        // including a change that races the subscription's cached snapshot.
        let muted = if matches!(action, AudioAction::Mute { .. }) {
            self.current_mute(mail, deadline, *input, &device.name)?
        } else {
            device.muted
        };
        self.call(mail, deadline, |context, tx, id| {
            let done = success(tx, id);
            match action {
                AudioAction::Select { input: true, .. } => {
                    context.set_default_source(&device.name, done)
                }
                AudioAction::Select { input: false, .. } => {
                    context.set_default_sink(&device.name, done)
                }
                AudioAction::Mute { input: true } => context.introspect().set_source_mute_by_name(
                    &device.name,
                    !muted,
                    Some(Box::new(done)),
                ),
                AudioAction::Mute { input: false } => context.introspect().set_sink_mute_by_name(
                    &device.name,
                    !muted,
                    Some(Box::new(done)),
                ),
                AudioAction::Volume { input, delta } => {
                    let percent = (i64::from(device.volume) + i64::from(*delta)).clamp(0, 100);
                    let mut volume = ChannelVolumes::default();
                    volume.set(
                        device.channels,
                        Volume(
                            (percent as f64 / 100.0 * f64::from(Volume::NORMAL.0)).round() as u32,
                        ),
                    );
                    if *input {
                        context.introspect().set_source_volume_by_name(
                            &device.name,
                            &volume,
                            Some(Box::new(done)),
                        )
                    } else {
                        context.introspect().set_sink_volume_by_name(
                            &device.name,
                            &volume,
                            Some(Box::new(done)),
                        )
                    }
                }
            }
        })?;
        if move_streams && matches!(action, AudioAction::Select { .. }) {
            self.move_streams(mail, deadline, *input, &device.name)?;
        }
        Ok(())
    }

    fn current_mute(
        &mut self,
        mail: &mut Mailbox,
        deadline: Instant,
        input: bool,
        name: &str,
    ) -> Result<bool> {
        let mut muted = None;
        let reply = if input {
            self.call(mail, deadline, |context, tx, id| {
                context
                    .introspect()
                    .get_source_info_by_name(name, move |row| {
                        let reply = match row {
                            ListResult::Item(info) => {
                                muted = Some(info.mute);
                                return;
                            }
                            ListResult::End => muted.map(Reply::Muted).unwrap_or(Reply::Failed),
                            ListResult::Error => Reply::Failed,
                        };
                        let _ = tx.send(Message::Complete(id, reply));
                    })
            })?
        } else {
            self.call(mail, deadline, |context, tx, id| {
                context
                    .introspect()
                    .get_sink_info_by_name(name, move |row| {
                        let reply = match row {
                            ListResult::Item(info) => {
                                muted = Some(info.mute);
                                return;
                            }
                            ListResult::End => muted.map(Reply::Muted).unwrap_or(Reply::Failed),
                            ListResult::Error => Reply::Failed,
                        };
                        let _ = tx.send(Message::Complete(id, reply));
                    })
            })?
        };
        let Reply::Muted(muted) = reply else {
            unreachable!()
        };
        Ok(muted)
    }

    fn move_streams(
        &mut self,
        mail: &mut Mailbox,
        deadline: Instant,
        input: bool,
        name: &str,
    ) -> Result<()> {
        let mut streams = Vec::new();
        let reply = if input {
            self.call(mail, deadline, |context, tx, id| {
                context
                    .introspect()
                    .get_source_output_info_list(move |row| {
                        let reply = match row {
                            ListResult::Item(info) => {
                                if movable(&info.proplist) {
                                    streams.push(info.index);
                                }
                                return;
                            }
                            ListResult::End => Reply::Streams(std::mem::take(&mut streams)),
                            ListResult::Error => Reply::Failed,
                        };
                        let _ = tx.send(Message::Complete(id, reply));
                    })
            })?
        } else {
            self.call(mail, deadline, |context, tx, id| {
                context.introspect().get_sink_input_info_list(move |row| {
                    let reply = match row {
                        ListResult::Item(info) => {
                            if movable(&info.proplist) {
                                streams.push(info.index);
                            }
                            return;
                        }
                        ListResult::End => Reply::Streams(std::mem::take(&mut streams)),
                        ListResult::Error => Reply::Failed,
                    };
                    let _ = tx.send(Message::Complete(id, reply));
                })
            })?
        };
        let Reply::Streams(streams) = reply else {
            unreachable!()
        };
        for index in streams {
            let result = self.call(mail, deadline, |context, tx, id| {
                let done = Some(Box::new(success(tx, id)) as Box<dyn FnMut(bool)>);
                if input {
                    context
                        .introspect()
                        .move_source_output_by_name(index, name, done)
                } else {
                    context
                        .introspect()
                        .move_sink_input_by_name(index, name, done)
                }
            });
            if let Err(error) = result {
                if error.is::<Disconnected>() || mail.stopping {
                    return Err(error);
                }
                // A stream can exit between enumeration and the move.
                eprintln!("stream {index}: {error}");
            }
        }
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.mainloop.lock();
        if let Some(mut context) = self.context.take() {
            context.set_state_callback(None);
            context.set_subscribe_callback(None);
            context.disconnect();
            drop(context);
        }
        self.mainloop.unlock();
        self.mainloop.stop();
    }
}

fn run(mut mail: Mailbox, changes: watch::Sender<bool>) {
    let mut backoff = 1;
    while !mail.stopping {
        let result = Connection::connect(&mut mail, &changes).and_then(|mut connection| {
            backoff = 1;
            loop {
                let message = if let Some(request) = mail.pending.pop_front() {
                    Message::Request(request)
                } else {
                    match mail.rx.recv() {
                        Ok(message) => message,
                        Err(_) => {
                            mail.stopping = true;
                            break;
                        }
                    }
                };
                match message {
                    Message::Stop => {
                        mail.stopping = true;
                        break;
                    }
                    Message::ContextChanged => connection.ready()?,
                    Message::Request(request) => {
                        if request.reply.is_closed() {
                            continue;
                        }
                        let result = if Instant::now() >= request.deadline {
                            Err(anyhow::anyhow!("audio request expired before execution"))
                        } else {
                            match request.command {
                                Command::Discover => connection
                                    .discover(&mut mail, request.deadline)
                                    .map(Answer::State),
                                Command::Act(action, state, moves) => connection
                                    .act(&mut mail, request.deadline, &action, &state, moves)
                                    .map(|_| Answer::Done),
                            }
                        };
                        let disconnected = result
                            .as_ref()
                            .err()
                            .is_some_and(|e| e.is::<Disconnected>());
                        let _ = request.reply.send(result);
                        if disconnected || mail.stopping {
                            break;
                        }
                    }
                    Message::Complete(_, _) => {}
                }
            }
            Ok(())
        });
        let _ = changes.send(false);
        if mail.stopping {
            break;
        }
        eprintln!("audio: {result:?}; reconnect in {backoff}s");
        mail.unavailable(Instant::now() + Duration::from_secs(backoff));
        backoff = (backoff * 2).min(30);
    }
}

fn success(tx: mpsc::Sender<Message>, id: u64) -> impl FnMut(bool) {
    move |ok| {
        let _ = tx.send(Message::Complete(id, Reply::Success(ok)));
    }
}

fn visible_input(name: &str, props: &Proplist, monitor: bool) -> bool {
    !monitor
        && !name.ends_with(".monitor")
        && !name.starts_with("alsa_input.hw_")
        && props.get_str("device.class").as_deref() != Some("monitor")
}
fn movable(props: &Proplist) -> bool {
    props.get_str("media.role").as_deref() != Some("filter")
        && props.get_str("node.virtual").as_deref() != Some("true")
}
fn describe(
    name: &str,
    label: &str,
    props: &Proplist,
    volume: &ChannelVolumes,
    muted: bool,
    available: bool,
) -> Device {
    let persistent = [
        "device.serial",
        "api.bluez5.address",
        "device.bus-id",
        "device.bus_path",
    ]
    .iter()
    .find_map(|key| props.get_str(key))
    .unwrap_or_else(|| name.into());
    let profile = props.get_str("device.profile.name").unwrap_or_default();
    let nick = props.get_str("node.nick").unwrap_or_default();
    let profile_label = props
        .get_str("device.profile.description")
        .unwrap_or_default();
    Device {
        name: name.into(),
        identity: serde_json::to_string(&(persistent, profile, name)).unwrap(),
        label: label.into(),
        short_label: if label.chars().count() > 32 && !nick.is_empty() && !profile_label.is_empty()
        {
            format!("{nick}\n{profile_label}")
        } else {
            label.into()
        },
        available,
        active: false,
        muted,
        volume: (f64::from(volume.max().0) / f64::from(Volume::NORMAL.0) * 100.0).round() as u32,
        channels: volume.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_devices_preserve_identity_labels_availability_and_channel_volume() {
        let mut props = Proplist::new().unwrap();
        props.set_str("device.serial", "serial-1").unwrap();
        props
            .set_str("device.profile.name", "analog-stereo")
            .unwrap();
        props.set_str("node.nick", "USB Audio").unwrap();
        props
            .set_str("device.profile.description", "Analog Stereo")
            .unwrap();
        let mut volume = ChannelVolumes::default();
        volume.set(2, Volume(32768));
        let first = describe(
            "usb-stable",
            "A very long description for the USB audio device",
            &props,
            &volume,
            true,
            false,
        );
        assert_eq!(first.volume, 50);
        assert_eq!(first.channels, 2);
        assert!(first.muted && !first.available);
        assert_eq!(first.short_label, "USB Audio\nAnalog Stereo");
        assert_eq!(
            first.identity,
            "[\"serial-1\",\"analog-stereo\",\"usb-stable\"]"
        );
        // Temporary server indices are absent from the device model.
        let second = describe("usb-stable", "New label", &props, &volume, false, true);
        assert_eq!(first.identity, second.identity);
        assert!(second.available);
    }

    #[test]
    fn native_monitor_sources_and_internal_streams_are_filtered() {
        let mut props = Proplist::new().unwrap();
        assert!(visible_input("mic", &props, false));
        assert!(!visible_input("mic", &props, true));
        assert!(!visible_input("speaker.monitor", &props, false));
        assert!(!visible_input("alsa_input.hw_0", &props, false));
        props.set_str("device.class", "monitor").unwrap();
        assert!(!visible_input("mic", &props, false));
        assert!(movable(&props));
        props.set_str("media.role", "filter").unwrap();
        assert!(!movable(&props));
        props.set_str("media.role", "music").unwrap();
        props.set_str("node.virtual", "true").unwrap();
        assert!(!movable(&props));
    }
}
