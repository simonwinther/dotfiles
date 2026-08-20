//! Animated Hyprland workspace pill, drawn straight onto a layer-shell surface.
//!
//! One surface per output, each tracking its own monitor's workspaces, all
//! driven by a single Hyprland connection and a single event loop.

mod hypr;
mod render;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::reexports::calloop::{
    channel, timer::{TimeoutAction, Timer}, EventLoop,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use tiny_skia::Pixmap;

use render::{
    center_for, Renderer, FIRST_CENTER, PANEL_HEIGHT, PANEL_TOP, PANEL_WIDTH, SLOT_WIDTH,
    WORKSPACE_COUNT,
};

/// The event stream drives every normal update and a dropped connection resyncs
/// on reconnect, so this poll is only insurance against a stall nothing else
/// catches.
const FALLBACK_REFRESH: Duration = Duration::from_secs(60);
/// How long to keep retrying Hyprland's IPC at startup before giving up: an
/// autostart can win the race against the compositor being ready.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const STARTUP_RETRY: Duration = Duration::from_millis(250);
/// Accumulated smooth-scroll units that add up to one notch.
const SCROLL_NOTCH: f64 = 10.0;

fn main() {
    // Render one frame to a file and exit, for comparing against the original.
    let args: Vec<String> = std::env::args().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--dump") {
        let path = args.get(index + 1).map(String::as_str).unwrap_or("frame.png");
        let scale = args.get(index + 2).and_then(|v| v.parse().ok()).unwrap_or(1.0);
        dump_frame(path, scale);
        return;
    }

    // Every output gets a pill unless this pins us to one.
    let only = std::env::var("WAYBAR_OUTPUT_NAME").ok().filter(|name| !name.is_empty());

    if !wait_for_hyprland() {
        return;
    }

    let connection = match Connection::connect_to_env() {
        Ok(connection) => connection,
        Err(_) => return,
    };
    let (globals, event_queue) = match registry_queue_init(&connection) {
        Ok(parts) => parts,
        Err(_) => return,
    };
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor unavailable");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell unavailable");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm unavailable");
    // Room for a few panels; the pool grows itself if the outputs need more.
    let Ok(pool) = SlotPool::new((PANEL_WIDTH * PANEL_HEIGHT * 4.0) as usize * 4, &shm) else {
        return;
    };

    let mut event_loop = EventLoop::<App>::try_new().expect("event loop");
    let handle = event_loop.handle();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        compositor,
        layer_shell,
        renderers: HashMap::new(),
        monitors: Vec::new(),
        pointer: None,
        qh: qh.clone(),
        only,
        focused: None,
    };

    let mut queue = event_queue;
    // One roundtrip so the outputs are known before we place surfaces on them.
    if queue.roundtrip(&mut app).is_err() {
        return;
    }

    for output in app.output_state.outputs().collect::<Vec<_>>() {
        app.add_monitor(output);
    }
    app.refresh();

    let (sender, receiver) = channel::channel();
    hypr::listen(move |event| {
        let _ = sender.send(event);
    });
    handle
        .insert_source(receiver, |event, _, app| {
            if let channel::Event::Msg(event) = event {
                app.handle_hypr_event(event);
            }
        })
        .expect("channel source");

    handle
        .insert_source(Timer::from_duration(FALLBACK_REFRESH), |_, _, app| {
            app.refresh();
            TimeoutAction::ToDuration(FALLBACK_REFRESH)
        })
        .expect("timer source");

    WaylandSource::new(connection.clone(), queue)
        .insert(handle.clone())
        .expect("wayland source");

    loop {
        if event_loop.dispatch(None, &mut app).is_err() {
            break;
        }
    }
}

fn dump_frame(path: &str, scale: f32) {
    let Some(renderer) = Renderer::new(scale) else {
        eprintln!("no font");
        return;
    };
    let Some(mut pixmap) = Pixmap::new(renderer.width(), renderer.height()) else {
        return;
    };
    let occupied: HashSet<i32> = [2, 8].into_iter().collect();
    let occupied_elsewhere: HashSet<i32> = [1, 3, 6, 9, 10].into_iter().collect();
    renderer.draw(
        &mut pixmap,
        center_for(8),
        &occupied,
        &occupied_elsewhere,
        None,
    );
    if let Err(error) = pixmap.save_png(path) {
        eprintln!("save failed: {error}");
    }
}

/// Keep asking until Hyprland's IPC answers; giving up on the first miss would
/// leave the bar without its pill for the whole session.
fn wait_for_hyprland() -> bool {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if hypr::snapshot_all(WORKSPACE_COUNT).is_some_and(|monitors| !monitors.is_empty()) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(STARTUP_RETRY);
    }
}

fn workspace_at(x: f64) -> i32 {
    let workspace = ((x as f32 - FIRST_CENTER) / SLOT_WIDTH).round() as i32 + 1;
    workspace.clamp(1, WORKSPACE_COUNT)
}

/// One output's pill: its own surface, its own animation, its own workspaces.
struct Monitor {
    output: wl_output::WlOutput,
    name: String,
    layer: LayerSurface,
    scale: i32,
    renderer: Rc<Renderer>,
    pixmap: Pixmap,
    configured: bool,
    frame_pending: bool,
    /// Cleared until the first state read lands, so the pill appears already at
    /// the right workspace instead of sliding there from 1 at login.
    initialized: bool,

    active: i32,
    occupied: HashSet<i32>,
    occupied_elsewhere: HashSet<i32>,
    deferred_occupied: Option<i32>,
    position: f32,
    start_position: f32,
    target_position: f32,
    animation_started: Instant,
    animation_duration: f32,
    animating: bool,
    scroll_accumulator: f64,
}

impl Monitor {
    /// Take one monitor's slice of a state read. Returns whether anything moved.
    fn apply(&mut self, snapshot: &hypr::Snapshot, occupied_elsewhere: HashSet<i32>) -> bool {
        let mut changed =
            snapshot.occupied != self.occupied || occupied_elsewhere != self.occupied_elsewhere;
        self.occupied = snapshot.occupied.clone();
        self.occupied_elsewhere = occupied_elsewhere;
        let workspace = snapshot.active.clamp(1, WORKSPACE_COUNT);
        if !self.initialized {
            self.snap_to(workspace);
            self.initialized = true;
            changed = true;
        } else if workspace != self.active {
            self.start_slide(workspace);
            changed = true;
        }
        changed
    }

    fn snap_to(&mut self, workspace: i32) {
        self.active = workspace;
        self.deferred_occupied = None;
        self.position = center_for(workspace);
        self.start_position = self.position;
        self.target_position = self.position;
        self.animating = false;
    }

    fn start_slide(&mut self, workspace: i32) {
        self.deferred_occupied =
            if self.occupied.contains(&workspace) { None } else { Some(workspace) };
        self.active = workspace;
        self.start_position = self.position;
        self.target_position = center_for(workspace);
        let distance = (self.target_position - self.start_position).abs() / SLOT_WIDTH;
        self.animation_duration = (0.17 + distance * 0.018).min(0.30);
        self.animation_started = Instant::now();
        self.animating = true;
    }

    /// Ask for one frame callback; the compositor decides when, so this is the
    /// native equivalent of driving the animation off a frame clock. Nothing is
    /// drawn until it fires, so an idle pill costs no wakeups at all.
    fn request_frame(&mut self, qh: &QueueHandle<App>) {
        if self.frame_pending || !self.configured {
            return;
        }
        self.frame_pending = true;
        let surface = self.layer.wl_surface();
        surface.frame(qh, FrameCallbackData(surface.clone()));
        surface.commit();
    }

    fn draw(&mut self, pool: &mut SlotPool, qh: &QueueHandle<App>) {
        if self.animating {
            let elapsed = self.animation_started.elapsed().as_secs_f32();
            let progress = (elapsed / self.animation_duration).min(1.0);
            let eased = 1.0 - (1.0 - progress).powi(3);
            self.position =
                self.start_position + (self.target_position - self.start_position) * eased;
            if progress >= 1.0 {
                self.position = self.target_position;
                self.deferred_occupied = None;
                self.animating = false;
            }
        }

        self.renderer.draw(
            &mut self.pixmap,
            self.position,
            &self.occupied,
            &self.occupied_elsewhere,
            self.deferred_occupied,
        );

        let width = self.renderer.width() as i32;
        let height = self.renderer.height() as i32;
        let Ok((buffer, canvas)) =
            pool.create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        // tiny-skia is premultiplied RGBA; wl_shm ARGB8888 is premultiplied
        // BGRA in memory on little-endian.
        for (out, src) in canvas.chunks_exact_mut(4).zip(self.pixmap.data().chunks_exact(4)) {
            out[0] = src[2];
            out[1] = src[1];
            out[2] = src[0];
            out[3] = src[3];
        }

        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, width, height);
        if self.animating {
            self.frame_pending = true;
            surface.frame(qh, FrameCallbackData(surface.clone()));
        } else {
            self.frame_pending = false;
        }
        let _ = buffer.attach_to(surface);
        surface.commit();
    }
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    compositor: CompositorState,
    layer_shell: LayerShell,
    /// Keyed by buffer scale. Building one costs a full Nerd Font parse, so
    /// outputs sharing a scale share the glyphs.
    renderers: HashMap<i32, Rc<Renderer>>,
    monitors: Vec<Monitor>,
    pointer: Option<wl_pointer::WlPointer>,
    qh: QueueHandle<App>,

    only: Option<String>,
    /// Which monitor Hyprland last reported as focused; `workspacev2` carries no
    /// monitor of its own, so that is the one its payload applies to.
    focused: Option<String>,
}

impl App {
    fn renderer_for(&mut self, scale: i32) -> Option<Rc<Renderer>> {
        if let Some(renderer) = self.renderers.get(&scale) {
            return Some(renderer.clone());
        }
        let renderer = Rc::new(Renderer::new(scale as f32)?);
        self.renderers.insert(scale, renderer.clone());
        Some(renderer)
    }

    fn add_monitor(&mut self, output: wl_output::WlOutput) {
        if self.monitors.iter().any(|monitor| monitor.output == output) {
            return;
        }
        let Some(info) = self.output_state.info(&output) else {
            return;
        };
        let scale = info.scale_factor.max(1);
        let Some(name) = info.name else {
            return;
        };
        if self.only.as_ref().is_some_and(|only| *only != name) {
            return;
        }
        let Some(renderer) = self.renderer_for(scale) else {
            return;
        };
        let Some(pixmap) = Pixmap::new(renderer.width(), renderer.height()) else {
            return;
        };

        let surface = self.compositor.create_surface(&self.qh);
        surface.set_buffer_scale(scale);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            // Above Waybar, which sits on the bottom layer, so the pill takes
            // the clicks over its own area. Same-layer stacking would be decided
            // by whichever process happened to start first.
            Layer::Top,
            Some(format!("waybar-workspace-slider-{name}")),
            Some(&output),
        );
        layer.set_size(PANEL_WIDTH as u32, PANEL_HEIGHT as u32);
        layer.set_anchor(Anchor::TOP);
        layer.set_margin(PANEL_TOP, 0, 0, 0);
        // Draw over Waybar's reserved strip instead of below it.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();

        let start = center_for(1);
        self.monitors.push(Monitor {
            output,
            name,
            layer,
            scale,
            renderer,
            pixmap,
            configured: false,
            frame_pending: false,
            initialized: false,
            active: 1,
            occupied: HashSet::new(),
            occupied_elsewhere: HashSet::new(),
            deferred_occupied: None,
            position: start,
            start_position: start,
            target_position: start,
            animation_started: Instant::now(),
            animation_duration: 0.20,
            animating: false,
            scroll_accumulator: 0.0,
        });
    }

    fn refresh(&mut self) {
        let Some(snapshots) = hypr::snapshot_all(WORKSPACE_COUNT) else {
            return;
        };
        self.focused = snapshots
            .iter()
            .find(|(_, snapshot)| snapshot.focused)
            .map(|(name, _)| name.clone());

        let qh = self.qh.clone();
        for monitor in &mut self.monitors {
            let Some(snapshot) = snapshots.get(&monitor.name) else {
                continue;
            };
            let occupied_elsewhere = snapshots
                .iter()
                .filter(|(name, _)| name.as_str() != monitor.name)
                .flat_map(|(_, snapshot)| snapshot.occupied.iter().copied())
                .collect();
            if monitor.apply(snapshot, occupied_elsewhere) {
                monitor.request_frame(&qh);
            }
        }
    }

    fn handle_hypr_event(&mut self, event: hypr::Event) {
        match event {
            // The payload already carries the new id, so start moving without
            // waiting for the state read that follows.
            hypr::Event::Switched(workspace) => {
                if !(1..=WORKSPACE_COUNT).contains(&workspace) {
                    return;
                }
                let Some(focused) = self.focused.clone() else {
                    return;
                };
                let qh = self.qh.clone();
                let Some(monitor) = self.monitors.iter_mut().find(|m| m.name == focused) else {
                    return;
                };
                if monitor.initialized && workspace != monitor.active {
                    monitor.start_slide(workspace);
                    monitor.request_frame(&qh);
                }
            }
            hypr::Event::Changed => self.refresh(),
        }
    }

    fn index_for_surface(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.monitors.iter().position(|monitor| monitor.layer.wl_surface() == surface)
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let factor = new_factor.max(1);
        let Some(index) = self.index_for_surface(surface) else {
            return;
        };
        if self.monitors[index].scale == factor {
            return;
        }
        let Some(renderer) = self.renderer_for(factor) else {
            return;
        };
        let Some(pixmap) = Pixmap::new(renderer.width(), renderer.height()) else {
            return;
        };
        let monitor = &mut self.monitors[index];
        monitor.scale = factor;
        monitor.renderer = renderer;
        monitor.pixmap = pixmap;
        monitor.layer.wl_surface().set_buffer_scale(factor);
        monitor.draw(&mut self.pool, qh);
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        let Some(index) = self.index_for_surface(surface) else {
            return;
        };
        let monitor = &mut self.monitors[index];
        monitor.frame_pending = false;
        monitor.draw(&mut self.pool, qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for App {
    /// One output going away should not take the other pills with it.
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        self.monitors.retain(|monitor| monitor.layer.wl_surface() != layer.wl_surface());
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let Some(index) = self.index_for_surface(layer.wl_surface()) else {
            return;
        };
        let monitor = &mut self.monitors[index];
        monitor.configured = true;
        monitor.frame_pending = false;
        monitor.draw(&mut self.pool, qh);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            // The surface says which pill was touched, and so which monitor the
            // dispatch below should act on.
            let Some(index) = self.index_for_surface(&event.surface) else {
                continue;
            };
            let monitor = &mut self.monitors[index];
            match event.kind {
                PointerEventKind::Press { .. } => {
                    let workspace = workspace_at(event.position.0);
                    hypr::dispatch(format!(
                        "dispatch focusmonitor {} ; dispatch workspace {}",
                        monitor.name, workspace
                    ));
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let notches = if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        monitor.scroll_accumulator += vertical.absolute;
                        let whole = (monitor.scroll_accumulator / SCROLL_NOTCH).trunc();
                        monitor.scroll_accumulator -= whole * SCROLL_NOTCH;
                        whole
                    };
                    if notches != 0.0 {
                        let direction = if notches > 0.0 { "e+1" } else { "e-1" };
                        hypr::dispatch(format!(
                            "dispatch focusmonitor {} ; dispatch workspace {}",
                            monitor.name, direction
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, o: wl_output::WlOutput) {
        self.add_monitor(o);
        self.refresh();
    }

    /// An output whose name only arrives with a later update still gets a pill.
    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        o: wl_output::WlOutput,
    ) {
        self.add_monitor(o);
        self.refresh();
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        o: wl_output::WlOutput,
    ) {
        self.monitors.retain(|monitor| monitor.output != o);
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);
