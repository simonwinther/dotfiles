//! Animated Hyprland workspace pill, drawn straight onto a layer-shell surface.

mod hypr;
mod render;

use std::collections::HashSet;
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

    let preset = std::env::var("WAYBAR_OUTPUT_NAME").ok().filter(|name| !name.is_empty());
    let Some((name, snapshot)) = initial_snapshot(preset.as_deref()) else {
        return;
    };

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

    let mut event_loop = EventLoop::<App>::try_new().expect("event loop");
    let handle = event_loop.handle();

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool: None,
        layer: None,
        pointer: None,
        renderer: None,
        pixmap: None,
        qh: qh.clone(),
        monitor: name,
        scale: 1,
        configured: false,
        frame_pending: false,
        active: snapshot.active.clamp(1, WORKSPACE_COUNT),
        occupied: snapshot.occupied,
        deferred_occupied: None,
        position: 0.0,
        start_position: 0.0,
        target_position: 0.0,
        animation_started: Instant::now(),
        animation_duration: 0.20,
        animating: false,
        pointer_position: (0.0, 0.0),
        scroll_accumulator: 0.0,
        exit: false,
    };
    app.position = center_for(app.active);
    app.start_position = app.position;
    app.target_position = app.position;

    let mut queue = event_queue;
    // One roundtrip so the outputs are known before we pick one.
    if queue.roundtrip(&mut app).is_err() {
        return;
    }

    let output = app.output_state.outputs().find(|output| {
        app.output_state.info(output).and_then(|info| info.name) == Some(app.monitor.clone())
    });
    let Some(output) = output else {
        return;
    };
    app.scale = app.output_state.info(&output).map(|info| info.scale_factor).unwrap_or(1).max(1);

    let Some(renderer) = Renderer::new(app.scale as f32) else {
        return;
    };
    let buffer_bytes = (renderer.width() * renderer.height() * 4) as usize;
    app.pixmap = Pixmap::new(renderer.width(), renderer.height());
    app.renderer = Some(renderer);
    app.pool = SlotPool::new(buffer_bytes, &app.shm).ok();
    if app.pixmap.is_none() || app.pool.is_none() {
        return;
    }

    let surface = compositor.create_surface(&qh);
    surface.set_buffer_scale(app.scale);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Bottom,
        Some(format!("waybar-workspace-slider-{}", app.monitor)),
        Some(&output),
    );
    layer.set_size(PANEL_WIDTH as u32, PANEL_HEIGHT as u32);
    layer.set_anchor(Anchor::TOP);
    layer.set_margin(PANEL_TOP, 0, 0, 0);
    // Draw over Waybar's reserved strip instead of below it.
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();
    app.layer = Some(layer);

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
        if event_loop.dispatch(None, &mut app).is_err() || app.exit {
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
    let occupied: HashSet<i32> = [2, 3, 7].into_iter().collect();
    renderer.draw(&mut pixmap, center_for(3), &occupied, None);
    if let Err(error) = pixmap.save_png(path) {
        eprintln!("save failed: {error}");
    }
}

/// Keep asking until Hyprland's IPC answers; giving up on the first miss would
/// leave the bar without its pill for the whole session.
fn initial_snapshot(preset: Option<&str>) -> Option<(String, hypr::Snapshot)> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        let name = match preset {
            Some(name) => Some(name.to_owned()),
            None => hypr::focused_monitor(),
        };
        if let Some(name) = name {
            if let Some(snapshot) = hypr::snapshot(&name, WORKSPACE_COUNT) {
                return Some((name, snapshot));
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(STARTUP_RETRY);
    }
}

struct App {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: Option<SlotPool>,
    layer: Option<LayerSurface>,
    pointer: Option<wl_pointer::WlPointer>,
    renderer: Option<Renderer>,
    pixmap: Option<Pixmap>,
    qh: QueueHandle<App>,

    monitor: String,
    scale: i32,
    configured: bool,
    frame_pending: bool,

    active: i32,
    occupied: HashSet<i32>,
    deferred_occupied: Option<i32>,
    position: f32,
    start_position: f32,
    target_position: f32,
    animation_started: Instant,
    animation_duration: f32,
    animating: bool,

    pointer_position: (f64, f64),
    scroll_accumulator: f64,
    exit: bool,
}

impl App {
    fn handle_hypr_event(&mut self, event: hypr::Event) {
        match event {
            // The payload already carries the new id, so start moving without
            // waiting for the state read that follows.
            hypr::Event::Switched(workspace) => {
                if (1..=WORKSPACE_COUNT).contains(&workspace) && workspace != self.active {
                    self.start_slide(workspace);
                    self.request_frame();
                }
            }
            hypr::Event::Changed => self.refresh(),
        }
    }

    fn refresh(&mut self) {
        let Some(snapshot) = hypr::snapshot(&self.monitor, WORKSPACE_COUNT) else {
            return;
        };
        let mut changed = snapshot.occupied != self.occupied;
        self.occupied = snapshot.occupied;
        let workspace = snapshot.active;
        if (1..=WORKSPACE_COUNT).contains(&workspace) && workspace != self.active {
            self.start_slide(workspace);
            changed = true;
        }
        if changed {
            self.request_frame();
        }
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
    fn request_frame(&mut self) {
        if self.frame_pending || !self.configured {
            return;
        }
        if let Some(layer) = &self.layer {
            self.frame_pending = true;
            let surface = layer.wl_surface();
            surface.frame(&self.qh, FrameCallbackData(surface.clone()));
            surface.commit();
        }
    }

    fn workspace_at(&self, x: f64) -> i32 {
        let workspace = ((x as f32 - FIRST_CENTER) / SLOT_WIDTH).round() as i32 + 1;
        workspace.clamp(1, WORKSPACE_COUNT)
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
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

        let (Some(renderer), Some(pixmap), Some(pool), Some(layer)) =
            (&self.renderer, &mut self.pixmap, &mut self.pool, &self.layer)
        else {
            return;
        };

        renderer.draw(pixmap, self.position, &self.occupied, self.deferred_occupied);

        let width = renderer.width() as i32;
        let height = renderer.height() as i32;
        let Ok((buffer, canvas)) =
            pool.create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        // tiny-skia is premultiplied RGBA; wl_shm ARGB8888 is premultiplied
        // BGRA in memory on little-endian.
        for (out, src) in canvas.chunks_exact_mut(4).zip(pixmap.data().chunks_exact(4)) {
            out[0] = src[2];
            out[1] = src[1];
            out[2] = src[0];
            out[3] = src[3];
        }

        let surface = layer.wl_surface();
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

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let factor = new_factor.max(1);
        if factor == self.scale {
            return;
        }
        self.scale = factor;
        if let Some(renderer) = Renderer::new(factor as f32) {
            self.pixmap = Pixmap::new(renderer.width(), renderer.height());
            self.renderer = Some(renderer);
        }
        if let Some(layer) = &self.layer {
            layer.wl_surface().set_buffer_scale(factor);
        }
        self.draw(qh);
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
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.frame_pending = false;
        self.draw(qh);
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
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.configured = true;
        self.frame_pending = false;
        self.draw(qh);
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
        let Some(layer) = &self.layer else { return };
        for event in events {
            if &event.surface != layer.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.pointer_position = event.position;
                }
                PointerEventKind::Press { .. } => {
                    self.pointer_position = event.position;
                    let workspace = self.workspace_at(event.position.0);
                    hypr::dispatch(format!(
                        "dispatch focusmonitor {} ; dispatch workspace {}",
                        self.monitor, workspace
                    ));
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let notches = if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        self.scroll_accumulator += vertical.absolute;
                        let whole = (self.scroll_accumulator / SCROLL_NOTCH).trunc();
                        self.scroll_accumulator -= whole * SCROLL_NOTCH;
                        whole
                    };
                    if notches != 0.0 {
                        let direction = if notches > 0.0 { "e+1" } else { "e-1" };
                        hypr::dispatch(format!(
                            "dispatch focusmonitor {} ; dispatch workspace {}",
                            self.monitor, direction
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

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _o: wl_output::WlOutput) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _o: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _o: wl_output::WlOutput,
    ) {
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
