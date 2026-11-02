//! A single-process layer-shell desktop bar with compositor-paced animation.
mod action_queue;
mod anim;
mod applications;
mod assets;
mod audio_commands;
mod bus;
mod clipboard;
mod desktop;
mod draw;
mod fuzzy;
mod help;
mod input;
mod ipc;
mod layout;
mod menus;
mod modules;
mod now_playing;
mod overview;
mod panel;
mod picker;
mod process;
mod prompt;
mod render;
mod search;
mod shell;
mod style;
mod surface;
mod text;
mod theme;
mod thumbnail;

use layout::{Module, Rect};
use modules::{
    Data, Update,
    tray::{Action as TrayAction, Client as TrayClient},
};
use panel::{Kind, Panel};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region, Surface},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop, LoopHandle, channel,
            signals::{Signal, Signals},
            timer::{TimeoutAction, Timer},
        },
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, Dispatch, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        xdg::{
            XdgPositioner, XdgShell,
            popup::{Popup, PopupConfigure, PopupHandler},
        },
    },
    shm::{Shm, ShmHandler},
};
use std::os::unix::process::CommandExt;

use std::{
    collections::HashMap,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use surface::Canvas;
use theme::Theme;
use wayland_protocols::{
    wp::{
        fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
        viewporter::client::{wp_viewport, wp_viewporter},
    },
    xdg::shell::client::xdg_positioner,
};

struct Bar {
    canvas: Canvas,
    view: render::View,
    layer: LayerSurface,
    output: wl_output::WlOutput,
    name: String,
    minimal: bool,
    motion: bool,
}
impl Bar {
    fn surface(&self) -> &wl_surface::WlSurface {
        self.layer.wl_surface()
    }
}
struct Backdrop {
    canvas: Canvas,
    layer: LayerSurface,
}
struct Morph {
    width: anim::Spring,
    height: anim::Spring,
    closed: (f32, f32),
    closed_image: Option<tiny_skia::Pixmap>,
    closed_dirty: bool,
    backdrop: Backdrop,
    keyboard: LayerSurface,
    auto_hide: bool,
    pointer_inside: bool,
    /// Panel area at the last click inside it. When that click shrinks the
    /// panel, the area keeps receiving the pointer until the pointer leaves
    /// it, so a resting cursor neither closes the panel nor clicks outside.
    sticky: Option<Rect>,
}
impl Morph {
    fn dimensions(&self) -> (f32, f32) {
        (
            self.width.value.max(self.closed.0 - 8.0),
            self.height.value.max(self.closed.1 - 6.0),
        )
    }
}
impl Drop for Morph {
    fn drop(&mut self) {
        self.keyboard
            .set_keyboard_interactivity(KeyboardInteractivity::None);
        self.keyboard.commit();
    }
}
enum FloatingWindow {
    Popup(Popup),
    Root(wl_surface::WlSurface),
    Overlay(LayerSurface),
}
impl FloatingWindow {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Self::Popup(popup) => popup.wl_surface(),
            Self::Root(surface) => surface,
            Self::Overlay(layer) => layer.wl_surface(),
        }
    }
    fn popup(&self) -> Option<&Popup> {
        match self {
            Self::Popup(popup) => Some(popup),
            _ => None,
        }
    }
}
struct Floating {
    canvas: Canvas,
    panel: Panel,
    parents: Vec<Panel>,
    window: FloatingWindow,
    name: String,
    anchor: Rect,
    last: Instant,
    morph: Option<Morph>,
    content: Option<tiny_skia::Pixmap>,
    content_stale: bool,
    mask: Option<tiny_skia::Mask>,
    painted: Option<Rect>,
}
struct App {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    compositor: CompositorState,
    layers: LayerShell,
    xdg: XdgShell,
    shm: Shm,
    viewporter: Option<wp_viewporter::WpViewporter>,
    fractional: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    qh: QueueHandle<Self>,
    handle: LoopHandle<'static, Self>,
    bars: Vec<Bar>,
    floating: Option<Floating>,
    pointers: HashMap<wl_seat::WlSeat, wl_pointer::WlPointer>,
    keyboards: HashMap<wl_seat::WlSeat, wl_keyboard::WlKeyboard>,
    theme: Theme,
    text: text::Text,
    data: Data,
    tray: TrayClient,
    workspaces: std::sync::mpsc::Sender<Vec<String>>,
    shell: shell::Service,
    modifiers: Modifiers,
    focused_output: Option<String>,
    scroll: f64,
    panel_scroll: input::Scroll,
    drag: Option<Module>,
    audio_drag: Option<(u64, String, String)>,
    hover_generation: u64,
    hover_pending: Option<(String, Option<Module>)>,
    overview_hover: Option<&'static str>,
    hover_suppressed_until: Instant,
    trace_frames: bool,
    refresh_scheduled: bool,
    last_seat: Option<wl_seat::WlSeat>,
    last_serial: u32,
    last_time: u32,
    exit: bool,
    preview: bool,
    menus_only: bool,
}

fn main() {
    // A few event workers allocate small snapshots. Sharing two arenas avoids
    // retaining one mostly empty allocator arena per worker on large machines.
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
    if let Err(error) = run() {
        if error.is::<picker::Cancelled>() {
            std::process::exit(130);
        }
        eprintln!("desktop-bar: {error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let initial_menu = if args.first().is_some_and(|s| s == "--menu") {
        let name = args
            .get(1)
            .filter(|_| args.len() == 2)
            .ok_or("--menu requires a panel")?;
        if ipc::send(name).is_ok() {
            return Ok(());
        }
        Some(name.clone())
    } else {
        None
    };
    let mut initial_picker = None;
    let mut picker_answers = None;
    if args.first().is_some_and(|s| s == "--dmenu") {
        let options = picker::Options::arguments(&args)?;
        match picker::client(&options) {
            Ok(Some(answer)) => {
                println!("{answer}");
                return Ok(());
            }
            Ok(None) => return Err(picker::Cancelled.into()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(error) => return Err(error.into()),
        }
        let (request, answers) = picker::Request::new(options);
        initial_picker = Some(request);
        picker_answers = Some(answers);
    }
    let menus_only = initial_menu.is_some() || initial_picker.is_some();
    if args.first().is_some_and(|s| s == "--panel") {
        let name = args.get(1).filter(|_| args.len() == 2).ok_or("Usage: desktop-bar --panel apps|themes|wallpapers|clipboard|styles|controls|projects|volume|calendar|battery|network|system|close")?;
        ipc::send(name)?;
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--action") {
        if !(3..=6).contains(&args.len()) {
            return Err("--action requires a provider and action arguments".into());
        }
        ipc::send(&format!("action/{}", serde_json::to_string(&args[1..])?))?;
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--catalog") {
        let provider = args
            .get(1)
            .filter(|_| args.len() == 2)
            .ok_or("--catalog requires a provider")?;
        let entries = desktop::load(provider, || false).ok_or("Unknown native provider")??;
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--weather") {
        let refresh = match &args[1..] {
            [] => false,
            [flag] if flag == "--refresh" => true,
            _ => return Err("Usage: desktop-bar --weather [--refresh]".into()),
        };
        let weather = modules::weather::read_with_refresh(refresh);
        if let Some(observation) = weather.observation {
            println!(
                "{}: {:.0}°C · {:.1} km/h{}",
                observation.description,
                observation.temperature,
                observation.wind_kmh,
                if weather.stale { " (cached)" } else { "" },
            );
        } else {
            println!("Weather unavailable: {}", weather.reason);
        }
        return Ok(());
    }
    if args.first().is_some_and(|s| s == "--dump") {
        return dump(&args).map_err(Into::into);
    }
    if args.iter().any(|s| s == "--help") {
        println!(
            "Morphing Notch Bar\ndesktop-bar [--preview] [--duration SECONDS]\ndesktop-bar --weather [--refresh]\ndesktop-bar --panel NAME\ndesktop-bar --menu NAME\ndesktop-bar --dmenu [--placeholder TITLE] [--inputonly] [--search QUERY]\ndesktop-bar --dump FILE SCALE clean|motion [--light] [--panel volume|calendar|battery]"
        );
        return Ok(());
    }
    if !menus_only
        && args.iter().enumerate().any(|(i, s)| {
            s != "--preview" && s != "--duration" && (i == 0 || args[i - 1] != "--duration")
        })
    {
        return Err("Unknown argument; see --help".into());
    }
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&connection)?;
    let qh = queue.handle();
    let mut event_loop = EventLoop::<App>::try_new()?;
    let handle = event_loop.handle();
    let (sender, receiver) = channel::channel();
    let mut theme = Theme::read();
    if menus_only {
        theme.style = "native-motion".into();
    }
    let text = text::Text::new(&theme.font)?;
    // Block process signals before creating workers so every thread inherits
    // the mask. Otherwise SIGUSR2 can terminate an arbitrary worker instead of
    // reaching calloop's signalfd.
    let signals = Signals::new(&[Signal::SIGUSR2, Signal::SIGTERM, Signal::SIGINT])?;
    let tray = modules::workers(sender.clone(), theme.text());
    let workspaces = modules::workspaces::start(sender.clone());
    let shell = shell::Service::start(sender.clone());
    let _server = if args.iter().any(|s| s == "--preview") {
        None
    } else {
        Some(ipc::Server::start(sender.clone())?)
    };
    let _menus_server = if args.iter().any(|s| s == "--preview") {
        None
    } else {
        Some(picker::Server::start(sender)?)
    };
    let compositor = CompositorState::bind(&globals, &qh)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        compositor,
        layers: LayerShell::bind(&globals, &qh)?,
        xdg: XdgShell::bind(&globals, &qh)?,
        shm: Shm::bind(&globals, &qh)?,
        viewporter: globals.bind(&qh, 1..=1, ()).ok(),
        fractional: globals.bind(&qh, 1..=1, ()).ok(),
        qh: qh.clone(),
        handle: handle.clone(),
        bars: vec![],
        floating: None,
        pointers: HashMap::new(),
        keyboards: HashMap::new(),
        theme,
        text,
        data: Data::default(),
        tray,
        workspaces,
        shell,
        modifiers: Modifiers::default(),
        focused_output: None,
        scroll: 0.0,
        panel_scroll: input::Scroll::default(),
        drag: None,
        audio_drag: None,
        hover_generation: 0,
        hover_pending: None,
        overview_hover: None,
        hover_suppressed_until: Instant::now(),
        trace_frames: std::env::var_os("DESKTOP_BAR_TRACE_FRAMES").is_some(),
        refresh_scheduled: false,
        last_seat: None,
        last_serial: 0,
        last_time: 0,
        exit: false,
        preview: args.iter().any(|s| s == "--preview"),
        menus_only,
    };
    queue.roundtrip(&mut app)?;
    app.sync_outputs();
    if menus_only {
        queue.roundtrip(&mut app)?;
    }
    if let Some(command) = initial_menu {
        app.command(&command);
    }
    if let Some(request) = initial_picker {
        app.show_picker(request);
    }
    if menus_only && app.floating.is_none() {
        return Err("No output is available for the menu".into());
    }
    handle.insert_source(receiver, |event, _, app| {
        if let channel::Event::Msg(update) = event {
            app.update(update);
        }
    })?;
    handle.insert_source(
        Timer::from_duration(modules::clock::until_minute()),
        |_, _, app| {
            app.invalidate_closed_notch();
            let panel = app
                .floating
                .as_ref()
                .is_some_and(|f| f.panel.kind.uses_clock());
            app.request_views(|_| true, panel);
            TimeoutAction::ToDuration(modules::clock::until_minute())
        },
    )?;
    handle.insert_source(Timer::from_duration(Duration::from_secs(5)), |_, _, _| {
        trim_heap();
        TimeoutAction::Drop
    })?;
    handle.insert_source(signals, |event, _, app| {
        if event.signal() == Signal::SIGUSR2 {
            app.reload();
        } else {
            app.exit = true;
        }
    })?;
    if let Some(index) = args.iter().position(|a| a == "--duration") {
        let seconds = args
            .get(index + 1)
            .ok_or("--duration requires seconds")?
            .parse::<u64>()?;
        handle.insert_source(
            Timer::from_duration(Duration::from_secs(seconds)),
            |_, _, app| {
                app.exit = true;
                TimeoutAction::Drop
            },
        )?;
    }
    WaylandSource::new(connection, queue).insert(handle)?;
    while !app.exit {
        event_loop.dispatch(None, &mut app)?;
    }
    if let Some(answers) = picker_answers {
        drop(app);
        match answers.recv()? {
            Some(answer) => println!("{answer}"),
            None => return Err(picker::Cancelled.into()),
        }
    }
    Ok(())
}

impl App {
    fn controls_kind(&mut self) -> Kind {
        Kind::Shell(Box::new(shell::Model::new(
            shell::Page::Controls,
            self.shell.load(shell::Page::Controls),
            &self.theme,
        )))
    }
    fn overview_kind(&self) -> Kind {
        let view = self.focused_bar().map(|i| &self.bars[i].view);
        Kind::Overview(overview::Overview {
            workspaces: view.map(|v| v.visible_workspaces()).unwrap_or_default(),
            active: view.map(|v| v.active).unwrap_or(0),
        })
    }
    fn focused_bar(&self) -> Option<usize> {
        self.focused_output
            .as_ref()
            .and_then(|name| self.bars.iter().position(|b| &b.name == name))
            .or_else(|| {
                self.floating
                    .as_ref()
                    .and_then(|f| self.bars.iter().position(|b| b.name == f.name))
            })
            .or_else(|| {
                self.bars
                    .iter()
                    .position(|b| b.view.hover.is_some() && !b.minimal)
            })
            .or_else(|| self.bars.iter().position(|b| !b.minimal))
    }
    fn command(&mut self, command: &str) {
        if let Some(request) = command.strip_prefix("launcher/") {
            if let Ok(request) = serde_json::from_str::<search::Request>(request) {
                self.show_provider(&request.providers.join(","), &request.query);
            }
            return;
        }
        if let Some(arguments) = command.strip_prefix("action/") {
            let Ok(arguments) = serde_json::from_str::<Vec<String>>(arguments) else {
                return;
            };
            let page = match arguments.first().map(String::as_str) {
                Some("audio") => shell::Page::Audio,
                Some("bluetooth") => shell::Page::Bluetooth,
                Some("wifi") => shell::Page::Wifi,
                Some("notifications") => shell::Page::Notifications,
                _ => return,
            };
            self.show_page(page, false);
            if let Some(f) = &self.floating
                && let Kind::Shell(model) = &f.panel.kind
            {
                self.shell.native_action(arguments, model.generation);
            }
            return;
        }
        if command == "close" {
            self.close();
            return;
        }
        if let Some(page) = shell::Page::parse(command) {
            self.show_page(page, true);
            return;
        }
        if let Some(group) = command.strip_prefix("menu/") {
            self.show_menu(group);
            return;
        }
        if search::PROVIDERS.contains(&command) {
            self.show_provider(command, "");
            return;
        }
        let kind = match command {
            "overview" => self.overview_kind(),
            "volume" => Kind::Volume,
            "calendar" => Kind::Calendar,
            "battery" => Kind::Battery,
            "network" => Kind::Network,
            "system" => Kind::System,
            _ => return,
        };
        if let Some(index) = self.focused_bar() {
            self.open_notch(index, kind);
        }
    }
    fn show_page(&mut self, page: shell::Page, toggle: bool) {
        if self.trace_frames {
            eprintln!("bar-page {page:?}");
        }
        if toggle
            && self.floating.as_ref().is_some_and(|f| {
                !f.panel.closing
                    && matches!(&f.panel.kind, Kind::Shell(model) if model.page == page)
            })
        {
            self.close();
            return;
        }
        if self.return_to(|kind| matches!(kind, Kind::Shell(model) if model.page == page)) {
            return;
        }
        let Some(index) = self.focused_bar() else {
            return;
        };
        let generation = if page == shell::Page::Reminder {
            self.shell.cancel();
            0
        } else {
            self.shell.load(page)
        };
        let mut model = shell::Model::new(page, generation, &self.theme);
        self.shell.populate(&mut model);
        if page == shell::Page::Clipboard {
            model.clipboard.target = modules::workspaces::active_window().unwrap_or_default();
        }
        self.open_notch(index, Kind::Shell(Box::new(model)));
    }
    fn show_menu(&mut self, group: &str) {
        if self.return_to(
            |kind| matches!(kind, Kind::Shell(model) if model.provider == format!("menu/{group}")),
        ) {
            return;
        }
        let page = match group {
            "bar" => Some(shell::Page::Styles),
            "theme" => Some(shell::Page::Themes),
            "background" => Some(shell::Page::Wallpapers),
            _ => None,
        };
        if let Some(page) = page {
            self.show_page(page, false);
            return;
        }
        if group == "reminder-set" {
            self.show_page(shell::Page::Reminder, false);
            return;
        }
        let Some(index) = self.focused_bar() else {
            return;
        };
        let generation = self.shell.provider(&format!("menu/{group}"));
        let mut model = shell::Model::new(shell::Page::Menu, generation, &self.theme);
        model.provider = format!("menu/{group}");
        model.title = if group == "main" {
            "Go".into()
        } else {
            group.replace('-', " ")
        };
        self.open_notch(index, Kind::Shell(Box::new(model)));
    }
    fn show_provider(&mut self, provider: &str, query: &str) {
        if matches!(
            provider,
            "" | "apps" | "desktopapplications" | "desktopapplications,websearch"
        ) && let Some(query) = query.strip_prefix('$')
        {
            self.show_provider("clipboard", query.trim_start());
            return;
        }
        if provider == "menus" {
            self.show_menu("main");
            return;
        }
        let page = match provider {
            "" | "apps" | "desktopapplications" | "desktopapplications,websearch" => {
                Some(shell::Page::Apps)
            }
            "clipboard" => Some(shell::Page::Clipboard),
            "bluetooth" => Some(shell::Page::Bluetooth),
            "menus:notifications" => Some(shell::Page::Notifications),
            _ => None,
        };
        if let Some(page) = page {
            self.show_page(page, false);
            if !query.is_empty()
                && let Some(f) = &mut self.floating
                && let Kind::Shell(model) = &mut f.panel.kind
            {
                model.query = query.into();
                model.filter();
                if page == shell::Page::Clipboard {
                    model.clipboard.searching = true;
                }
                self.shell.query(model);
                f.canvas.request(f.window.wl_surface(), &self.qh);
            }
            return;
        }
        if let Some(group) = provider
            .strip_prefix("menus:")
            .filter(|group| *group != "repositories")
        {
            self.show_menu(group);
            return;
        }
        if !provider
            .split(',')
            .all(|name| search::PROVIDERS.contains(&name))
        {
            return;
        }
        let Some(index) = self.focused_bar() else {
            return;
        };
        let name = format!("search/{provider}");
        let mut model =
            shell::Model::new(shell::Page::Search, self.shell.provider(&name), &self.theme);
        model.provider = name;
        model.title = search::title(provider).into();
        model.query = query.into();
        self.shell.query(&mut model);
        self.open_notch(index, Kind::Shell(Box::new(model)));
    }
    fn show_picker(&mut self, mut request: std::sync::Arc<picker::Request>) {
        let Some(index) = self.focused_bar() else {
            return;
        };
        let mut model =
            shell::Model::new(shell::Page::Picker, self.shell.provider(""), &self.theme);
        model.title = request.options.title.clone();
        model.query = request.options.query.clone();
        let entries = std::sync::Arc::get_mut(&mut request)
            .map(|request| std::mem::take(&mut request.options.entries))
            .unwrap_or_else(|| request.options.entries.clone());
        model.entries = entries
            .into_iter()
            .enumerate()
            .map(|(index, value)| shell::Entry {
                id: index.to_string(),
                name: value,
                ..Default::default()
            })
            .collect();
        model.loading = false;
        model.picker = Some(request);
        model.filter();
        self.shell.query(&mut model);
        self.remember_parent();
        self.open_notch(index, Kind::Shell(Box::new(model)));
    }
    fn show_actions(&mut self) {
        let Some(f) = &self.floating else { return };
        let Kind::Shell(source) = &f.panel.kind else {
            return;
        };
        let entry = source
            .filtered
            .get(source.selected)
            .map(|&index| source.entries[index].clone());
        let provider = entry
            .as_ref()
            .map(|entry| entry.action_provider.as_str())
            .filter(|provider| !provider.is_empty())
            .or_else(|| source.provider.strip_prefix("search/"))
            .unwrap_or("")
            .to_owned();
        if provider.is_empty() {
            return;
        }
        let query = search::Request::launcher(
            source.provider.strip_prefix("search/").unwrap_or("apps"),
            &source.query,
        )
        .query;
        let mut entries = vec![];
        if let Some(entry) = &entry {
            for action in &entry.actions {
                entries.push(shell::Entry {
                    id: action.clone(),
                    name: search::action_label(action).into(),
                    detail: entry.name.clone(),
                    command: vec![
                        "search-activate".into(),
                        provider.clone(),
                        entry.id.clone(),
                        action.clone(),
                        query.clone(),
                    ],
                    ..Default::default()
                });
            }
        }
        let actions: &[&str] = match provider.as_str() {
            "todo" => &["create", "search", "clear"],
            "files" => &["refresh_index"],
            "calc" => &["delete_all"],
            _ => &[],
        };
        for action in actions {
            entries.push(shell::Entry {
                id: action.to_string(),
                name: search::action_label(action).into(),
                command: vec![
                    "search-activate".into(),
                    provider.clone(),
                    String::new(),
                    action.to_string(),
                    String::new(),
                ],
                ..Default::default()
            });
        }
        if entries.is_empty() {
            return;
        }
        let Some(index) = self.focused_bar() else {
            return;
        };
        self.remember_parent();
        let mut model = shell::Model::new(shell::Page::Menu, self.shell.provider(""), &self.theme);
        model.title = "Actions".into();
        model.provider = format!("actions/{provider}");
        model.entries = entries;
        model.loading = false;
        model.filter();
        self.open_notch(index, Kind::Shell(Box::new(model)));
    }
    fn shell_reply(&mut self, reply: shell::Reply) {
        if let shell::Reply::NativeAction(id, result) = reply {
            if self.trace_frames {
                eprintln!("bar-action generation={id} ok={}", result.is_ok());
            }
            if let Some(f) = &mut self.floating
                && let Kind::Prompt(model) = &f.panel.kind
                && matches!(model.previous.as_ref(), Kind::Shell(previous) if previous.generation == id)
            {
                let _ = model.request.response.try_send(None);
                f.panel.kind = *model.previous.clone();
            }
            if let Some(f) = &mut self.floating
                && !f.panel.closing
                && let Kind::Shell(model) = &mut f.panel.kind
                && model.generation == id
            {
                model.refreshing = false;
                model.finish_query_action(result.is_ok());
                if let Err(error) = result {
                    model.error = if error.contains("Cancelled") || error.contains("Canceled") {
                        String::new()
                    } else {
                        error
                    };
                } else {
                    model.error.clear();
                    model.refreshing = true;
                    self.shell.refresh(model);
                }
                f.canvas.request(f.window.wl_surface(), &self.qh);
            } else if let Err(error) = result
                && !error.contains("Cancelled")
                && !error.contains("Canceled")
            {
                spawn(&["notify-send", "Desktop controls", &error]);
            }
            return;
        }
        if let shell::Reply::ClipboardAction(_, Err(error)) = &reply {
            spawn(&["notify-send", "Clipboard", error]);
        }
        let Some(f) = &mut self.floating else {
            return;
        };
        let Kind::Shell(model) = &mut f.panel.kind else {
            return;
        };
        if f.panel.closing {
            return;
        }
        match reply {
            shell::Reply::Search(id, revision, result)
                if id == model.generation && revision == model.revision =>
            {
                if self.trace_frames {
                    eprintln!(
                        "bar-search generation={id} revision={revision} results={}",
                        result.as_ref().map(Vec::len).unwrap_or(0)
                    );
                }
                model.loading = false;
                model.refreshing = false;
                match result {
                    Ok(entries) => {
                        model.entries = entries;
                        model.remote = true;
                        model.error.clear();
                    }
                    Err(error) => model.error = error,
                }
                model.filter();
                self.shell.images(model);
            }
            shell::Reply::Filtered(id, revision, filtered)
                if id == model.generation && revision == model.revision =>
            {
                if self.trace_frames {
                    eprintln!(
                        "bar-filter generation={id} revision={revision} results={}",
                        filtered.len()
                    );
                }
                model.loading = false;
                model.filtered = filtered;
                model.selected = 0;
                model.cursor.snap(0.0);
                self.shell.images(model);
            }
            shell::Reply::ClipboardAction(id, Ok(Some(entries))) if id == model.generation => {
                let focused = model
                    .filtered
                    .get(model.selected)
                    .map(|&index| model.entries[index].id.clone());
                model
                    .clipboard
                    .selected
                    .retain(|id| entries.iter().any(|entry| &entry.id == id));
                model.entries = entries;
                model.filter();
                if let Some(position) = model
                    .filtered
                    .iter()
                    .position(|&index| Some(&model.entries[index].id) == focused.as_ref())
                {
                    model.navigate(position as i32);
                }
                self.shell.images(model);
            }
            shell::Reply::Catalog(id, result) if id == model.generation => {
                if model.page == shell::Page::Apps
                    && !model.query.is_empty()
                    && search::Request::launcher("apps", &model.query).providers
                        != ["desktopapplications", "websearch"]
                {
                    return;
                }
                let refreshing = model.refreshing;
                model.refreshing = false;
                // Live pages poll every second; most polls change nothing.
                let unchanged = |model: &shell::Model| {
                    (
                        model.entries.clone(),
                        model.filtered.clone(),
                        model.selected,
                        model.section,
                        model.error.clone(),
                        model.loading,
                    )
                };
                let before = (refreshing && model.query.is_empty() && desktop::refresh(model.page))
                    .then(|| unchanged(model));
                let focused = model
                    .filtered
                    .get(model.selected)
                    .map(|&i| model.entries[i].id.clone());
                let selected = model.selected;
                model.loading = false;
                match result {
                    Ok(entries) if model.page == shell::Page::Controls => {
                        for entry in entries {
                            if let Some(previous) =
                                model.entries.iter_mut().find(|item| item.id == entry.id)
                            {
                                *previous = entry;
                            } else {
                                model.entries.push(entry);
                            }
                        }
                    }
                    Ok(entries) => {
                        model.entries = entries;
                        model.error.clear();
                    }
                    Err(error) => model.error = error,
                }
                model.filter();
                if !model.query.is_empty() {
                    self.shell.query(model);
                }
                if !refreshing
                    && model.page == shell::Page::Audio
                    && !model
                        .entries
                        .iter()
                        .any(|entry| entry.category == "playback")
                {
                    model.section = [2, 3, 1, 4]
                        .into_iter()
                        .find(|&section| {
                            model.entries.iter().any(|entry| {
                                entry.category
                                    == [
                                        "playback",
                                        "recording",
                                        "outputs",
                                        "inputs",
                                        "configuration",
                                    ][section]
                            })
                        })
                        .unwrap_or(0);
                    model.filter();
                }
                if refreshing {
                    let position = focused
                        .and_then(|id| {
                            model
                                .filtered
                                .iter()
                                .position(|&i| model.entries[i].id == id)
                        })
                        .unwrap_or(selected)
                        .min(model.filtered.len().saturating_sub(1));
                    model.navigate(position as i32);
                } else {
                    model.select_current();
                }
                self.shell.images(model);
                if before.is_some_and(|before| before == unchanged(model)) {
                    return;
                }
            }
            shell::Reply::Images(id, requested, images)
                if id == model.generation && requested == model.requested =>
            {
                model.images = images
            }
            _ => return,
        }
        f.canvas.request(f.window.wl_surface(), &self.qh);
    }
    fn activate(&mut self) {
        if self.floating.as_ref().is_some_and(
            |f| matches!(&f.panel.kind, Kind::Shell(model) if model.page == shell::Page::Clipboard),
        ) {
            let input = if let Some(f) = &mut self.floating
                && let Kind::Shell(model) = &mut f.panel.kind
            {
                clipboard::key(
                    model,
                    Keysym::Return,
                    self.modifiers.ctrl,
                    self.modifiers.shift,
                )
            } else {
                clipboard::Input::Unhandled
            };
            self.clipboard_input(input);
            return;
        }
        let command = self.floating.as_ref().and_then(|f| match &f.panel.kind {
            Kind::Shell(model) => model.command(),
            _ => None,
        });
        let Some(command) = command.filter(|arguments| !arguments.is_empty()) else {
            return;
        };
        if command
            .first()
            .is_some_and(|value| value == "picker-choose")
        {
            if let Some(f) = &self.floating
                && let Kind::Shell(model) = &f.panel.kind
                && let Some(request) = &model.picker
            {
                request.finish(command.get(1).cloned());
            }
            self.close();
        } else if command.first().is_some_and(|value| value == "provider") {
            if let Some(provider) = command.get(1) {
                self.remember_parent();
                self.show_provider(provider, "");
            }
        } else if command
            .first()
            .is_some_and(|value| value == "search-activate")
            && command.len() == 5
        {
            let keep = self.modifiers.ctrl || search::keep_open(&command[1], &command[3]);
            if self.floating.as_ref().is_some_and(|f| matches!(&f.panel.kind, Kind::Shell(model) if model.provider.starts_with("actions/"))) {
                self.back();
            }
            if let Some(f) = &mut self.floating
                && let Kind::Shell(model) = &mut f.panel.kind
            {
                if command[1] == "todo" {
                    if matches!(command[3].as_str(), "create" | "search") {
                        model.query.clear();
                    } else if matches!(command[3].as_str(), "save" | "save_next") {
                        model.clear_after_action = Some(model.query.clone());
                    }
                }
                model.refreshing = true;
                self.shell.native_action(command, model.generation);
            }
            if !keep {
                self.close();
            }
        } else if command.first().is_some_and(|value| value == "native") {
            if let Some(f) = &mut self.floating
                && let Kind::Shell(model) = &mut f.panel.kind
            {
                let mut arguments = command[1..].to_vec();
                if self.modifiers.ctrl
                    && model.page == shell::Page::Audio
                    && let Some(&i) = model.filtered.get(model.selected)
                {
                    let item = &model.entries[i];
                    if item.category == "outputs" || item.category == "inputs" {
                        arguments = vec![
                            "audio".into(),
                            if item.category == "outputs" {
                                "default-output"
                            } else {
                                "default-input"
                            }
                            .into(),
                            item.keywords.clone(),
                            String::new(),
                        ];
                    }
                }
                model.refreshing = true;
                self.shell.native_action(arguments, model.generation);
            }
        } else if command
            .first()
            .is_some_and(|value| value == "notification-detail")
        {
            self.remember_parent();
            if let Some(index) = self.focused_bar() {
                self.open_notch(index, Kind::Tooltip(command[1..].join("\n")));
            }
        } else if command == ["exec", "desktop-ring", "menu"] {
            self.remember_parent();
            self.command("controls");
        } else if command.first().is_some_and(|value| value == "keybindings") {
            self.remember_parent();
            self.show_page(shell::Page::Keybindings, false);
        } else if command.first().is_some_and(|value| value == "launch")
            && command.len() == 2
            && ["audio", "bluetooth", "wifi"].contains(&command[1].as_str())
        {
            self.remember_parent();
            self.command(&command[1]);
        } else if command.first().is_some_and(|value| value == "panel") {
            if let Some(page) = command.get(1) {
                self.remember_parent();
                self.command(page);
            }
        } else if command.first().is_some_and(|value| value == "menu") {
            if let Some(group) = command.get(1) {
                self.remember_parent();
                self.show_menu(group);
            }
        } else if command.first().is_some_and(|value| value == "launcher") && command.len() == 1 {
            self.remember_parent();
            self.show_page(shell::Page::Apps, false);
        } else if command == ["wallpaper", "grid"] {
            self.remember_parent();
            self.show_page(shell::Page::Wallpapers, false);
        } else if command == ["clipboard"] {
            self.remember_parent();
            self.show_page(shell::Page::Clipboard, false);
        } else if command.first().is_some_and(|value| value == "project-open")
            && let Some(url) = command.get(1)
        {
            // The picker reuses an open ChatGPT window or starts the browser
            // itself, without the desktopctl hop.
            spawn(&["project-picker", "--open", url]);
            self.close();
        } else {
            let mut arguments = vec!["desktopctl"];
            arguments.extend(command.iter().map(String::as_str));
            spawn(&arguments);
            self.close();
        }
    }
    fn key(&mut self, event: KeyEvent) {
        if let Some(f) = &mut self.floating
            && let Kind::Prompt(model) = &mut f.panel.kind
        {
            match event.keysym {
                Keysym::Return | Keysym::KP_Enter | Keysym::Escape => {
                    let _ = model.request.response.try_send(
                        (event.keysym != Keysym::Escape).then(|| std::mem::take(&mut model.value)),
                    );
                    f.panel.kind = *model.previous.clone();
                }
                Keysym::BackSpace => {
                    model.value.pop();
                }
                Keysym::u if self.modifiers.ctrl => model.value.clear(),
                _ if !self.modifiers.ctrl && !self.modifiers.alt && !self.modifiers.logo => {
                    if let Some(value) = event.utf8
                        && model.value.len() + value.len() <= 1024
                        && !value.chars().any(char::is_control)
                    {
                        model.value.push_str(&value);
                    }
                }
                _ => {}
            }
            f.canvas.request(f.window.wl_surface(), &self.qh);
            return;
        }
        if event.keysym == Keysym::question
            && !self.modifiers.ctrl
            && !self.modifiers.alt
            && !self.modifiers.logo
        {
            self.show_help();
            return;
        }
        if self.modifiers.ctrl
            && !self.modifiers.alt
            && !self.modifiers.logo
            && event.keysym == Keysym::o
        {
            self.show_actions();
            return;
        }
        if self.modifiers.ctrl
            && !self.modifiers.alt
            && !self.modifiers.logo
            && event.keysym == Keysym::y
        {
            self.activate();
            return;
        }
        if let Some(f) = &mut self.floating
            && !f.panel.closing
            && let Kind::Shell(model) = &mut f.panel.kind
            && model.page == shell::Page::Controls
        {
            let focused = match event.keysym {
                Keysym::Escape if model.search_focused => Some(false),
                Keysym::l if self.modifiers.ctrl && !self.modifiers.alt && !self.modifiers.logo => {
                    Some(true)
                }
                Keysym::slash
                    if !model.search_focused
                        && !self.modifiers.ctrl
                        && !self.modifiers.alt
                        && !self.modifiers.logo =>
                {
                    Some(true)
                }
                _ => None,
            };
            if let Some(focused) = focused {
                model.search_focused = focused;
                f.canvas.request(f.window.wl_surface(), &self.qh);
                return;
            }
        }
        if !self.modifiers.alt && !self.modifiers.logo {
            let input = if let Some(f) = &mut self.floating
                && !f.panel.closing
                && let Kind::Shell(model) = &mut f.panel.kind
                && model.page == shell::Page::Clipboard
            {
                clipboard::key(
                    model,
                    event.keysym,
                    self.modifiers.ctrl,
                    self.modifiers.shift,
                )
            } else {
                clipboard::Input::Unhandled
            };
            if !matches!(input, clipboard::Input::Unhandled) {
                self.clipboard_input(input);
                return;
            }
        }
        if event.keysym == Keysym::Escape {
            self.back();
            return;
        }
        let Some(f) = &mut self.floating else {
            return;
        };
        if f.panel.closing {
            return;
        }
        let keysym = input::navigation_key(
            event.keysym,
            self.modifiers.ctrl,
            self.modifiers.alt,
            self.modifiers.logo,
        );
        if let Kind::Menu { entries, .. } = &f.panel.kind {
            if matches!(
                keysym,
                Keysym::Down | Keysym::Up | Keysym::Next | Keysym::Prior
            ) {
                let current = f.panel.hover.unwrap_or(0) as i32;
                let step = if matches!(keysym, Keysym::Down | Keysym::Next) {
                    1
                } else {
                    -1
                };
                let mut next = current;
                let amount = if matches!(keysym, Keysym::Next | Keysym::Prior) {
                    8
                } else {
                    1
                };
                let mut remaining = amount;
                for _ in 0..entries.len() {
                    next = (next + step).clamp(0, entries.len().saturating_sub(1) as i32);
                    if entries
                        .get(next as usize)
                        .is_some_and(|entry| entry.enabled && !entry.separator)
                    {
                        remaining -= 1;
                        if remaining == 0 {
                            break;
                        }
                    }
                }
                f.panel.hover = Some(next as usize);
                f.panel.scroll = (next as usize).saturating_sub(15);
                f.canvas.request(f.window.wl_surface(), &self.qh);
            } else if keysym == Keysym::Return
                && let Some(row) = f.panel.hover
            {
                let y = 48.0 + (row - f.panel.scroll) as f32 * 28.0 + 14.0;
                let x = if f.morph.is_some() {
                    f.canvas.width as f32 / self.theme.scale / 2.0
                } else {
                    160.0
                };
                self.popup_click(x, y);
            }
            return;
        }
        let Kind::Shell(model) = &mut f.panel.kind else {
            return;
        };
        let keysym = input::navigation_key(
            event.keysym,
            self.modifiers.ctrl,
            self.modifiers.alt,
            self.modifiers.logo,
        );
        let keysym = if model.page == shell::Page::Controls
            && !model.search_focused
            && !self.modifiers.ctrl
            && !self.modifiers.alt
            && !self.modifiers.logo
        {
            match keysym {
                Keysym::h => Keysym::Left,
                Keysym::j => Keysym::Down,
                Keysym::k => Keysym::Up,
                Keysym::l => Keysym::Right,
                _ => keysym,
            }
        } else {
            keysym
        };
        if model.page == shell::Page::Audio && matches!(keysym, Keysym::Tab | Keysym::ISO_Left_Tab)
        {
            model.section = (model.section + if self.modifiers.shift { 4 } else { 1 }) % 5;
            model.filter();
            f.canvas.request(f.window.wl_surface(), &self.qh);
            return;
        }
        if model.page == shell::Page::Notifications
            && keysym == Keysym::Delete
            && let Some(&i) = model.filtered.get(model.selected)
            && model.entries[i].keywords == "active"
            && let Some(id) = model.entries[i].id.strip_prefix("notification/")
        {
            self.shell.native_action(
                vec!["notifications".into(), "dismiss".into(), id.into()],
                model.generation,
            );
            return;
        }
        if model.page == shell::Page::Audio && matches!(keysym, Keysym::Left | Keysym::Right) {
            if let Some(&i) = model.filtered.get(model.selected)
                && let Some(level) = model.entries[i].level
            {
                let value = (level as i32 + if keysym == Keysym::Right { 5 } else { -5 })
                    .clamp(0, 100) as u32;
                model.entries[i].level = Some(value);
                let mut args = model.entries[i].command[1..].to_vec();
                args[1] = format!("{}-volume", model.entries[i].category);
                args[3] = value.to_string();
                self.shell.native_action(args, model.generation);
                f.canvas.request(f.window.wl_surface(), &self.qh);
            }
            return;
        }
        let previous_query = model.query.clone();
        match keysym {
            Keysym::Return | Keysym::KP_Enter => {
                self.activate();
                return;
            }
            Keysym::Tab | Keysym::ISO_Left_Tab => {
                let current = shell::Page::ALL
                    .iter()
                    .position(|&page| page == model.page)
                    .unwrap_or(0);
                let count = shell::Page::ALL.len();
                let next = if self.modifiers.shift {
                    (current + count - 1) % count
                } else {
                    (current + 1) % count
                };
                self.show_page(shell::Page::ALL[next], false);
                return;
            }
            Keysym::Down => model.navigate(if model.page == shell::Page::Controls {
                4
            } else {
                1
            }),
            Keysym::Up => model.navigate(if model.page == shell::Page::Controls {
                -4
            } else {
                -1
            }),
            Keysym::Next => model.navigate(model.half_page()),
            Keysym::Prior => model.navigate(-model.half_page()),
            Keysym::Right => model.navigate(1),
            Keysym::Left => model.navigate(-1),
            Keysym::Home => model.navigate(-(model.filtered.len() as i32)),
            Keysym::End => model.navigate(model.filtered.len() as i32),
            Keysym::BackSpace => {
                model.query.pop();
                model.filter();
            }
            _ if self.modifiers.ctrl && event.keysym == Keysym::u => {
                model.query.clear();
                model.filter();
            }
            _ if !self.modifiers.ctrl && !self.modifiers.alt && !self.modifiers.logo => {
                if let Some(value) = event.utf8
                    && model.query.len() < 256
                    && !value.chars().any(char::is_control)
                {
                    model.query.push_str(&value);
                    model.search_focused = true;
                    model.filter();
                }
            }
            _ => return,
        }
        if model.query != previous_query {
            if model.page == shell::Page::Apps
                && let Some(query) = model.query.strip_prefix('$').map(str::to_owned)
            {
                model.query.clear();
                model.filter();
                self.remember_parent();
                self.show_provider("clipboard", query.trim_start());
                return;
            }
            self.shell.query(model);
        }
        self.shell.images(model);
        f.canvas.request(f.window.wl_surface(), &self.qh);
    }
    fn show_help(&mut self) {
        let Some(f) = self.floating.as_ref().filter(|f| !f.panel.closing) else {
            return;
        };
        if matches!(f.panel.kind, Kind::Help(_)) {
            self.back();
            return;
        }
        let Some(model) = help::Model::for_panel(&f.panel.kind) else {
            return;
        };
        let Some(index) = self.bars.iter().position(|bar| bar.name == f.name) else {
            return;
        };
        self.remember_parent();
        self.shell.cancel();
        self.open_notch(index, Kind::Help(model));
    }
    fn clipboard_input(&mut self, input: clipboard::Input) {
        match input {
            clipboard::Input::Close => {
                self.back();
                return;
            }
            clipboard::Input::Action(action) => {
                let close = matches!(
                    &action,
                    clipboard::Action::Paste { .. }
                        | clipboard::Action::Copy(_)
                        | clipboard::Action::Files(_)
                );
                self.shell.clipboard_action(action);
                if close {
                    self.close();
                    return;
                }
            }
            clipboard::Input::Reload => {
                if let Some(f) = &mut self.floating
                    && let Kind::Shell(model) = &mut f.panel.kind
                {
                    model.generation = self.shell.load(shell::Page::Clipboard);
                    model.loading = true;
                    model.error.clear();
                }
            }
            _ => {}
        }
        if let Some(f) = &mut self.floating
            && let Kind::Shell(model) = &mut f.panel.kind
        {
            self.shell.images(model);
            f.canvas.request(f.window.wl_surface(), &self.qh);
        }
    }
    fn content_position(&self, x: f32, y: f32) -> (f32, f32) {
        if let Some(f) = &self.floating
            && f.morph.is_some()
        {
            let (w, h) = f.panel.size();
            return (
                x - (f.canvas.width as f32 / self.theme.scale - w as f32) / 2.0,
                y - if self.theme.motion() {
                    0.0
                } else {
                    (f.canvas.height as f32 / self.theme.scale - h as f32) / 2.0
                },
            );
        }
        (x, y)
    }
    fn floating_contains(&self, x: f32, y: f32) -> bool {
        let Some(f) = &self.floating else {
            return false;
        };
        let Some(morph) = &f.morph else {
            return true;
        };
        self.morph_body(f, morph).contains(x, y)
    }
    /// The painted notch body in bar coordinates.
    fn morph_body(&self, f: &Floating, morph: &Morph) -> Rect {
        let (width, height) = morph.dimensions();
        let width = (width - 32.0).max(1.0);
        let height = (height - 16.0).max(1.0);
        Rect::new(
            (f.canvas.width as f32 / self.theme.scale - width) / 2.0,
            if self.theme.motion() {
                0.0
            } else {
                (f.canvas.height as f32 / self.theme.scale - height) / 2.0
            },
            width,
            height,
        )
    }
    fn in_sticky(&self, x: f32, y: f32) -> bool {
        self.floating.as_ref().is_some_and(|f| {
            !f.panel.closing
                && f.morph
                    .as_ref()
                    .and_then(|m| m.sticky)
                    .is_some_and(|r| r.contains(x, y))
        })
    }
    /// Remembers the current panel area before a click can resize it.
    fn remember_click_area(&mut self) {
        let Some(f) = &self.floating else { return };
        let Some(morph) = &f.morph else { return };
        let body = self.morph_body(f, morph);
        if let Some(morph) = self.floating.as_mut().and_then(|f| f.morph.as_mut()) {
            morph.sticky = Some(body);
        }
    }
    fn open_notch(&mut self, index: usize, kind: Kind) {
        self.open_notch_with_focus(index, kind, KeyboardInteractivity::Exclusive);
    }
    fn remember_parent(&mut self) {
        if let Some(f) = &mut self.floating
            && !f.panel.closing
            && f.morph.is_some()
        {
            if f.parents.len() == 16 {
                f.parents.remove(0);
            }
            f.parents.push(f.panel.clone());
        }
    }
    fn back(&mut self) {
        let Some(f) = &mut self.floating else { return };
        if f.panel.closing {
            return;
        }
        if let Kind::Menu {
            address,
            parent,
            parents,
            ..
        } = &mut f.panel.kind
            && let Some(previous) = parents.pop()
        {
            *parent = previous;
            f.panel.hover = None;
            f.panel.scroll = 0;
            self.tray.send(TrayAction::Menu(address.clone(), previous));
            return;
        }
        let Some(mut previous) = f.parents.pop() else {
            self.close();
            return;
        };
        let Some(index) = self.bars.iter().position(|bar| bar.name == f.name) else {
            self.close();
            return;
        };
        if let Kind::Shell(model) = &mut previous.kind {
            self.shell.resume(model);
            model.requested.clear();
            self.shell.images(model);
        } else {
            self.shell.cancel();
        }
        self.open_notch(index, previous.kind);
        if let Some(f) = &mut self.floating {
            f.panel.month = previous.month;
            f.panel.hover = previous.hover;
            f.panel.scroll = previous.scroll;
            f.panel.origin = previous.origin;
            if let Kind::Menu {
                address, parent, ..
            } = &f.panel.kind
            {
                self.tray.send(TrayAction::Menu(address.clone(), *parent));
            }
        }
    }
    fn return_to(&mut self, matches: impl Fn(&Kind) -> bool) -> bool {
        let Some(f) = self.floating.as_mut().filter(|f| !f.panel.closing) else {
            return false;
        };
        let Some(index) = f.parents.iter().rposition(|parent| matches(&parent.kind)) else {
            return false;
        };
        f.parents.truncate(index + 1);
        self.back();
        true
    }
    fn open_notch_with_focus(
        &mut self,
        index: usize,
        mut kind: Kind,
        keyboard_mode: KeyboardInteractivity,
    ) {
        if let Kind::Shell(model) = &mut kind {
            model.sync_media(&self.data);
        }
        self.hover_generation = self.hover_generation.wrapping_add(1);
        self.hover_pending = None;
        self.overview_hover = None;
        self.panel_scroll = input::Scroll::default();
        self.drag = None;
        self.audio_drag = None;
        self.tray.send(TrayAction::CloseMenu);
        let name = self.bars[index].name.clone();
        if let Some(f) = &mut self.floating
            && f.name == name
            && f.morph.is_some()
        {
            let morph = f.morph.as_mut().unwrap();
            morph.auto_hide = morph.pointer_inside;
            morph.keyboard.set_keyboard_interactivity(keyboard_mode);
            morph.backdrop.layer.wl_surface().set_input_region(None);
            morph.backdrop.layer.commit();
            morph.keyboard.commit();
            let reversing = f.panel.closing.then(|| f.panel.growth.clone());
            if f.panel.closing {
                f.parents.clear();
            }
            f.panel = Panel::new(kind);
            f.panel
                .growth
                .tune(if self.theme.motion() { 22.0 } else { 40.0 }, 0.94);
            f.last = Instant::now();
            if let Some(growth) = reversing {
                f.panel.growth = growth;
            } else {
                f.panel.growth.snap(0.25);
            }
            f.panel.growth.target = 1.0;
            f.canvas.request(f.window.wl_surface(), &self.qh);
            if f.canvas.configured {
                self.paint_popup();
            }
            return;
        }
        let previous = self
            .floating
            .as_ref()
            .filter(|f| matches!(f.window, FloatingWindow::Root(_)))
            .map(|f| f.name.clone());
        self.floating = None;
        if let Some(previous) = previous
            && let Some(i) = self.bars.iter().position(|b| b.name == previous)
        {
            self.bars[i].view.force = true;
            self.paint_bar(i);
        }
        let bar = &self.bars[index];
        let group = if self.theme.motion() {
            bar.view.layout.groups.first().copied()
        } else {
            Some(bar.view.layout.workspace)
        }
        .unwrap_or(Rect::new(
            bar.canvas.width as f32 / self.theme.scale / 2.0 - 190.0,
            0.0,
            380.0,
            36.0,
        ));
        let anchor = Rect::new(
            group.x * self.theme.scale,
            0.0,
            group.w * self.theme.scale,
            0.0,
        );
        let pixel_scale = self.theme.scale * bar.canvas.scale;
        let closed_image = if self.theme.motion() {
            bar.view.motion_content(
                &mut self.text,
                &self.data,
                &self.theme,
                pixel_scale,
                bar.canvas.width as f32 / self.theme.scale,
            )
        } else {
            None
        };
        let mut panel = Panel::new(kind);
        panel
            .growth
            .tune(if self.theme.motion() { 22.0 } else { 40.0 }, 0.94);
        // A stable popup envelope keeps the anchor fixed while only its painted
        // path morphs. No configure/reposition round trips are needed per frame.
        let (w, h) = if self.theme.motion() {
            (bar.canvas.width, bar.canvas.height)
        } else {
            (
                (880.0 * self.theme.scale)
                    .round()
                    .min(bar.canvas.width as f32) as u32,
                (560.0 * self.theme.scale).round() as u32,
            )
        };
        let closed = if self.theme.motion() {
            (group.w + 32.0, group.h + 16.0)
        } else {
            (60.0, 44.0)
        };
        let backdrop_surface = self.compositor.create_surface(&self.qh);
        let Ok(backdrop_canvas) = Canvas::new(
            &backdrop_surface,
            &self.qh,
            &self.shm,
            self.viewporter.as_ref(),
            None,
            1,
            1,
            1.0,
        ) else {
            return;
        };
        if backdrop_canvas.viewport.is_none() {
            return;
        }
        let layer = self.layers.create_layer_surface(
            &self.qh,
            backdrop_surface,
            if self.theme.motion() {
                Layer::Top
            } else {
                Layer::Overlay
            },
            Some(format!("desktop-bar-panel-{name}")),
            Some(&bar.output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, 0);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        let window = if self.theme.motion() {
            FloatingWindow::Root(bar.surface().clone())
        } else {
            let surface = self.compositor.create_surface(&self.qh);
            let menu = self.layers.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some(format!("desktop-bar-menu-{name}")),
                Some(&bar.output),
            );
            menu.set_anchor(Anchor::empty());
            menu.set_size(w, h);
            menu.set_exclusive_zone(-1);
            menu.set_keyboard_interactivity(KeyboardInteractivity::None);
            menu.commit();
            FloatingWindow::Overlay(menu)
        };
        let keyboard = if let FloatingWindow::Overlay(menu) = &window {
            menu.clone()
        } else {
            bar.layer.clone()
        };
        keyboard.set_keyboard_interactivity(keyboard_mode);
        keyboard.commit();
        let canvas = if self.theme.motion() {
            Canvas::shared(&bar.canvas, &self.shm)
        } else {
            Canvas::new(
                window.wl_surface(),
                &self.qh,
                &self.shm,
                self.viewporter.as_ref(),
                self.fractional.as_ref(),
                w,
                h,
                bar.canvas.scale,
            )
        };
        let Ok(canvas) = canvas else {
            return;
        };
        window.wl_surface().commit();
        self.floating = Some(Floating {
            canvas,
            panel,
            parents: vec![],
            window,
            name,
            anchor,
            last: Instant::now(),
            content: None,
            content_stale: false,
            mask: None,
            painted: None,
            morph: Some(Morph {
                width: anim::Spring::new(
                    closed.0,
                    if self.theme.motion() { 26.0 } else { 34.0 },
                    if self.theme.motion() { 0.94 } else { 1.0 },
                ),
                height: anim::Spring::new(
                    closed.1,
                    if self.theme.motion() { 22.0 } else { 32.0 },
                    if self.theme.motion() { 0.88 } else { 1.0 },
                ),
                closed,
                closed_image,
                closed_dirty: false,
                backdrop: Backdrop {
                    canvas: backdrop_canvas,
                    layer,
                },
                keyboard,
                auto_hide: bar.view.hover.is_some(),
                pointer_inside: bar.view.hover.is_some(),
                sticky: None,
            }),
        });
        if let Some(f) = &mut self.floating
            && matches!(f.window, FloatingWindow::Root(_))
        {
            f.canvas.configured = true;
            self.paint_popup();
        }
    }
    fn sync_outputs(&mut self) {
        let outputs: Vec<_> = self.outputs.outputs().collect();
        self.bars.retain(|bar| {
            outputs.contains(&bar.output)
                && bar.motion == (self.theme.motion() && !self.theme.minimal(&bar.name))
                && self.outputs.info(&bar.output).is_some_and(|i| {
                    self.theme
                        .selected(&bar.name, i.description.as_deref().unwrap_or(""))
                })
        });
        for output in outputs {
            let Some(info) = self.outputs.info(&output) else {
                continue;
            };
            let Some(name) = info.name else {
                continue;
            };
            if !self
                .theme
                .selected(&name, info.description.as_deref().unwrap_or(""))
            {
                continue;
            }
            let motion = self.theme.motion() && !self.theme.minimal(&name);
            let width = info
                .logical_size
                .map(|s| s.0)
                .unwrap_or_else(|| {
                    info.modes
                        .iter()
                        .find(|m| m.current)
                        .map(|m| m.dimensions.0 / info.scale_factor.max(1))
                        .unwrap_or(1920)
                })
                .max(1) as u32;
            let width = if motion {
                width.min((960.0 * self.theme.scale).round() as u32)
            } else {
                width
            };
            let height = ((if motion { 560.0 } else { 56.0 }) * self.theme.scale).round() as u32;
            if let Some(bar) = self.bars.iter_mut().find(|bar| bar.output == output) {
                bar.minimal = self.theme.minimal(&name);
                let surface = bar.surface().clone();
                if bar
                    .canvas
                    .resize(&surface, width, height, bar.canvas.scale, &self.shm)
                {
                    bar.view.force = true;
                }
                bar.layer.set_size(
                    if motion { width } else { 0 },
                    if motion {
                        (560.0 * self.theme.scale).round() as u32
                    } else {
                        height
                    },
                );
                bar.layer
                    .set_layer(if self.preview || (self.theme.motion() && !bar.minimal) {
                        Layer::Overlay
                    } else {
                        Layer::Top
                    });
                bar.layer
                    .set_exclusive_zone(if self.preview || self.menus_only {
                        -1
                    } else {
                        (if motion {
                            44.0 * self.theme.scale
                        } else {
                            height as f32
                        })
                        .round() as i32
                    });
                bar.canvas.request(&surface, &self.qh);
                continue;
            }
            let root = self.compositor.create_surface(&self.qh);
            let canvas = match Canvas::new(
                &root,
                &self.qh,
                &self.shm,
                self.viewporter.as_ref(),
                self.fractional.as_ref(),
                width,
                height,
                info.scale_factor.max(1) as f32,
            ) {
                Ok(canvas) => canvas,
                Err(error) => {
                    eprintln!("desktop-bar: {error}");
                    continue;
                }
            };
            let layer = self.layers.create_layer_surface(
                &self.qh,
                root,
                if self.preview || motion {
                    Layer::Overlay
                } else {
                    Layer::Top
                },
                Some(format!("desktop-bar-{name}")),
                Some(&output),
            );
            layer.set_size(if motion { width } else { 0 }, height);
            layer.set_anchor(if motion {
                Anchor::TOP
            } else {
                Anchor::TOP | Anchor::LEFT | Anchor::RIGHT
            });
            layer.set_margin(if self.preview { 70 } else { 0 }, 0, 0, 0);
            layer.set_exclusive_zone(if self.preview || self.menus_only {
                -1
            } else {
                (if motion {
                    44.0 * self.theme.scale
                } else {
                    height as f32
                })
                .round() as i32
            });
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            self.bars.push(Bar {
                canvas,
                view: render::View::new(motion),
                layer,
                output,
                name: name.clone(),
                minimal: self.theme.minimal(&name),
                motion,
            });
        }
        let _ = self.workspaces.send(
            self.bars
                .iter()
                .filter(|b| !b.minimal)
                .map(|b| b.name.clone())
                .collect(),
        );
    }
    fn request_all(&mut self) {
        self.request_views(|_| true, true);
    }
    fn invalidate_closed_notch(&mut self) {
        if let Some(f) = &mut self.floating
            && matches!(f.window, FloatingWindow::Root(_))
            && let Some(morph) = &mut f.morph
        {
            morph.closed_dirty = true;
        }
    }
    fn refresh_closed_notch(&mut self) {
        let Some(f) = &mut self.floating else { return };
        let Some(morph) = &mut f.morph else { return };
        let Some(bar) = self.bars.iter_mut().find(|b| b.name == f.name) else {
            return;
        };
        bar.view.settle_workspaces();
        let layout = layout::Layout::motion(
            bar.canvas.width as f32 / self.theme.scale,
            &self.data,
            &self.theme,
            &bar.view.visible_workspaces(),
        );
        let body = layout.groups[0];
        morph.closed = (body.w + 32.0, body.h + 16.0);
        morph.closed_image = bar.view.motion_content(
            &mut self.text,
            &self.data,
            &self.theme,
            self.theme.scale * bar.canvas.scale,
            bar.canvas.width as f32 / self.theme.scale,
        );
        bar.view.layout = layout;
        morph.closed_dirty = false;
    }
    fn request_views(&mut self, visible: impl Fn(&Bar) -> bool, panel: bool) {
        let active = self
            .floating
            .as_ref()
            .filter(|f| matches!(f.window, FloatingWindow::Root(_)))
            .map(|f| f.name.as_str());
        for bar in &mut self.bars {
            if active == Some(bar.name.as_str()) || !visible(bar) {
                continue;
            }
            let surface = bar.surface().clone();
            bar.canvas.request(&surface, &self.qh);
        }
        if panel
            && let Some(f) = &mut self.floating
            && !f.panel.closing
        {
            f.canvas.request(f.window.wl_surface(), &self.qh);
        }
    }
    fn update(&mut self, update: Update) {
        let panel_changed = self
            .floating
            .as_ref()
            .is_some_and(|f| f.panel.kind.observes(&update));
        let compact_source = matches!(
            &update,
            Update::Battery(_)
                | Update::Network(_)
                | Update::NetworkSignals(_)
                | Update::Tray(_)
                | Update::Audio(false, _)
        );
        let detailed_changed = compact_source
            || matches!(
                &update,
                Update::Cpu(_)
                    | Update::Memory(_)
                    | Update::Disk(_)
                    | Update::Weather(_)
                    | Update::Audio(..)
            );
        let compact_before = compact_source.then(|| render::motion_data_signature(&self.data));
        match update {
            Update::Picker(request) => {
                self.show_picker(request);
                return;
            }
            Update::Command(command, output) => {
                self.focused_output = output;
                self.command(&command);
                return;
            }
            Update::Prompt(request) => {
                if let Some(f) = &mut self.floating
                    && !f.panel.closing
                    && let Some(morph) = &mut f.morph
                {
                    let previous =
                        std::mem::replace(&mut f.panel.kind, Kind::Tooltip(String::new()));
                    f.panel.kind = Kind::Prompt(prompt::Model {
                        request,
                        value: String::new(),
                        previous: Box::new(previous),
                    });
                    morph.auto_hide = false;
                    morph
                        .keyboard
                        .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                    morph.keyboard.commit();
                    f.canvas.request(f.window.wl_surface(), &self.qh);
                } else {
                    let _ = request.response.try_send(None);
                }
                return;
            }
            Update::Shell(reply) => {
                self.shell_reply(reply);
                return;
            }
            Update::Cpu(v) => self.data.cpu = v,
            Update::Memory(v) => self.data.memory = v,
            Update::Disk(v) => self.data.disk = v,
            Update::Battery(v) => self.data.battery = v,
            Update::Weather(v) => self.data.weather = v,
            Update::Network(v) => self.data.network = v,
            Update::Media(mut media) => {
                if media.art_url == self.data.media.art_url {
                    media.art = self.data.media.art.clone();
                }
                self.data.media = media;
                if let Some(f) = &mut self.floating
                    && let Kind::Shell(model) = &mut f.panel.kind
                {
                    model.sync_media(&self.data);
                }
            }
            Update::MediaArt(url, image) => {
                if self.data.media.art_url != url {
                    return;
                }
                self.data.media.art = image;
            }
            Update::NetworkSignals(signals) => {
                self.data.network.rssi = signals.get(&self.data.network.interface).copied();
            }
            Update::Audio(mic, v) => {
                if mic {
                    self.data.microphone = v
                } else {
                    self.data.audio = v
                }
            }
            Update::Tray(items) => {
                self.data.tray = items;
                for reply in self.tray.menus.lock().unwrap().drain(..) {
                    if let Some(f) = &mut self.floating
                        && let Kind::Menu {
                            address,
                            parent,
                            entries,
                            ..
                        } = &mut f.panel.kind
                        && address == &reply.address
                        && *parent == reply.parent
                    {
                        *entries = reply.entries;
                        f.panel.scroll = 0;
                    }
                }
            }
            Update::Workspaces(name, snapshot) => {
                if let Some(b) = self.bars.iter_mut().find(|b| b.name == name) {
                    b.view.snapshot(snapshot);
                    if let Some(f) = &mut self.floating
                        && f.name == name
                    {
                        if matches!(f.window, FloatingWindow::Root(_))
                            && let Some(morph) = &mut f.morph
                        {
                            morph.closed_dirty = true;
                        }
                        if let Kind::Overview(model) = &mut f.panel.kind {
                            model.workspaces = b.view.visible_workspaces();
                            model.active = b.view.active;
                            f.canvas.request(f.window.wl_surface(), &self.qh);
                        }
                    }
                    if self.trace_frames {
                        eprintln!(
                            "bar-workspaces output={name} visible={:?}",
                            b.view.visible_workspaces()
                        );
                    }
                    if !self.floating.as_ref().is_some_and(|f| {
                        f.name == name && matches!(f.window, FloatingWindow::Root(_))
                    }) {
                        let surface = b.surface().clone();
                        b.canvas.request(&surface, &self.qh);
                    }
                }
                return;
            }
        }
        // The compact notch draws only a few fields; most status samples
        // leave it unchanged and need no frame.
        let compact_changed = compact_before
            .is_some_and(|before| before != render::motion_data_signature(&self.data));
        if compact_changed {
            self.invalidate_closed_notch();
        }
        self.request_views(
            |bar| {
                !bar.minimal
                    && if bar.motion {
                        compact_changed
                    } else {
                        detailed_changed
                    }
            },
            panel_changed,
        );
    }
    fn reload(&mut self) {
        self.hover_generation = self.hover_generation.wrapping_add(1);
        self.hover_pending = None;
        self.overview_hover = None;
        self.tray.send(TrayAction::CloseMenu);
        let next = Theme::read();
        self.tray.send(TrayAction::Foreground(next.text()));
        let style_changed = next.style != self.theme.style;
        self.text.reload(&next.font);
        for b in &mut self.bars {
            if style_changed {
                b.view.fade = Some((b.canvas.pixmap.clone(), Instant::now()));
            }
            b.view.force = true;
        }
        self.floating = None;
        self.shell.cancel();
        self.theme = next;
        self.sync_outputs();
        self.request_all();
    }
    fn paint_bar(&mut self, index: usize) {
        if self.floating.as_ref().is_some_and(|f| {
            matches!(f.window, FloatingWindow::Root(_)) && f.name == self.bars[index].name
        }) {
            self.paint_popup();
            return;
        }
        let bar = &mut self.bars[index];
        let surface = bar.surface().clone();
        if self.menus_only {
            bar.canvas.pixmap.fill(tiny_skia::Color::TRANSPARENT);
            if let Ok(region) = Region::new(&self.compositor) {
                surface.set_input_region(Some(region.wl_region()));
            }
            let damage = [Rect::new(
                0.0,
                0.0,
                bar.canvas.width as f32,
                bar.canvas.height as f32,
            )];
            bar.canvas.present(&surface, &self.qh, &damage, false);
            return;
        }
        let fading = bar.view.fade.is_some();
        let scale = self.theme.scale * bar.canvas.scale;
        // A compact notch only paints its top band, so the retained image
        // covers that band instead of the whole transparent envelope.
        if bar
            .canvas
            .limit_rows(bar.motion.then(|| render::motion_rows(scale)))
        {
            bar.view.force = true;
        }
        let full = bar.view.force;
        let mut damage = bar.view.render(
            &mut bar.canvas.pixmap,
            &mut self.text,
            &self.data,
            &self.theme,
            bar.canvas.width as f32 / self.theme.scale,
            bar.minimal,
            scale,
        );
        if full && !damage.is_empty() {
            // Panels drawn into this surface may have covered the lower rows.
            let (width, height) = bar.canvas.buffer_size();
            damage = vec![Rect::new(0.0, 0.0, width as f32, height as f32)];
        }
        if let Ok(region) = Region::new(&self.compositor) {
            for r in &bar.view.layout.groups {
                let r = r.scale(self.theme.scale);
                region.add(
                    r.x.floor() as i32,
                    r.y.floor() as i32,
                    r.w.ceil() as i32,
                    r.h.ceil() as i32,
                );
            }
            surface.set_input_region(Some(region.wl_region()));
        }
        let moving = bar.view.moving(&self.theme);
        bar.canvas.present(&surface, &self.qh, &damage, moving);
        if fading && bar.view.fade.is_none() {
            trim_heap();
        }
    }
    fn positioner(&self, anchor: Rect, width: u32, height: u32) -> Option<XdgPositioner> {
        let p = XdgPositioner::new(&self.xdg).ok()?;
        p.set_size(width.max(1) as i32, height.max(1) as i32);
        p.set_anchor_rect(
            anchor.x as i32,
            anchor.y as i32,
            anchor.w.max(1.0) as i32,
            anchor.h.max(1.0) as i32,
        );
        p.set_anchor(if anchor.h == 0.0 {
            xdg_positioner::Anchor::Top
        } else {
            xdg_positioner::Anchor::Bottom
        });
        p.set_gravity(xdg_positioner::Gravity::Bottom);
        p.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::FlipY
                | xdg_positioner::ConstraintAdjustment::SlideY,
        );
        Some(p)
    }
    fn open(&mut self, index: usize, module: Module, kind: Kind, grab: bool) {
        if (self.theme.motion() && !matches!(kind, Kind::Tooltip(_)))
            || matches!(kind, Kind::Shell(_))
        {
            self.open_notch(index, kind);
            return;
        }
        self.tray.send(TrayAction::CloseMenu);
        self.floating = None;
        self.drag = None;
        self.audio_drag = None;
        let bar = &self.bars[index];
        let Some(cell) = bar
            .view
            .layout
            .cells
            .iter()
            .find(|c| c.label.module == module)
        else {
            return;
        };
        let mut anchor = cell.rect.scale(self.theme.scale);
        if grab {
            anchor.y -= anchor.h;
        }
        let mut panel = Panel::new(kind);
        panel.origin = format!("{} {}", cell.label.icon, cell.label.value);
        if matches!(panel.kind, Kind::Tooltip(_)) {
            panel.growth.snap(1.0);
        }
        let (w, h) = panel.animated_size(anchor.w / self.theme.scale);
        let w = (w * self.theme.scale) as u32;
        let h = (h * self.theme.scale).round().max(1.0) as u32;
        let Some(position) = self.positioner(anchor, w, h) else {
            return;
        };
        let Ok(surface) = Surface::new(&self.compositor, &self.qh) else {
            return;
        };
        let Ok(popup) = Popup::from_surface(None, &position, &self.qh, surface, &self.xdg) else {
            return;
        };
        bar.layer.get_popup(popup.xdg_popup());
        if grab && let Some(seat) = &self.last_seat {
            popup.xdg_popup().grab(seat, self.last_serial);
        }
        let Ok(canvas) = Canvas::new(
            popup.wl_surface(),
            &self.qh,
            &self.shm,
            self.viewporter.as_ref(),
            self.fractional.as_ref(),
            w,
            h,
            bar.canvas.scale,
        ) else {
            return;
        };
        popup.wl_surface().commit();
        self.floating = Some(Floating {
            canvas,
            panel,
            parents: vec![],
            window: FloatingWindow::Popup(popup),
            name: bar.name.clone(),
            anchor,
            last: Instant::now(),
            morph: None,
            content: None,
            content_stale: false,
            mask: None,
            painted: None,
        });
    }
    fn close(&mut self) {
        self.hover_suppressed_until = Instant::now() + Duration::from_millis(350);
        self.dismiss();
    }
    fn dismiss(&mut self) {
        self.invalidate_closed_notch();
        self.hover_generation = self.hover_generation.wrapping_add(1);
        self.hover_pending = None;
        self.overview_hover = None;
        if let Some(f) = &mut self.floating {
            if f.panel.closing {
                return;
            }
            if matches!(f.panel.kind, Kind::Menu { .. }) {
                self.tray.send(TrayAction::CloseMenu);
            }
            for panel in std::iter::once(&f.panel).chain(f.parents.iter()) {
                if let Kind::Shell(model) = &panel.kind
                    && let Some(request) = &model.picker
                {
                    request.finish(None);
                }
            }
            f.panel.close();
            f.last = Instant::now();
            self.shell.cancel();
            if let Some(morph) = &mut f.morph {
                morph
                    .keyboard
                    .set_keyboard_interactivity(KeyboardInteractivity::None);
                if let Ok(region) = Region::new(&self.compositor) {
                    morph
                        .backdrop
                        .layer
                        .wl_surface()
                        .set_input_region(Some(region.wl_region()));
                    f.window
                        .wl_surface()
                        .set_input_region(Some(region.wl_region()));
                }
                morph.backdrop.layer.commit();
                morph.keyboard.commit();
            }
            f.canvas.animate(f.window.wl_surface(), &self.qh);
        }
        self.drag = None;
        self.audio_drag = None;
        if self.floating.as_ref().is_some_and(|f| f.canvas.configured) {
            self.paint_popup();
        }
    }
    /// Runs the one-second refresh only while a live page is shown, so an
    /// idle bar has no periodic wakeups on the main loop.
    fn schedule_refresh(&mut self) {
        if self.refresh_scheduled || !self.refresh_wanted() {
            return;
        }
        self.refresh_scheduled = self
            .handle
            .insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, app| {
                if !app.refresh_wanted() {
                    app.refresh_scheduled = false;
                    return TimeoutAction::Drop;
                }
                if app.audio_drag.is_none()
                    && let Some(f) = &mut app.floating
                    && let Kind::Shell(model) = &mut f.panel.kind
                    && !model.loading
                    && !model.refreshing
                {
                    model.refreshing = true;
                    app.shell.refresh(model);
                }
                TimeoutAction::ToDuration(Duration::from_secs(1))
            })
            .is_ok();
    }
    fn refresh_wanted(&self) -> bool {
        self.floating.as_ref().is_some_and(|f| {
            !f.panel.closing
                && matches!(&f.panel.kind, Kind::Shell(model) if desktop::refresh(model.page))
        })
    }
    fn paint_popup(&mut self) {
        self.schedule_refresh();
        if self.floating.as_ref().is_some_and(|f| {
            f.morph.as_ref().is_some_and(|m| m.closed_dirty)
                && (f.panel.closing || f.panel.growth.value < 0.4)
        }) {
            self.refresh_closed_notch();
        }
        let Some(f) = &mut self.floating else {
            return;
        };
        if f.canvas.dirty {
            f.content_stale = true;
        }
        let now = Instant::now();
        let started = now;
        let dt = now.duration_since(f.last).as_secs_f32();
        f.last = now;
        f.panel.growth.step(dt);
        let (w, h) = if let Some(morph) = &mut f.morph {
            let (w, h) = f.panel.size();
            morph.width.target = if f.panel.closing {
                morph.closed.0
            } else {
                (w as f32 + 32.0)
                    .max(morph.closed.0)
                    .min(f.canvas.width as f32 / self.theme.scale)
            };
            morph.height.target = if f.panel.closing {
                morph.closed.1
            } else {
                (h as f32 + 16.0).min(f.canvas.height as f32 / self.theme.scale)
            };
            morph.width.step(dt);
            morph.height.step(dt);
            morph.dimensions()
        } else {
            f.panel.animated_size(f.anchor.w / self.theme.scale)
        };
        if f.panel.closing
            && if !self.theme.motion()
                && let Some(morph) = &f.morph
            {
                (morph.width.value - morph.closed.0).abs() <= 1.0
                    && (morph.height.value - morph.closed.1).abs() <= 1.0
                    && f.panel.growth.value <= 0.03
            } else {
                !f.panel.growth.moving()
                    && f.morph
                        .as_ref()
                        .is_none_or(|m| !m.width.moving() && !m.height.moving())
            }
        {
            let name = f.name.clone();
            self.floating = None;
            if self.menus_only {
                self.exit = true;
            }
            if let Some(i) = self.bars.iter().position(|b| b.name == name) {
                self.bars[i].view.force = true;
                self.paint_bar(i);
            }
            trim_heap();
            return;
        }
        let scale = self.theme.scale;
        let width = if f.morph.is_some() {
            f.canvas.width
        } else {
            (w * scale).round() as u32
        };
        let height = if f.morph.is_some() {
            f.canvas.height
        } else {
            (h * scale).round().max(1.0) as u32
        };
        let changed = f.canvas.resize(
            f.window.wl_surface(),
            width,
            height,
            f.canvas.scale,
            &self.shm,
        );
        if let Kind::Shell(model) = &mut f.panel.kind
            && model.cursor.moving()
        {
            model.cursor.step(dt);
            f.content_stale = true;
        }
        if changed {
            f.painted = None;
        }
        let damage = if let Some(morph) = &f.morph {
            if let Ok(region) = Region::new(&self.compositor) {
                if !f.panel.closing {
                    region.add(
                        ((width as f32 - w * scale) / 2.0 + 16.0 * scale).floor() as i32,
                        if self.theme.motion() {
                            0
                        } else {
                            ((height as f32 - (h - 16.0) * scale) / 2.0).floor() as i32
                        },
                        ((w - 32.0) * scale).ceil() as i32,
                        ((h - 16.0) * scale).ceil() as i32,
                    );
                    if let Some(sticky) = morph.sticky {
                        let r = sticky.scale(scale);
                        region.add(
                            r.x.floor() as i32,
                            r.y.floor() as i32,
                            r.w.ceil() as i32,
                            r.h.ceil() as i32,
                        );
                    }
                } else if matches!(f.window, FloatingWindow::Root(_)) {
                    let closed = morph.closed;
                    region.add(
                        ((width as f32 - (closed.0 - 32.0) * scale) / 2.0).floor() as i32,
                        0,
                        ((closed.0 - 32.0) * scale).ceil() as i32,
                        ((closed.1 - 16.0) * scale).ceil() as i32,
                    );
                }
                f.window
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
            }
            f.panel.draw_shell(
                &mut f.canvas.pixmap,
                &mut self.text,
                &self.data,
                &self.theme,
                scale * f.canvas.scale,
                (w, h),
                &mut f.content,
                &mut f.content_stale,
                &mut f.mask,
                &mut f.painted,
            )
        } else {
            f.panel.draw(
                &mut f.canvas.pixmap,
                &mut self.text,
                &self.data,
                &self.theme,
                scale * f.canvas.scale,
            );
            Rect::new(
                0.0,
                0.0,
                f.canvas.pixmap.width() as f32,
                f.canvas.pixmap.height() as f32,
            )
        };
        if let Some(morph) = &f.morph
            && let Some(image) = &morph.closed_image
        {
            let opacity = (1.0 - f.panel.growth.value * 2.5).clamp(0.0, 1.0);
            if opacity > 0.0 {
                let x = (f.canvas.pixmap.width() as i32 - image.width() as i32) / 2;
                f.canvas.pixmap.draw_pixmap(
                    x,
                    0,
                    image.as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity,
                        ..Default::default()
                    },
                    tiny_skia::Transform::identity(),
                    f.mask.as_ref(),
                );
            }
        }
        self.text.release_font_pages();
        f.canvas.present(
            f.window.wl_surface(),
            &self.qh,
            &[damage],
            f.panel.growth.moving()
                || f.morph
                    .as_ref()
                    .is_some_and(|m| m.width.moving() || m.height.moving())
                || matches!(&f.panel.kind, Kind::Shell(model) if model.cursor.moving()),
        );
        if changed && let Some(popup) = f.window.popup().cloned() {
            let anchor = f.anchor;
            if let Some(p) = self.positioner(anchor, width, height) {
                popup.reposition(&p, 0);
            }
        }
        if self.trace_frames {
            eprintln!(
                "bar-frame dt_ms={:.3} render_ms={:.3} width={w:.2} height={h:.2}",
                dt * 1000.0,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    fn hover(&mut self, index: usize, module: Option<Module>) {
        if self.bars[index].view.hover == module {
            return;
        }
        self.bars[index].view.hover = module;
        self.scroll = 0.0;
        self.hover_generation = self.hover_generation.wrapping_add(1);
        if self
            .floating
            .as_ref()
            .is_some_and(|f| matches!(f.panel.kind, Kind::Tooltip(_)))
        {
            self.floating = None;
        }
        {
            let bar = &mut self.bars[index];
            let surface = bar.surface().clone();
            bar.canvas.request(&surface, &self.qh);
        }
        let generation = self.hover_generation;
        let name = self.bars[index].name.clone();
        if self.theme.motion() {
            return;
        }
        if let Some(module) = module {
            let _ = self.handle.insert_source(
                Timer::from_duration(Duration::from_millis(650)),
                move |_, _, app| {
                    if app.hover_generation == generation
                        && app.floating.is_none()
                        && let Some(i) = app
                            .bars
                            .iter()
                            .position(|b| b.name == name && b.view.hover == Some(module))
                    {
                        let label = panel::tooltip(module, &app.data);
                        if !label.is_empty() {
                            app.open(i, module, Kind::Tooltip(label), false);
                        }
                    }
                    TimeoutAction::Drop
                },
            );
        }
    }
    fn overview_pointer(&mut self, x: f32, y: f32) {
        let hover = self.floating.as_ref().and_then(|f| {
            if !f.panel.closing
                && let Kind::Overview(model) = &f.panel.kind
                && (88.0..118.0).contains(&y)
                && let Some(overview::Action::Panel(panel)) = model.hit(x, y, &self.data)
            {
                Some(panel)
            } else {
                None
            }
        });
        if hover == self.overview_hover {
            return;
        }
        self.overview_hover = hover;
        let Some(panel) = hover else {
            return;
        };
        let _ = self.handle.insert_source(
            Timer::from_duration(Duration::from_millis(120)),
            move |_, _, app| {
                if app.overview_hover == Some(panel)
                    && app.floating.as_ref().is_some_and(|f| {
                        !f.panel.closing && matches!(f.panel.kind, Kind::Overview(_))
                    })
                {
                    app.command(panel);
                    if let Some(f) = &mut app.floating
                        && let Some(morph) = &mut f.morph
                    {
                        morph.auto_hide = true;
                        morph.pointer_inside = true;
                        morph
                            .keyboard
                            .set_keyboard_interactivity(KeyboardInteractivity::None);
                        morph.keyboard.commit();
                    }
                }
                TimeoutAction::Drop
            },
        );
    }
    fn notch_pointer(&mut self, inside: bool) {
        let Some(f) = &mut self.floating else {
            return;
        };
        let Some(morph) = &mut f.morph else {
            return;
        };
        if matches!(f.panel.kind, Kind::Prompt(_)) {
            morph.auto_hide = false;
            morph.pointer_inside = inside;
            return;
        }
        if f.panel.closing {
            morph.pointer_inside = inside;
            return;
        }
        if morph.pointer_inside == inside && (!inside || morph.auto_hide) {
            return;
        }
        morph.pointer_inside = inside;
        if inside {
            morph.auto_hide = true;
        }
        self.hover_generation = self.hover_generation.wrapping_add(1);
        if inside || !morph.auto_hide {
            return;
        }
        let generation = self.hover_generation;
        let _ = self.handle.insert_source(
            Timer::from_duration(Duration::from_millis(130)),
            move |_, _, app| {
                if app.hover_generation == generation
                    && app.floating.as_ref().is_some_and(|f| {
                        !f.panel.closing
                            && f.morph
                                .as_ref()
                                .is_some_and(|m| m.auto_hide && !m.pointer_inside)
                    })
                {
                    app.dismiss();
                }
                TimeoutAction::Drop
            },
        );
    }
    fn motion_hover(&mut self, index: usize, inside: bool) {
        if !self.theme.motion() || self.bars[index].minimal {
            return;
        }
        let name = self.bars[index].name.clone();
        if self
            .floating
            .as_ref()
            .is_some_and(|f| f.name == name && !f.panel.closing && f.morph.is_some())
        {
            self.hover_pending = None;
            self.overview_hover = None;
            self.notch_pointer(inside);
            return;
        }
        let module = self.bars[index].view.hover;
        let pending = inside.then(|| (name.clone(), module));
        if self.hover_pending == pending {
            return;
        }
        self.hover_pending = pending;
        self.hover_generation = self.hover_generation.wrapping_add(1);
        if !inside
            || matches!(module, Some(Module::Tray(_)))
            || Instant::now() < self.hover_suppressed_until
        {
            return;
        }
        let generation = self.hover_generation;
        let _ = self.handle.insert_source(
            Timer::from_duration(Duration::from_millis(70)),
            move |_, _, app| {
                if app.hover_generation == generation
                    && Instant::now() >= app.hover_suppressed_until
                    && app
                        .hover_pending
                        .as_ref()
                        .is_some_and(|(output, hover)| output == &name && *hover == module)
                {
                    app.focused_output = Some(name.clone());
                    if let Some(index) = app.bars.iter().position(|b| b.name == name) {
                        let kind = match module {
                            Some(Module::Volume | Module::Microphone) => Kind::Volume,
                            Some(Module::Battery) => Kind::Battery,
                            Some(Module::Clock) => app.controls_kind(),
                            Some(Module::Network) => Kind::Network,
                            Some(Module::Weather | Module::Cpu | Module::Memory | Module::Disk) => {
                                Kind::System
                            }
                            _ => app.controls_kind(),
                        };
                        app.open_notch_with_focus(index, kind, KeyboardInteractivity::None);
                    }
                    if let Some(f) = &mut app.floating
                        && let Some(morph) = &mut f.morph
                    {
                        morph.auto_hide = true;
                        morph.pointer_inside = true;
                        morph
                            .keyboard
                            .set_keyboard_interactivity(KeyboardInteractivity::None);
                        morph.backdrop.layer.commit();
                        morph.keyboard.commit();
                    }
                }
                TimeoutAction::Drop
            },
        );
    }
    fn click(&mut self, index: usize, module: Module, button: u32) {
        self.focused_output = Some(self.bars[index].name.clone());
        self.hover_generation = self.hover_generation.wrapping_add(1);
        self.hover_pending = None;
        self.overview_hover = None;
        self.hover_suppressed_until = Instant::now() + Duration::from_millis(350);
        let context = self.theme.motion()
            && button == 0x111
            && matches!(module, Module::Shell | Module::Tray(_) | Module::Volume);
        if self
            .floating
            .as_ref()
            .is_some_and(|f| !f.panel.closing && !matches!(f.panel.kind, Kind::Tooltip(_)))
            && !context
        {
            self.close();
            return;
        }
        if self.floating.as_ref().is_some_and(|f| f.morph.is_none()) {
            self.floating = None;
        }
        match module {
            Module::Shell => self.command(if button == 0x111 {
                "styles"
            } else {
                "controls"
            }),
            Module::Workspace(n) => {
                self.close();
                let bar = &self.bars[index];
                if bar.view.available {
                    modules::workspaces::switch_workspace(
                        &bar.name,
                        &n.to_string(),
                        bar.view.space.as_deref(),
                    );
                }
            }
            Module::Cpu => spawn(&["desktopctl", "launch", "focus", "missioncenter"]),
            Module::Volume if button == 0x111 => mute(false),
            Module::Microphone => mute(true),
            Module::Volume if self.theme.motion() => self.open(index, module, Kind::Volume, true),
            Module::Volume => spawn(&["desktopctl", "launch", "audio"]),
            Module::Clock if self.theme.motion() => self.open(index, module, Kind::Calendar, true),
            Module::Clock => {
                let b = &mut self.bars[index];
                b.view.long_date = !b.view.long_date;
                let surface = b.surface().clone();
                b.canvas.request(&surface, &self.qh);
            }
            Module::Battery if self.theme.motion() => self.open(index, module, Kind::Battery, true),
            Module::Network if self.theme.motion() => self.open(index, module, Kind::Network, true),
            Module::Network => spawn(&["desktopctl", "launch", "wifi"]),
            Module::Tray(n) => {
                if let Some(item) = self.data.tray.get(n).cloned() {
                    let origin = self
                        .outputs
                        .info(&self.bars[index].output)
                        .and_then(|i| i.logical_position)
                        .unwrap_or((0, 0));
                    let inset = if self.theme.motion() {
                        self.outputs
                            .info(&self.bars[index].output)
                            .and_then(|i| i.logical_size)
                            .map(|size| (size.0 - self.bars[index].canvas.width as i32) / 2)
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    let rect = self.bars[index]
                        .view
                        .layout
                        .cells
                        .iter()
                        .find(|c| c.label.module == module)
                        .map(|c| c.rect)
                        .unwrap_or_default();
                    let x = origin.0 + inset + ((rect.x + rect.w / 2.0) * self.theme.scale) as i32;
                    let y = origin.1 + ((rect.y + rect.h) * self.theme.scale) as i32;
                    if button == 0x111 || item.is_menu {
                        if item.menu.is_some() {
                            self.open(
                                index,
                                module,
                                Kind::Menu {
                                    address: item.address.clone(),
                                    parent: 0,
                                    parents: vec![],
                                    entries: vec![],
                                },
                                true,
                            );
                            self.tray.send(TrayAction::Menu(item.address, 0));
                        } else {
                            self.tray.send(TrayAction::Context(item.address, x, y));
                        }
                    } else if button == 0x112 {
                        self.tray.send(TrayAction::Secondary(item.address, x, y));
                    } else {
                        self.tray.send(TrayAction::Activate(item.address, x, y));
                    }
                }
            }
            _ => {}
        }
    }
    fn wheel(&mut self, index: usize, module: Module, notches: i32) {
        if notches == 0 {
            return;
        }
        match module {
            Module::Workspace(_) => {
                let b = &self.bars[index];
                if b.view.available {
                    modules::workspaces::switch_workspace(
                        &b.name,
                        &format!("e{notches:+}"),
                        b.view.space.as_deref(),
                    );
                }
            }
            Module::Volume | Module::Microphone => {
                scroll_audio(module, notches);
            }
            Module::Tray(n) => {
                if let Some(item) = self.data.tray.get(n) {
                    self.tray
                        .send(TrayAction::Scroll(item.address.clone(), -notches * 120));
                }
            }
            _ => {}
        }
    }
    fn popup_click(&mut self, x: f32, y: f32) {
        let (x, y) = self.content_position(x, y);
        let action = self.floating.as_ref().and_then(|f| {
            if let Kind::Overview(model) = &f.panel.kind {
                model.hit(x, y, &self.data)
            } else if let Kind::Shell(model) = &f.panel.kind {
                model
                    .media_rect()
                    .and_then(|rect| now_playing::hit(&self.data.media, rect, x, y))
                    .map(overview::Action::Media)
            } else {
                None
            }
        });
        if let Some(action) = action {
            match action {
                overview::Action::Panel(panel) => {
                    self.remember_parent();
                    self.command(panel);
                }
                overview::Action::Media(method) => {
                    if self.data.media.available() {
                        self.shell.native_action(
                            vec!["media".into(), self.data.media.bus.clone(), method.into()],
                            0,
                        );
                    }
                }
                overview::Action::Workspace(n) => {
                    if let Some(i) = self.focused_bar() {
                        let bar = &self.bars[i];
                        modules::workspaces::switch_workspace(
                            &bar.name,
                            &n.to_string(),
                            bar.view.space.as_deref(),
                        );
                    }
                }
                overview::Action::Tray(n) => {
                    if let Some(item) = self.data.tray.get(n) {
                        let address = item.address.clone();
                        if item.menu.is_some() {
                            self.remember_parent();
                            if let Some(index) = self.focused_bar() {
                                self.open_notch(
                                    index,
                                    Kind::Menu {
                                        address: address.clone(),
                                        parent: 0,
                                        parents: vec![],
                                        entries: vec![],
                                    },
                                );
                            }
                            self.tray.send(TrayAction::Menu(address, 0));
                        } else {
                            self.tray.send(TrayAction::Activate(address, 0, 0));
                        }
                    }
                }
            }
            return;
        }
        if let Some(f) = &mut self.floating
            && let Kind::Shell(model) = &mut f.panel.kind
        {
            if model.page == shell::Page::Audio {
                if (53.0..79.0).contains(&y) && (20.0..574.0).contains(&x) {
                    model.section = (((x - 24.0).max(0.0) / 110.0) as usize).min(4);
                    model.filter();
                    f.canvas.request(f.window.wl_surface(), &self.qh);
                    return;
                }
                if let Some(row) = model.hit(x, y, model.size().0 as f32)
                    && (422.0..534.0).contains(&x)
                {
                    let i = model.filtered[row];
                    if model.entries[i].level.is_some() {
                        let value = ((x - 422.0) / 112.0 * 100.0).round().clamp(0.0, 100.0) as u32;
                        model.entries[i].level = Some(value);
                        let mut args = model.entries[i].command[1..].to_vec();
                        args[1] = format!("{}-volume", model.entries[i].category);
                        args[3] = value.to_string();
                        self.audio_drag = Some((
                            model.generation,
                            model.entries[i].category.clone(),
                            args[2].clone(),
                        ));
                        self.shell.native_action(args, model.generation);
                        f.canvas.request(f.window.wl_surface(), &self.qh);
                        return;
                    }
                }
            }
            if let Some(page) = shell::Model::tab(x, y, model.size().0 as f32) {
                self.show_page(page, false);
                return;
            }
            if model.page == shell::Page::Controls {
                model.search_focused = y < 48.0;
                if model.search_focused {
                    f.canvas.request(f.window.wl_surface(), &self.qh);
                    return;
                }
            }
            if model.page == shell::Page::Clipboard {
                model.clipboard.searching = y < 48.0;
                if (50.0..76.0).contains(&y) && (19.0..277.0).contains(&x) {
                    model.clipboard.category = [
                        clipboard::Category::All,
                        clipboard::Category::Images,
                        clipboard::Category::Text,
                    ][((x - 19.0) / 86.0) as usize];
                    model.filter();
                } else if let Some(index) = model.hit(x, y, model.size().0 as f32) {
                    model.selected = index;
                    model.cursor.snap(index as f32);
                    let id = model.entries[model.filtered[index]].id.clone();
                    model.clipboard.toggle(&id);
                } else if y > 410.0 && x >= 480.0 {
                    self.activate();
                    return;
                }
                self.shell.images(model);
                f.canvas.request(f.window.wl_surface(), &self.qh);
                return;
            }
            if let Some(index) = model.hit(x, y, model.size().0 as f32) {
                model.selected = index;
                model.cursor.target = index as f32;
                self.activate();
            }
            return;
        }
        let Some(f) = &mut self.floating else {
            return;
        };
        if let Some((module, percent)) = f.panel.slider(x, y) {
            self.drag = Some(module);
            set_volume(module, percent);
            return;
        }
        let row = f.panel.row(y);
        match &mut f.panel.kind {
            Kind::Volume if (149.0..172.0).contains(&y) => {
                self.remember_parent();
                self.show_page(shell::Page::Audio, false);
                return;
            }
            Kind::Network if (132.0..164.0).contains(&y) => {
                self.remember_parent();
                self.show_page(shell::Page::Wifi, false);
                return;
            }
            Kind::Calendar if y < 42.0 => {
                f.panel.month += if x < 160.0 { -1 } else { 1 };
            }
            Kind::Menu {
                address,
                parent,
                parents,
                entries,
            } => {
                if y < 42.0 {
                    if let Some(previous) = parents.pop() {
                        *parent = previous;
                        self.tray.send(TrayAction::Menu(address.clone(), *parent));
                    } else {
                        self.close();
                        return;
                    }
                } else if let Some(i) = row
                    && let Some(entry) = entries.get(i).filter(|e| e.enabled && !e.separator)
                {
                    if entry.submenu {
                        parents.push(*parent);
                        *parent = entry.id;
                        self.tray.send(TrayAction::Menu(address.clone(), *parent));
                    } else {
                        self.tray.send(TrayAction::Click(
                            address.clone(),
                            entry.id,
                            self.last_time,
                        ));
                        self.close();
                        return;
                    }
                }
            }
            _ => {}
        }
        if let Some(f) = &mut self.floating {
            f.canvas.request(f.window.wl_surface(), &self.qh);
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(b) = self.bars.iter_mut().find(|b| b.surface() == surface)
            && b.canvas.viewport.is_none()
        {
            b.canvas.resize(
                surface,
                b.canvas.width,
                b.canvas.height,
                factor.max(1) as f32,
                &self.shm,
            );
            b.view.force = true;
            b.canvas.request(surface, &self.qh);
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(i) = self.bars.iter().position(|b| b.surface() == surface) {
            self.bars[i].canvas.frame_done();
            self.paint_bar(i);
        } else if let Some(f) = &mut self.floating
            && f.window.wl_surface() == surface
        {
            f.canvas.frame_done();
            self.paint_popup();
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.bars.retain(|b| &b.layer != layer);
        if self.floating.as_ref().is_some_and(|f| {
            f.morph.as_ref().is_some_and(|m| &m.backdrop.layer == layer)
                || matches!(&f.window, FloatingWindow::Overlay(menu) if menu == layer)
        }) && let Some(f) = self.floating.take()
        {
            // A notch panel shares the bar surface; repaint the compact bar
            // so the expanded image does not stay on screen.
            if matches!(f.window, FloatingWindow::Root(_))
                && let Some(bar) = self.bars.iter_mut().find(|b| b.name == f.name)
            {
                bar.view.force = true;
                let surface = bar.surface().clone();
                bar.canvas.request(&surface, &self.qh);
            }
        }
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        config: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(f) = &mut self.floating
            && matches!(&f.window, FloatingWindow::Overlay(menu) if menu == layer)
        {
            let (width, height) = config.new_size;
            f.canvas.resize(
                layer.wl_surface(),
                width.max(1),
                height.max(1),
                f.canvas.scale,
                &self.shm,
            );
            f.canvas.configured = true;
            if !f.canvas.pending() {
                self.paint_popup();
            }
            return;
        }
        if let Some(f) = &mut self.floating
            && let Some(morph) = &mut f.morph
            && &morph.backdrop.layer == layer
        {
            let canvas = &mut morph.backdrop.canvas;
            canvas.configured = true;
            if let Some(viewport) = &canvas.viewport {
                viewport.set_destination(
                    config.new_size.0.max(1) as i32,
                    config.new_size.1.max(1) as i32,
                );
            }
            canvas.present(
                layer.wl_surface(),
                &self.qh,
                &[Rect::new(0.0, 0.0, 1.0, 1.0)],
                false,
            );
            return;
        }
        if let Some(i) = self.bars.iter().position(|b| &b.layer == layer) {
            let b = &mut self.bars[i];
            let w = if config.new_size.0 > 0 {
                config.new_size.0
            } else {
                b.canvas.width
            };
            let h = if config.new_size.1 > 0 {
                config.new_size.1
            } else {
                b.canvas.height
            };
            if b.canvas
                .resize(layer.wl_surface(), w, h, b.canvas.scale, &self.shm)
            {
                b.view.force = true;
            }
            b.canvas.configured = true;
            if !b.canvas.pending() {
                self.paint_bar(i);
            }
        }
    }
}
impl PopupHandler for App {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        _: PopupConfigure,
    ) {
        if let Some(f) = &mut self.floating
            && f.window.popup() == Some(popup)
        {
            f.canvas.configured = true;
            if !f.canvas.pending() {
                self.paint_popup();
            }
        }
    }
    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        if self
            .floating
            .as_ref()
            .is_some_and(|f| f.window.popup() == Some(popup))
        {
            // Dismissed popups are unmapped, so closing animations cannot
            // receive frame callbacks. Destroy the popup and release its grab.
            self.tray.send(TrayAction::CloseMenu);
            self.floating = None;
            self.drag = None;
            self.audio_drag = None;
            trim_heap();
        }
    }
}
impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.bars.retain(|b| b.output != output);
        if self
            .floating
            .as_ref()
            .is_some_and(|f| !self.bars.iter().any(|b| b.name == f.name))
        {
            self.floating = None;
        }
    }
}
impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if !self.pointers.contains_key(&seat)
                    && let Ok(p) = self.seats.get_pointer(qh, &seat)
                {
                    self.pointers.insert(seat, p);
                }
            }
            Capability::Keyboard => {
                if !self.keyboards.contains_key(&seat)
                    && let Ok(keyboard) = self.seats.get_keyboard_with_repeat(
                        qh,
                        &seat,
                        None,
                        self.handle.clone(),
                        Box::new(|app, _, event| app.key(event)),
                    )
                {
                    self.keyboards.insert(seat, keyboard);
                }
            }
            _ => {}
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                if let Some(p) = self.pointers.remove(&seat) {
                    p.release();
                }
            }
            Capability::Keyboard => {
                if let Some(k) = self.keyboards.remove(&seat) {
                    k.release();
                }
            }
            _ => {}
        }
    }
    fn remove_seat(&mut self, c: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.remove_capability(c, qh, seat.clone(), Capability::Pointer);
        self.remove_capability(c, qh, seat, Capability::Keyboard);
    }
}
impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        self.last_seat = self
            .pointers
            .iter()
            .find(|(_, p)| *p == pointer)
            .map(|(s, _)| s.clone());
        for event in events {
            let (x, y) = (
                event.position.0 as f32 / self.theme.scale,
                event.position.1 as f32 / self.theme.scale,
            );
            if self.floating.as_ref().is_some_and(|f| {
                f.morph
                    .as_ref()
                    .is_some_and(|m| m.backdrop.layer.wl_surface() == &event.surface)
            }) {
                if matches!(event.kind, PointerEventKind::Press { .. }) {
                    self.close();
                } else if matches!(
                    event.kind,
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. }
                ) {
                    // The pointer has left the remembered click area too.
                    if let Some(f) = &mut self.floating
                        && let Some(morph) = &mut f.morph
                        && morph.sticky.take().is_some()
                    {
                        f.canvas.request(f.window.wl_surface(), &self.qh);
                    }
                    self.notch_pointer(false);
                }
                continue;
            }
            if self
                .floating
                .as_ref()
                .is_some_and(|f| f.window.wl_surface() == &event.surface)
            {
                if self
                    .floating
                    .as_ref()
                    .is_some_and(|f| f.panel.closing && matches!(f.window, FloatingWindow::Root(_)))
                {
                    if let PointerEventKind::Press {
                        serial,
                        time,
                        button,
                    } = event.kind
                        && let Some(i) =
                            self.bars.iter().position(|b| b.surface() == &event.surface)
                        && let Some(module) = self.bars[i].view.layout.hit(x, y)
                    {
                        self.last_serial = serial;
                        self.last_time = time;
                        self.click(i, module, button);
                        continue;
                    }
                    if let Some(i) = self.bars.iter().position(|b| b.surface() == &event.surface) {
                        match event.kind {
                            PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                                self.bars[i].view.hover = self.bars[i].view.layout.hit(x, y);
                                let inside = self.bars[i]
                                    .view
                                    .layout
                                    .groups
                                    .iter()
                                    .any(|r| r.contains(x, y));
                                self.notch_pointer(inside);
                                self.motion_hover(i, inside);
                            }
                            PointerEventKind::Leave { .. } => {
                                self.bars[i].view.hover = None;
                                self.notch_pointer(false);
                                self.motion_hover(i, false);
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                match event.kind {
                    PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                        let inside = self.floating_contains(x, y)
                            || self.in_sticky(x, y)
                            || self.drag.is_some()
                            || self.audio_drag.is_some();
                        self.notch_pointer(inside);
                        if let Some(b) =
                            self.bars.iter_mut().find(|b| b.surface() == &event.surface)
                        {
                            b.view.hover = b.view.layout.hit(x, y);
                        }
                    }
                    PointerEventKind::Leave { .. } => {
                        self.overview_hover = None;
                        self.notch_pointer(false);
                        if let Some(b) =
                            self.bars.iter_mut().find(|b| b.surface() == &event.surface)
                        {
                            b.view.hover = None;
                        }
                    }
                    _ => {}
                }
                match event.kind {
                    PointerEventKind::Press {
                        serial,
                        time,
                        button,
                    } => {
                        self.last_serial = serial;
                        self.last_time = time;
                        if !self.floating_contains(x, y) {
                            // Content moved away from a resting cursor; this
                            // click was not aimed outside the panel.
                            if !self.in_sticky(x, y) {
                                self.close();
                            }
                            continue;
                        }
                        self.remember_click_area();
                        if self.theme.motion()
                            && let Some(i) =
                                self.bars.iter().position(|b| b.surface() == &event.surface)
                            && let Some(module) = self.bars[i].view.layout.hit(x, y)
                            && button == 0x111
                            && matches!(module, Module::Shell | Module::Tray(_) | Module::Volume)
                        {
                            self.click(i, module, button);
                            continue;
                        }
                        if let Some(morph) = self.floating.as_ref().and_then(|f| f.morph.as_ref()) {
                            morph
                                .keyboard
                                .set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                            morph.backdrop.layer.commit();
                            morph.keyboard.commit();
                        }
                        self.popup_click(x, y);
                    }
                    PointerEventKind::Release { .. } => {
                        self.drag = None;
                        self.audio_drag = None;
                        self.notch_pointer(self.floating_contains(x, y) || self.in_sticky(x, y));
                    }
                    PointerEventKind::Motion { .. } => {
                        let (x, y) = self.content_position(x, y);
                        self.overview_pointer(x, y);
                        if let Some((id, category, target)) = &self.audio_drag {
                            let value =
                                ((x - 422.0) / 112.0 * 100.0).round().clamp(0.0, 100.0) as u32;
                            self.shell.native_action(
                                vec![
                                    "audio".into(),
                                    format!("{category}-volume"),
                                    target.clone(),
                                    value.to_string(),
                                ],
                                *id,
                            );
                            if let Some(f) = &mut self.floating
                                && let Kind::Shell(model) = &mut f.panel.kind
                                && model.generation == *id
                            {
                                for item in &mut model.entries {
                                    if item.command.get(3) == Some(target)
                                        && item.category == *category
                                    {
                                        item.level = Some(value);
                                    }
                                }
                                f.canvas.request(f.window.wl_surface(), &self.qh);
                            }
                            continue;
                        }
                        if let Some(module) = self.drag {
                            set_volume(
                                module,
                                ((x - 24.0) / 272.0 * 100.0).round().clamp(0.0, 100.0) as u32,
                            );
                        }
                        if self.panel_scroll.active() {
                            continue;
                        }
                        if let Some(f) = &mut self.floating {
                            let mut changed = false;
                            if let Kind::Shell(model) = &mut f.panel.kind {
                                if !model.page.gallery()
                                    && let Some(index) = model.hit(x, y, model.size().0 as f32)
                                    && model.selected != index
                                {
                                    model.selected = index;
                                    model.cursor.snap(index as f32);
                                    self.shell.images(model);
                                    changed = true;
                                }
                            } else {
                                let row = f.panel.row(y);
                                // Only menus draw the hovered row.
                                changed = f.panel.hover != row
                                    && matches!(f.panel.kind, Kind::Menu { .. });
                                f.panel.hover = row;
                            }
                            if changed {
                                f.canvas.request(f.window.wl_surface(), &self.qh);
                            }
                        }
                    }
                    PointerEventKind::Axis {
                        horizontal,
                        vertical,
                        ..
                    } => {
                        let gallery = self.floating.as_ref().is_some_and(
                            |f| matches!(&f.panel.kind, Kind::Shell(model) if model.page.gallery()),
                        );
                        let magnitude =
                            |axis: smithay_client_toolkit::seat::pointer::AxisScroll| {
                                axis.absolute
                                    .abs()
                                    .max(axis.value120.abs() as f64)
                                    .max(axis.discrete.abs() as f64)
                            };
                        let axis = if gallery && magnitude(horizontal) > magnitude(vertical) {
                            horizontal
                        } else {
                            vertical
                        };
                        let steps = self.panel_scroll.steps(axis);
                        if steps == 0 {
                            continue;
                        }
                        if let Some(f) = &self.floating
                            && matches!(f.panel.kind, Kind::Volume)
                        {
                            let (content_x, content_y) = self.content_position(x, y);
                            let header = self
                                .bars
                                .iter()
                                .find(|bar| bar.name == f.name)
                                .and_then(|bar| bar.view.layout.hit(x, y))
                                .filter(|module| {
                                    matches!(module, Module::Volume | Module::Microphone)
                                });
                            let module = f
                                .panel
                                .slider(content_x, content_y)
                                .map(|(module, _)| module)
                                .or(header)
                                .unwrap_or(Module::Volume);
                            scroll_audio(module, steps);
                            continue;
                        }
                        if let Some(f) = &mut self.floating {
                            match &mut f.panel.kind {
                                Kind::Shell(model) => {
                                    model.navigate(
                                        steps
                                            * if matches!(
                                                model.page,
                                                shell::Page::Controls | shell::Page::Clipboard
                                            ) {
                                                4
                                            } else {
                                                1
                                            },
                                    );
                                    self.shell.images(model);
                                }
                                Kind::Calendar => f.panel.month += steps,
                                Kind::Menu { entries, .. } => {
                                    let max = entries.len().saturating_sub(16);
                                    f.panel.scroll = (f.panel.scroll as i32 + steps)
                                        .clamp(0, max as i32)
                                        as usize;
                                }
                                _ => {}
                            }
                            f.canvas.request(f.window.wl_surface(), &self.qh);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let Some(i) = self.bars.iter().position(|b| b.surface() == &event.surface) else {
                continue;
            };
            let module = self.bars[i].view.layout.hit(x, y);
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.last_serial = serial;
                    self.hover(i, module);
                    let inside = self.bars[i]
                        .view
                        .layout
                        .groups
                        .iter()
                        .any(|r| r.contains(x, y));
                    self.motion_hover(i, inside);
                }
                PointerEventKind::Motion { .. } => {
                    self.hover(i, module);
                    let inside = self.bars[i]
                        .view
                        .layout
                        .groups
                        .iter()
                        .any(|r| r.contains(x, y));
                    self.motion_hover(i, inside);
                }
                PointerEventKind::Leave { .. } => {
                    self.hover(i, None);
                    self.motion_hover(i, false);
                }
                PointerEventKind::Press {
                    serial,
                    button,
                    time,
                } => {
                    self.last_serial = serial;
                    self.last_time = time;
                    if let Some(m) = module {
                        self.click(i, m, button);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let notches = if vertical.discrete != 0 {
                        vertical.discrete
                    } else {
                        self.scroll += vertical.absolute;
                        let n = (self.scroll / 10.0).trunc() as i32;
                        self.scroll -= n as f64 * 10.0;
                        n
                    };
                    if let Some(m) = module {
                        self.wheel(i, m, notches);
                    }
                }
                _ => {}
            }
        }
    }
}
impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.modifiers = Modifiers::default();
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.key(event);
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = modifiers;
    }
}
impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, wl_surface::WlSurface> for App {
    fn event(
        app: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let scale = (scale as f32 / 120.0).clamp(0.5, 4.0);
            if let Some(b) = app.bars.iter_mut().find(|b| b.surface() == surface) {
                if b.canvas
                    .resize(surface, b.canvas.width, b.canvas.height, scale, &app.shm)
                {
                    b.view.force = true;
                }
                b.canvas.request(surface, &app.qh);
            }
            if let Some(f) = &mut app.floating
                && f.window.wl_surface() == surface
            {
                f.canvas
                    .resize(surface, f.canvas.width, f.canvas.height, scale, &app.shm);
                f.canvas.request(surface, &app.qh);
            }
        }
    }
}
impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}
impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}
smithay_client_toolkit::delegate_dispatch2!(App);
impl Dispatch<wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,smithay_client_toolkit::globals::GlobalData> for App {
    fn event(_: &mut Self,_: &wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,_:wayland_protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::Event,_:&smithay_client_toolkit::globals::GlobalData,_:&Connection,_:&QueueHandle<Self>){}
}
delegate_registry!(App);
smithay_client_toolkit::reexports::client::delegate_noop!(App:ignore wp_viewporter::WpViewporter);
smithay_client_toolkit::reexports::client::delegate_noop!(App:ignore wp_viewport::WpViewport);
smithay_client_toolkit::reexports::client::delegate_noop!(App:ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);

fn spawn(args: &[&str]) {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        if let Some(command) = args.first() {
            let mut child_command = Command::new(command);
            unsafe {
                child_command.pre_exec(|| {
                    reset_child_signals();
                    Ok(())
                });
            }
            match child_command
                .args(&args[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(e) => eprintln!("desktop-bar: cannot launch {command}: {e}"),
            }
        }
    });
}
fn trim_heap() {
    #[cfg(target_env = "gnu")]
    unsafe {
        libc::malloc_trim(0);
    }
}
pub fn reset_child_signals() {
    unsafe {
        let mut mask: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        for signal in [libc::SIGUSR2, libc::SIGTERM, libc::SIGINT] {
            libc::sigaddset(&mut mask, signal);
        }
        libc::sigprocmask(libc::SIG_UNBLOCK, &mask, std::ptr::null_mut());
    }
}
fn mute(microphone: bool) {
    audio_commands::send(audio_commands::Action::Mute { microphone });
}
fn scroll_audio(module: Module, notches: i32) {
    let step = if module == Module::Volume { 10 } else { 5 };
    audio_commands::send(audio_commands::Action::Adjust {
        microphone: module == Module::Microphone,
        percent: notches.saturating_mul(step).saturating_neg(),
    });
}

fn set_volume(module: Module, percent: u32) {
    audio_commands::send(audio_commands::Action::Set {
        microphone: module == Module::Microphone,
        percent,
    });
}

fn dump(args: &[String]) -> Result<(), String> {
    let path = args
        .get(1)
        .ok_or("--dump requires FILE SCALE clean|motion")?;
    let scale = args
        .get(2)
        .ok_or("--dump requires a scale")?
        .parse::<f32>()
        .map_err(|_| "Invalid scale")?;
    if !scale.is_finite() || !(0.5..=3.0).contains(&scale) {
        return Err("Scale must be between 0.5 and 3".into());
    }
    let style = match args.get(3).map(String::as_str) {
        Some("clean" | "native-clean") => "native-clean",
        Some("motion" | "native-motion") => "native-motion",
        _ => return Err("Style must be clean or motion".into()),
    };
    let t = Theme {
        style: style.into(),
        light: args.iter().any(|s| s == "--light"),
        ..Theme::default()
    };
    let mut text = text::Text::new(&t.font)?;
    let data = sample_data();
    let p = if let Some(i) = args.iter().position(|s| s == "--panel") {
        let kind = match args.get(i + 1).map(String::as_str) {
            Some("volume") => Kind::Volume,
            Some("calendar") => Kind::Calendar,
            Some("battery") => Kind::Battery,
            _ => return Err("Unknown panel".into()),
        };
        let mut panel = Panel::new(kind);
        panel.growth.snap(1.0);
        let (w, h) = panel.size();
        let mut p = tiny_skia::Pixmap::new((w as f32 * scale) as u32, (h as f32 * scale) as u32)
            .ok_or("Invalid dimensions")?;
        panel.draw(&mut p, &mut text, &data, &t, scale);
        p
    } else {
        let mut p = tiny_skia::Pixmap::new((1920.0 * scale) as u32, (56.0 * scale) as u32)
            .ok_or("Invalid dimensions")?;
        let mut view = render::View::new(t.motion());
        view.snapshot(Some(modules::workspaces::Snapshot {
            active: 3,
            space: None,
            occupied: [1, 2, 3, 7].into_iter().collect(),
            remote: [(1, 1), (2, 2)].into_iter().collect(),
            urgent: Default::default(),
        }));
        view.render(&mut p, &mut text, &data, &t, 1920.0, false, scale);
        p
    };
    p.save_png(path).map_err(|e| e.to_string())
}
fn sample_data() -> Data {
    Data {
        cpu: vec![23.0, 10.0, 40.0, 30.0, 12.0],
        memory: Some(modules::mem::Memory {
            used: 8.4,
            total: 32.0,
            swap_used: 0.0,
            swap_total: 8.0,
        }),
        disk: Some(modules::disk::Disk {
            free: 214.0,
            total: 1000.0,
            used_percent: 78.6,
        }),
        battery: Some(modules::battery::Battery {
            percent: 76.0,
            charging: false,
            plugged: false,
            watts: 8.6,
            hours: 4.2,
        }),
        audio: modules::audio::Audio {
            volume: 65,
            available: true,
            ..Default::default()
        },
        microphone: modules::audio::Audio {
            volume: 100,
            available: true,
            ..Default::default()
        },
        network: modules::network::Network {
            available: true,
            powered: true,
            state: "connected".into(),
            name: "Wi-Fi".into(),
            interface: "wlan0".into(),
            rssi: Some(-58),
        },
        weather: modules::weather::Weather {
            observation: Some(desktop_weather::Observation {
                id: 801,
                description: "Few clouds".into(),
                temperature: 12.0,
                wind_kmh: 14.4,
                sunrise: 0,
                sunset: u64::MAX,
            }),
            ..Default::default()
        },
        media: modules::media::Media::default(),
        tray: vec![],
    }
}
