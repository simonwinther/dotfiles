use libpulse_binding::{
    callbacks::ListResult,
    context::{Context, FlagSet, State, subscribe::InterestMaskSet},
    mainloop::standard::{IterateResult, Mainloop},
    volume::Volume,
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Audio {
    pub volume: u32,
    pub muted: bool,
    pub description: String,
    pub available: bool,
}

pub fn listen(send: impl Fn(bool, Audio) + Send + 'static) {
    std::thread::Builder::new()
        .name("bar-audio".into())
        .spawn(move || {
            loop {
                run(&send);
                send(false, Audio::default());
                send(true, Audio::default());
                std::thread::sleep(Duration::from_secs(3));
            }
        })
        .expect("audio worker");
}

fn run(send: &impl Fn(bool, Audio)) {
    let Some(mut mainloop) = Mainloop::new() else {
        return;
    };
    let Some(mut context) = Context::new(&mainloop, "Desktop Bar") else {
        return;
    };
    if context.connect(None, FlagSet::NOAUTOSPAWN, None).is_err() {
        return;
    }
    if pump(&mut mainloop, &context, || {
        context.get_state() == State::Ready
    })
    .is_err()
    {
        return;
    }
    let changed = Rc::new(Cell::new(true));
    let event = changed.clone();
    context.set_subscribe_callback(Some(Box::new(move |_, _, _| event.set(true))));
    let _subscription = context.subscribe(
        InterestMaskSet::SERVER | InterestMaskSet::SINK | InterestMaskSet::SOURCE,
        |_| {},
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let mut sink = None;
    let mut source = None;
    loop {
        if context.get_state() != State::Ready {
            break;
        }
        if changed.replace(false) {
            // Replacing operations cancels stale requests through the shared binding.
            let output = tx.clone();
            sink = Some(context.introspect().get_sink_info_by_name(
                "@DEFAULT_SINK@",
                move |info| {
                    if let ListResult::Item(info) = info {
                        let _ = output.send((
                            false,
                            Audio {
                                volume: (info.volume.avg().0 as f64 / Volume::NORMAL.0 as f64
                                    * 100.0)
                                    .round() as u32,
                                muted: info.mute,
                                description: info.description.as_deref().unwrap_or("Output").into(),
                                available: true,
                            },
                        ));
                    }
                },
            ));
            let input = tx.clone();
            source = Some(context.introspect().get_source_info_by_name(
                "@DEFAULT_SOURCE@",
                move |info| {
                    if let ListResult::Item(info) = info {
                        let _ = input.send((
                            true,
                            Audio {
                                volume: (info.volume.avg().0 as f64 / Volume::NORMAL.0 as f64
                                    * 100.0)
                                    .round() as u32,
                                muted: info.mute,
                                description: info
                                    .description
                                    .as_deref()
                                    .unwrap_or("Microphone")
                                    .into(),
                                available: true,
                            },
                        ));
                    }
                },
            ));
        }
        for (microphone, value) in rx.try_iter() {
            send(microphone, value);
        }
        if matches!(
            mainloop.iterate(true),
            IterateResult::Err(_) | IterateResult::Quit(_)
        ) {
            break;
        }
    }
    drop((sink, source));
    context.disconnect();
}

fn connect() -> Result<(Mainloop, Context), String> {
    connect_with(&|| false, Instant::now() + Duration::from_secs(3))
}
fn connect_with(
    cancelled: &impl Fn() -> bool,
    deadline: Instant,
) -> Result<(Mainloop, Context), String> {
    if cancelled() {
        return Err("Cancelled".into());
    }
    let mut mainloop = Mainloop::new().ok_or("Audio mainloop unavailable")?;
    let mut context =
        Context::new(&mainloop, "Desktop Controls").ok_or("Audio context unavailable")?;
    context
        .connect(None, FlagSet::NOAUTOSPAWN, None)
        .map_err(|e| {
            e.to_string()
                .unwrap_or_else(|| "Audio connection failed".into())
        })?;
    pump_with(
        &mut mainloop,
        &context,
        || context.get_state() == State::Ready,
        cancelled,
        deadline,
    )?;
    Ok((mainloop, context))
}
fn pump(mainloop: &mut Mainloop, context: &Context, done: impl Fn() -> bool) -> Result<(), String> {
    pump_with(
        mainloop,
        context,
        done,
        &|| false,
        Instant::now() + Duration::from_secs(3),
    )
}
fn pump_with(
    mainloop: &mut Mainloop,
    context: &Context,
    done: impl Fn() -> bool,
    cancelled: &impl Fn() -> bool,
    deadline: Instant,
) -> Result<(), String> {
    while !done() {
        if cancelled() {
            return Err("Cancelled".into());
        }
        if Instant::now() >= deadline
            || matches!(context.get_state(), State::Failed | State::Terminated)
            || matches!(
                mainloop.iterate(false),
                IterateResult::Err(_) | IterateResult::Quit(_)
            )
        {
            return Err("Audio service did not respond".into());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}
pub fn entries(cancelled: &impl Fn() -> bool) -> anyhow::Result<Vec<crate::shell::Entry>> {
    use crate::shell::Entry;
    use std::cell::RefCell;
    let deadline = Instant::now() + Duration::from_secs(3);
    let (mut mainloop, mut context) =
        connect_with(cancelled, deadline).map_err(anyhow::Error::msg)?;
    let entries = Rc::new(RefCell::new(Vec::new()));
    let finished = Rc::new(Cell::new(0));
    let defaults = Rc::new(RefCell::new((String::new(), String::new())));
    let names = defaults.clone();
    let count = finished.clone();
    let server = context.introspect().get_server_info(move |info| {
        *names.borrow_mut() = (
            info.default_sink_name.as_deref().unwrap_or_default().into(),
            info.default_source_name
                .as_deref()
                .unwrap_or_default()
                .into(),
        );
        count.set(count.get() + 1);
    });
    pump_with(
        &mut mainloop,
        &context,
        || finished.get() >= 1,
        cancelled,
        deadline,
    )
    .map_err(anyhow::Error::msg)?;
    let make = |category: &str,
                index: u32,
                name: &str,
                detail: &str,
                volume: &libpulse_binding::volume::ChannelVolumes,
                mute: bool,
                channels: u8,
                selected: bool| {
        let level = (volume.avg().0 as f64 / Volume::NORMAL.0 as f64 * 100.0).round() as u32;
        Entry {
            id: format!("{category}/{index}"),
            name: name.into(),
            detail: detail.into(),
            category: category.into(),
            level: Some(level),
            muted: mute,
            selected,
            icon: if category == "inputs" || category == "recording" {
                ""
            } else {
                ""
            }
            .into(),
            command: vec![
                "native".into(),
                "audio".into(),
                format!("{category}-mute"),
                format!("{index}:{channels}"),
                (!mute).to_string(),
            ],
            ..Default::default()
        }
    };
    let list = entries.clone();
    let count = finished.clone();
    let names = defaults.clone();
    let sinks = context
        .introspect()
        .get_sink_info_list(move |result| match result {
            ListResult::Item(info) => {
                let mut item = make(
                    "outputs",
                    info.index,
                    info.description.as_deref().unwrap_or("Output"),
                    info.name.as_deref().unwrap_or_default(),
                    &info.volume,
                    info.mute,
                    info.channel_map.len(),
                    names.borrow().0 == info.name.as_deref().unwrap_or_default(),
                );
                item.keywords = info.name.as_deref().unwrap_or_default().into();
                list.borrow_mut().push(item);
            }
            _ => count.set(count.get() + 1),
        });
    let list = entries.clone();
    let count = finished.clone();
    let names = defaults.clone();
    let sources = context
        .introspect()
        .get_source_info_list(move |result| match result {
            ListResult::Item(info) if info.monitor_of_sink.is_none() => {
                let mut item = make(
                    "inputs",
                    info.index,
                    info.description.as_deref().unwrap_or("Input"),
                    info.name.as_deref().unwrap_or_default(),
                    &info.volume,
                    info.mute,
                    info.channel_map.len(),
                    names.borrow().1 == info.name.as_deref().unwrap_or_default(),
                );
                item.keywords = info.name.as_deref().unwrap_or_default().into();
                list.borrow_mut().push(item);
            }
            ListResult::Item(_) => {}
            _ => count.set(count.get() + 1),
        });
    let list = entries.clone();
    let count = finished.clone();
    let playback = context
        .introspect()
        .get_sink_input_info_list(move |result| match result {
            ListResult::Item(info) => list.borrow_mut().push(make(
                "playback",
                info.index,
                &info
                    .proplist
                    .get_str("application.name")
                    .unwrap_or_else(|| "Playback".into()),
                info.name.as_deref().unwrap_or_default(),
                &info.volume,
                info.mute,
                info.channel_map.len(),
                false,
            )),
            _ => count.set(count.get() + 1),
        });
    let list = entries.clone();
    let count = finished.clone();
    let recording = context
        .introspect()
        .get_source_output_info_list(move |result| match result {
            ListResult::Item(info) => list.borrow_mut().push(make(
                "recording",
                info.index,
                &info
                    .proplist
                    .get_str("application.name")
                    .unwrap_or_else(|| "Recording".into()),
                info.name.as_deref().unwrap_or_default(),
                &info.volume,
                info.mute,
                info.channel_map.len(),
                false,
            )),
            _ => count.set(count.get() + 1),
        });
    let list = entries.clone();
    let count = finished.clone();
    let cards = context
        .introspect()
        .get_card_info_list(move |result| match result {
            ListResult::Item(info) => {
                for profile in &info.profiles {
                    if !profile.available {
                        continue;
                    }
                    let name = profile.name.as_deref().unwrap_or_default();
                    list.borrow_mut().push(Entry {
                        id: format!("card/{}/{name}", info.index),
                        name: profile.description.as_deref().unwrap_or(name).into(),
                        detail: info
                            .proplist
                            .get_str("device.description")
                            .unwrap_or_default(),
                        category: "configuration".into(),
                        icon: "󰒓".into(),
                        selected: info.active_profile.as_ref().and_then(|p| p.name.as_deref())
                            == Some(name),
                        command: vec![
                            "native".into(),
                            "audio".into(),
                            "profile".into(),
                            info.name.as_deref().unwrap_or_default().into(),
                            name.into(),
                        ],
                        ..Default::default()
                    });
                }
            }
            _ => count.set(count.get() + 1),
        });
    pump_with(
        &mut mainloop,
        &context,
        || finished.get() >= 6,
        cancelled,
        deadline,
    )
    .map_err(anyhow::Error::msg)?;
    drop((server, sinks, sources, playback, recording, cards));
    context.disconnect();
    let result = entries.borrow().clone();
    Ok(result)
}
pub fn action(operation: &str, target: &str, value: &str) -> Result<(), String> {
    if target.contains('\0') || value.contains('\0') {
        return Err("Invalid audio target".into());
    }
    let (mut mainloop, mut context) = connect()?;
    let result = Rc::new(Cell::new(None));
    let done = result.clone();
    let callback = Some(Box::new(move |ok| done.set(Some(ok))) as Box<dyn FnMut(bool)>);
    let (index, channels) = target.split_once(':').unwrap_or((target, "2"));
    let index = index.parse::<u32>().unwrap_or(0);
    let channels = channels.parse::<u8>().unwrap_or(2).clamp(1, 32);
    let muted = value == "true";
    let mut volume = libpulse_binding::volume::ChannelVolumes::default();
    volume.set(
        channels,
        Volume(
            (value.parse::<f64>().unwrap_or(0.0).clamp(0.0, 100.0) / 100.0
                * Volume::NORMAL.0 as f64)
                .round() as u32,
        ),
    );
    let mut info = context.introspect();
    let command = match operation {
        "outputs-volume" => info.set_sink_volume_by_index(index, &volume, callback),
        "inputs-volume" => info.set_source_volume_by_index(index, &volume, callback),
        "playback-volume" => info.set_sink_input_volume(index, &volume, callback),
        "recording-volume" => info.set_source_output_volume(index, &volume, callback),
        "outputs-mute" => info.set_sink_mute_by_index(index, muted, callback),
        "inputs-mute" => info.set_source_mute_by_index(index, muted, callback),
        "playback-mute" => info.set_sink_input_mute(index, muted, callback),
        "recording-mute" => info.set_source_output_mute(index, muted, callback),
        "profile" => info.set_card_profile_by_name(target, value, callback),
        "default-output" => {
            let done = result.clone();
            context.set_default_sink(target, move |ok| done.set(Some(ok)))
        }
        "default-input" => {
            let done = result.clone();
            context.set_default_source(target, move |ok| done.set(Some(ok)))
        }
        _ => return Err("Unsupported audio action".into()),
    };
    pump(&mut mainloop, &context, || result.get().is_some())?;
    drop(command);
    context.disconnect();
    if result.get() == Some(true) {
        Ok(())
    } else {
        Err("Audio action failed".into())
    }
}
