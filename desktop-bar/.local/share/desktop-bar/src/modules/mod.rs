pub mod audio;
pub mod battery;
pub mod clock;
pub mod cpu;
pub mod disk;
pub mod media;
pub mod mem;
pub mod network;
pub mod tray;
pub mod weather;
pub mod workspaces;

#[derive(Clone, Debug, Default)]
pub struct Data {
    pub cpu: Vec<f32>,
    pub memory: Option<mem::Memory>,
    pub disk: Option<disk::Disk>,
    pub battery: Option<battery::Battery>,
    pub audio: audio::Audio,
    pub microphone: audio::Audio,
    pub network: network::Network,
    pub media: media::Media,
    pub weather: weather::Weather,
    pub tray: Vec<tray::Item>,
}

pub enum Update {
    Command(String, Option<String>),
    Shell(crate::shell::Reply),
    Prompt(crate::prompt::Request),
    Picker(std::sync::Arc<crate::picker::Request>),
    Cpu(Vec<f32>),
    Memory(Option<mem::Memory>),
    Disk(Option<disk::Disk>),
    Battery(Option<battery::Battery>),
    Audio(bool, audio::Audio),
    Network(network::Network),
    Media(media::Media),
    MediaArt(String, Option<std::sync::Arc<tiny_skia::Pixmap>>),
    NetworkSignals(std::collections::HashMap<String, i16>),
    Weather(weather::Weather),
    Tray(Vec<tray::Item>),
    Workspaces(String, Option<workspaces::Snapshot>),
}

/// Starts or stops the CPU, memory and disk samples that only detailed views
/// draw. Battery and Wi-Fi signal samples continue for the compact notch.
pub struct Metrics {
    detailed: bool,
    demand: std::sync::mpsc::Sender<bool>,
}
impl Metrics {
    pub fn show_detailed(&mut self, visible: bool) {
        if self.detailed != visible {
            self.detailed = visible;
            let _ = self.demand.send(visible);
        }
    }
}

/// Per-core CPU usage over the interval since the previous sample.
#[derive(Default)]
struct CpuSampler {
    previous: Vec<cpu::Times>,
    taken: Option<std::time::Instant>,
}
impl CpuSampler {
    fn sample(&mut self, now: std::time::Instant) -> Vec<f32> {
        let current = cpu::parse(&std::fs::read_to_string("/proc/stat").unwrap_or_default());
        let usage = cpu::usage(&self.previous, &current);
        self.previous = current;
        self.taken = Some(now);
        usage
    }
    /// A baseline older than one period would otherwise leave a newly shown
    /// view with outdated usage until the next sample.
    fn stale(&self, now: std::time::Instant, period: std::time::Duration) -> bool {
        self.taken
            .is_none_or(|taken| now.duration_since(taken) >= period)
    }
}

fn sample_metrics(
    metrics: smithay_client_toolkit::reexports::calloop::channel::Sender<Update>,
    demand: std::sync::mpsc::Receiver<bool>,
) {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};
    const SIGNALS_AND_CPU: usize = 0;
    const MEMORY: usize = 1;
    const DISK: usize = 2;
    const BATTERY: usize = 3;
    const PERIODS: [Duration; 4] = [
        Duration::from_secs(2),
        Duration::from_secs(3),
        Duration::from_secs(30),
        Duration::from_secs(10),
    ];
    let mut detailed = false;
    let mut cpu = CpuSampler::default();
    let mut previous_signals = Default::default();
    let now = Instant::now();
    // Memory and disk samples pause while no view draws them.
    let mut deadlines = [Some(now), None, None, Some(now)];
    loop {
        let now = Instant::now();
        for (task, deadline) in deadlines.iter_mut().enumerate() {
            if deadline.is_none_or(|deadline| now < deadline) {
                continue;
            }
            let update = match task {
                SIGNALS_AND_CPU => {
                    let signals = network::signals();
                    if signals != previous_signals {
                        if metrics
                            .send(Update::NetworkSignals(signals.clone()))
                            .is_err()
                        {
                            return;
                        }
                        previous_signals = signals;
                    }
                    if detailed {
                        Some(Update::Cpu(cpu.sample(now)))
                    } else {
                        if cpu.taken.is_none() {
                            cpu.sample(now);
                        }
                        None
                    }
                }
                MEMORY => Some(Update::Memory(
                    std::fs::read_to_string("/proc/meminfo")
                        .ok()
                        .and_then(|s| mem::parse(&s)),
                )),
                DISK => Some(Update::Disk(disk::read())),
                BATTERY => Some(Update::Battery(battery::read())),
                _ => unreachable!("metrics task {task}"),
            };
            if let Some(update) = update
                && metrics.send(update).is_err()
            {
                return;
            }
            *deadline = Some(now + PERIODS[task]);
        }
        let next = deadlines.iter().flatten().min().copied();
        match demand.recv_timeout(next.map_or(Duration::MAX, |next| {
            next.saturating_duration_since(Instant::now())
        })) {
            Ok(visible) if visible != detailed => {
                detailed = visible;
                let now = Instant::now();
                if detailed {
                    deadlines[MEMORY] = Some(now);
                    deadlines[DISK] = Some(now);
                    if cpu.stale(now, PERIODS[SIGNALS_AND_CPU]) {
                        deadlines[SIGNALS_AND_CPU] = Some(now);
                    }
                } else {
                    deadlines[MEMORY] = None;
                    deadlines[DISK] = None;
                }
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

pub fn workers(
    sender: smithay_client_toolkit::reexports::calloop::channel::Sender<Update>,
    foreground: crate::theme::Color,
) -> (tray::Client, Metrics) {
    let metrics = sender.clone();
    let (demand, demand_changes) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("bar-metrics".into())
        .spawn(move || sample_metrics(metrics, demand_changes))
        .expect("metrics worker");
    let audio = sender.clone();
    audio::listen(move |mic, value| {
        let _ = audio.send(Update::Audio(mic, value));
    });
    let network = sender.clone();
    network::listen(move |value| {
        let _ = network.send(Update::Network(value));
    });
    media::listen(sender.clone());
    let weather = sender.clone();
    std::thread::Builder::new()
        .name("bar-weather".into())
        .spawn(move || {
            loop {
                if weather.send(Update::Weather(weather::read())).is_err() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(600));
            }
        })
        .expect("weather worker");
    let tray = tray::Client::new(foreground, move |items| {
        let _ = sender.send(Update::Tray(items));
    });
    (
        tray,
        Metrics {
            detailed: false,
            demand,
        },
    )
}
