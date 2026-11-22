//! Click-through name labels on the top border of every visible Hyprland
//! window, so terminals running different agents are told apart at a glance.
mod hyprland;
mod instance;
mod placement;
mod render;

use placement::{LABEL_HEIGHT, Placement};
use render::Painter;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop, channel,
            timer::{TimeoutAction, Timer},
        },
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_output, wl_shm, wl_surface},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use std::{collections::HashMap, process::ExitCode, time::Duration};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};

/// Catches geometry changes that Hyprland reports without an event, such as
/// interactive resizing.
const POLL_INTERVAL: Duration = Duration::from_millis(400);
const NAMESPACE: &str = "window-labels";

/// What a label surface currently shows.
#[derive(Clone, PartialEq)]
struct Content {
    text: String,
    active: bool,
    width: i32,
    scale: f64,
}

struct Label {
    layer: LayerSurface,
    viewport: wp_viewport::WpViewport,
    output: String,
    configured: bool,
    position: Option<(i32, i32)>,
    content: Option<Content>,
    wanted_position: (i32, i32),
    wanted_content: Content,
    /// Kept alive until the next buffer replaces it on the surface.
    _buffer: Option<Buffer>,
}

impl Drop for Label {
    fn drop(&mut self) {
        self.viewport.destroy();
    }
}

struct App {
    registry: RegistryState,
    outputs: OutputState,
    compositor: CompositorState,
    layers: LayerShell,
    shm: Shm,
    viewporter: wp_viewporter::WpViewporter,
    pool: SlotPool,
    painter: Painter,
    labels: HashMap<String, Label>,
    qh: QueueHandle<Self>,
}

impl App {
    fn output(&self, name: &str) -> Option<wl_output::WlOutput> {
        self.outputs.outputs().find(|output| {
            self.outputs
                .info(output)
                .and_then(|info| info.name)
                .as_deref()
                == Some(name)
        })
    }

    fn refresh(&mut self) {
        let (Ok(clients), Ok(monitors)) = (
            hyprland::request::<Vec<hyprland::Client>>("j/clients"),
            hyprland::request::<Vec<hyprland::Monitor>>("j/monitors"),
        ) else {
            return;
        };
        let wanted = placement::placements(&clients, &monitors);
        self.labels.retain(|address, label| {
            wanted
                .iter()
                .any(|p| &p.address == address && p.output == label.output)
        });
        for placement in wanted {
            self.place(placement);
        }
    }

    fn place(&mut self, placement: Placement) {
        let text = self.painter.fit(&placement.title, placement.max_width);
        let content = Content {
            width: self.painter.width(&text),
            text,
            active: placement.active,
            scale: placement.scale,
        };
        let position = (placement.x, placement.y);
        if !self.labels.contains_key(&placement.address) {
            let Some(label) = self.create_label(&placement.output, position, content.clone())
            else {
                return;
            };
            self.labels.insert(placement.address.clone(), label);
        }
        let label = self
            .labels
            .get_mut(&placement.address)
            .expect("label was just ensured");
        label.wanted_position = position;
        label.wanted_content = content;
        Self::present(label, &mut self.pool, &self.painter);
    }

    fn create_label(
        &self,
        output_name: &str,
        position: (i32, i32),
        content: Content,
    ) -> Option<Label> {
        let output = self.output(output_name)?;
        let surface = self.compositor.create_surface(&self.qh);
        let region = Region::new(&self.compositor).ok()?;
        surface.set_input_region(Some(region.wl_region()));
        let viewport = self.viewporter.get_viewport(&surface, &self.qh, ());
        let layer = self.layers.create_layer_surface(
            &self.qh,
            surface,
            Layer::Top,
            Some(NAMESPACE),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(content.width as u32, LABEL_HEIGHT as u32);
        layer.set_margin(position.1, 0, 0, position.0);
        layer.commit();
        Some(Label {
            layer,
            viewport,
            output: output_name.to_owned(),
            configured: false,
            position: Some(position),
            content: None,
            wanted_position: position,
            wanted_content: content,
            _buffer: None,
        })
    }

    fn present(label: &mut Label, pool: &mut SlotPool, painter: &Painter) {
        if !label.configured {
            return;
        }
        let surface = label.layer.wl_surface().clone();
        let mut changed = false;
        if label.position != Some(label.wanted_position) {
            let (x, y) = label.wanted_position;
            label.layer.set_margin(y, 0, 0, x);
            label.position = Some(label.wanted_position);
            changed = true;
        }
        if label.content.as_ref() != Some(&label.wanted_content) {
            let content = label.wanted_content.clone();
            let Some(pixmap) =
                painter.paint(&content.text, content.active, content.width, content.scale)
            else {
                return;
            };
            let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
            let Ok((buffer, canvas)) =
                pool.create_buffer(width, height, width * 4, wl_shm::Format::Argb8888)
            else {
                return;
            };
            // tiny-skia stores premultiplied RGBA; wl_shm ARGB8888 is BGRA in memory.
            for (target, source) in canvas
                .chunks_exact_mut(4)
                .zip(pixmap.data().chunks_exact(4))
            {
                target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
            }
            label
                .layer
                .set_size(content.width as u32, LABEL_HEIGHT as u32);
            label.viewport.set_destination(content.width, LABEL_HEIGHT);
            surface.set_buffer_scale(1);
            if buffer.attach_to(&surface).is_err() {
                return;
            }
            surface.damage_buffer(0, 0, width, height);
            label._buffer = Some(buffer);
            label.content = Some(content);
            changed = true;
        }
        if changed {
            label.layer.commit();
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
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

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.labels.retain(|_, label| &label.layer != layer);
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        if let Some(label) = self.labels.values_mut().find(|label| &label.layer == layer)
            && !label.configured
        {
            label.configured = true;
            Self::present(label, &mut self.pool, &self.painter);
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
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
delegate_registry!(App);
smithay_client_toolkit::reexports::client::delegate_noop!(App: ignore wp_viewporter::WpViewporter);
smithay_client_toolkit::reexports::client::delegate_noop!(App: ignore wp_viewport::WpViewport);

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&connection)?;
    let qh = queue.handle();
    let shm = Shm::bind(&globals, &qh)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh)?,
        layers: LayerShell::bind(&globals, &qh)?,
        viewporter: globals.bind(&qh, 1..=1, ())?,
        pool: SlotPool::new(64 * 1024, &shm)?,
        shm,
        painter: Painter::new()?.with_borders(hyprland::border_colors()),
        labels: HashMap::new(),
        qh,
    };

    // Learn output names before the first refresh places labels on them.
    queue.roundtrip(&mut app)?;

    let mut event_loop = EventLoop::<App>::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(connection, queue).insert(handle.clone())?;

    let (notify, events) = channel::channel::<()>();
    std::thread::spawn(move || {
        let _ = hyprland::watch_events(|| notify.send(()).is_ok());
    });
    handle.insert_source(events, |_, _, app| app.refresh())?;
    handle.insert_source(Timer::immediate(), |_, _, app| {
        app.refresh();
        TimeoutAction::ToDuration(POLL_INTERVAL)
    })?;

    loop {
        event_loop.dispatch(None, &mut app)?;
    }
}

fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("toggle") => match instance::acquire() {
            Ok(Some(_lock)) => run(),
            Ok(None) => instance::stop_running().map_err(Into::into),
            Err(error) => Err(error.into()),
        },
        Some("start") => match instance::acquire() {
            Ok(Some(_lock)) => run(),
            Ok(None) => Ok(()),
            Err(error) => Err(error.into()),
        },
        Some("stop") => match instance::acquire() {
            Ok(Some(_)) => Ok(()),
            Ok(None) => instance::stop_running().map_err(Into::into),
            Err(error) => Err(error.into()),
        },
        Some("status") => match instance::acquire() {
            Ok(Some(_)) => {
                println!("off");
                Ok(())
            }
            Ok(None) => {
                println!("on");
                Ok(())
            }
            Err(error) => Err(error.into()),
        },
        _ => {
            eprintln!("usage: window-labels toggle|start|stop|status");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("window-labels: {error}");
            ExitCode::FAILURE
        }
    }
}
