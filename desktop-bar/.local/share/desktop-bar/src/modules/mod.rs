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

pub fn workers(
    sender: smithay_client_toolkit::reexports::calloop::channel::Sender<Update>,
    foreground: crate::theme::Color,
) -> tray::Client {
    let metrics = sender.clone();
    std::thread::Builder::new()
        .name("bar-metrics".into())
        .spawn(move || {
            use std::time::{Duration, Instant};
            let mut previous = vec![];
            let mut previous_signals = Default::default();
            let mut deadlines = [Instant::now(); 4];
            let periods = [2, 3, 30, 10];
            loop {
                let now = Instant::now();
                for i in 0..4 {
                    if now < deadlines[i] {
                        continue;
                    }
                    let update = match i {
                        0 => {
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
                            let current = cpu::parse(
                                &std::fs::read_to_string("/proc/stat").unwrap_or_default(),
                            );
                            let usage = cpu::usage(&previous, &current);
                            previous = current;
                            Update::Cpu(usage)
                        }
                        1 => Update::Memory(
                            std::fs::read_to_string("/proc/meminfo")
                                .ok()
                                .and_then(|s| mem::parse(&s)),
                        ),
                        2 => Update::Disk(disk::read()),
                        _ => Update::Battery(battery::read()),
                    };
                    if metrics.send(update).is_err() {
                        return;
                    }
                    deadlines[i] = now + Duration::from_secs(periods[i]);
                }
                std::thread::sleep(
                    deadlines
                        .iter()
                        .min()
                        .unwrap()
                        .saturating_duration_since(Instant::now()),
                );
            }
        })
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
    tray::Client::new(foreground, move |items| {
        let _ = sender.send(Update::Tray(items));
    })
}
