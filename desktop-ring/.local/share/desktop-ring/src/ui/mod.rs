mod geometry;
use crate::core::{
    self, Request, Response, Snapshot,
    config::{self, Theme},
    menu::{self, Entry, Menu},
};
use anyhow::{Context, Result};
use gtk::{cairo, gdk, gio, glib, pango, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use std::{
    cell::RefCell,
    f64::consts::{PI, TAU},
    io::{BufRead, Read, Write},
    os::fd::AsRawFd,
    os::unix::net::UnixStream,
    rc::Rc,
    time::Instant,
};

struct View {
    root: Menu,
    path: Vec<String>,
    selected: Option<String>,
    previous: Option<String>,
    hover_start: Instant,
    pointer: Option<(f64, f64)>,
    theme: Theme,
    start: Instant,
    animating: bool,
    pending: bool,
    message: Option<String>,
}
impl View {
    fn select(&mut self, selected: Option<String>) -> bool {
        if selected == self.selected {
            return false;
        }
        self.previous = self.selected.take();
        self.selected = selected;
        self.hover_start = Instant::now();
        true
    }
    fn current(&self) -> Menu {
        let mut menu = self.root.clone();
        for id in &self.path {
            if let Some(sub) = menu
                .entries
                .iter()
                .find(|e| &e.id == id)
                .and_then(|e| e.submenu.clone())
            {
                menu = sub;
            } else {
                break;
            }
        }
        menu
    }
    fn update(&mut self, snapshot: Snapshot) {
        self.root = menu::paged(&menu::build(&snapshot));
        self.message = snapshot.message;
        // Drop navigation levels whose hardware disappeared, preserving all surviving levels.
        let mut menu = &self.root;
        let mut keep = 0;
        for id in &self.path {
            if let Some(sub) = menu
                .entries
                .iter()
                .find(|e| &e.id == id)
                .and_then(|e| e.submenu.as_ref())
            {
                menu = sub;
                keep += 1;
            } else {
                break;
            }
        }
        self.path.truncate(keep);
        if !self
            .current()
            .entries
            .iter()
            .any(|e| Some(&e.id) == self.selected.as_ref())
        {
            self.selected = None;
        }
    }
}
fn color(value: &str) -> (f64, f64, f64) {
    let s = value.trim_start_matches('#');
    if s.len() == 6
        && let Ok(n) = u32::from_str_radix(s, 16)
    {
        return (
            ((n >> 16) & 255) as f64 / 255.0,
            ((n >> 8) & 255) as f64 / 255.0,
            (n & 255) as f64 / 255.0,
        );
    }
    (0.1, 0.1, 0.15)
}
fn rgb(cr: &cairo::Context, c: (f64, f64, f64), alpha: f64) {
    cr.set_source_rgba(c.0, c.1, c.2, alpha);
}
// A drawing primitive keeps font, placement, wrapping, and Cairo context explicit.
#[allow(clippy::too_many_arguments)]
fn text(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    label: &str,
    font: &str,
    size: f64,
    x: f64,
    y: f64,
    width: f64,
    lines: i32,
) {
    let height = (size * 1.4 * lines as f64).ceil() + 2.0;
    bounded_text(
        area,
        cr,
        label,
        font,
        size,
        geometry::Rect::centered(x, y, width, height),
        lines,
    );
}

#[allow(clippy::too_many_arguments)]
fn bounded_text(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    label: &str,
    font: &str,
    size: f64,
    bounds: geometry::Rect,
    lines: i32,
) {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }
    // Normalize paragraphs so Pango's line limit is a global label budget.
    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    let layout = area.create_pango_layout(Some(&label));
    let mut description = pango::FontDescription::from_string(font);
    layout.set_width((bounds.width * pango::SCALE as f64) as i32);
    layout.set_alignment(pango::Alignment::Center);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    for factor in [1.0, 0.95, 0.90, 0.85] {
        description.set_absolute_size(size * factor * pango::SCALE as f64);
        layout.set_font_description(Some(&description));
        layout.set_height(-lines);
        if !layout.is_ellipsized() && layout.pixel_size().1 as f64 <= bounds.height {
            break;
        }
    }
    if layout.pixel_size().1 as f64 > bounds.height {
        layout.set_height((bounds.height * pango::SCALE as f64) as i32);
    }
    let (_, height) = layout.pixel_size();
    let _ = cr.save();
    cr.rectangle(bounds.x, bounds.y, bounds.width, bounds.height);
    cr.clip();
    cr.move_to(
        bounds.x,
        bounds.y + (bounds.height - height as f64).max(0.0) / 2.0,
    );
    pangocairo::functions::show_layout(cr, &layout);
    let _ = cr.restore();
}

fn animate(area: &gtk::DrawingArea, view: &Rc<RefCell<View>>) {
    {
        let mut v = view.borrow_mut();
        v.start = Instant::now();
        v.hover_start = v.start;
        v.previous = None;
    }
    ticks(area, view);
}
fn ticks(area: &gtk::DrawingArea, view: &Rc<RefCell<View>>) {
    let mut v = view.borrow_mut();
    area.queue_draw();
    if v.animating {
        return;
    }
    v.animating = true;
    drop(v);
    let weak = Rc::downgrade(view);
    area.add_tick_callback(move |area, _| {
        let Some(view) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mut view = view.borrow_mut();
        area.queue_draw();
        let duration = view.theme.animation_ms as u128;
        if view.start.elapsed().as_millis() >= duration
            && view.hover_start.elapsed().as_millis() >= duration
        {
            view.animating = false;
            view.previous = None;
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}
fn blend(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    (
        a.0 + (b.0 - a.0) * t,
        a.1 + (b.1 - a.1) * t,
        a.2 + (b.2 - a.2) * t,
    )
}
fn progress(start: Instant, duration: u32) -> f64 {
    if duration == 0 {
        1.0
    } else {
        (start.elapsed().as_secs_f64() * 1000.0 / duration as f64).min(1.0)
    }
}
// Small curved corners soften each annular sector without changing its radial hit region.
fn sector(cr: &cairo::Context, inner: f64, outer: f64, start: f64, end: f64) {
    let corner = 5.0;
    let point = |r: f64, a: f64| (r * a.cos(), r * a.sin());
    let curve = |control: (f64, f64), end: (f64, f64)| {
        cr.curve_to(control.0, control.1, control.0, control.1, end.0, end.1);
    };
    cr.new_path();
    let p = point(outer, start + corner / outer);
    cr.move_to(p.0, p.1);
    cr.arc(
        0.0,
        0.0,
        outer,
        start + corner / outer,
        end - corner / outer,
    );
    curve(point(outer, end), point(outer - corner, end));
    let p = point(inner + corner, end);
    cr.line_to(p.0, p.1);
    curve(point(inner, end), point(inner, end - corner / inner));
    cr.arc_negative(
        0.0,
        0.0,
        inner,
        end - corner / inner,
        start + corner / inner,
    );
    curve(point(inner, start), point(inner + corner, start));
    let p = point(outer - corner, start);
    cr.line_to(p.0, p.1);
    curve(point(outer, start), point(outer, start + corner / outer));
    cr.close_path();
}
fn draw(area: &gtk::DrawingArea, cr: &cairo::Context, w: i32, h: i32, view: &View) {
    let menu = view.current();
    let count = menu.entries.len();
    let (cx, cy, r) = geometry::center(w as f64, h as f64, view.theme.radius, view.pointer);
    let inner = r * 0.39;
    let unit = (r / 214.0).min(1.0);
    let ease = 1.0 - (1.0 - progress(view.start, view.theme.animation_ms)).powi(3);
    let hover = 1.0 - (1.0 - progress(view.hover_start, view.theme.animation_ms)).powi(3);
    let bg = color(&view.theme.background);
    let fg = color(&view.theme.foreground);
    let accent = color(&view.theme.accent);
    let surface = blend(bg, fg, 0.12);
    // The dimmed backdrop and opaque disc keep content readable over any wallpaper/window.
    rgb(cr, (0.0, 0.0, 0.0), 0.20 * ease);
    let _ = cr.paint();
    let _ = cr.save();
    cr.translate(cx, cy);
    cr.scale(0.96 + 0.04 * ease, 0.96 + 0.04 * ease);
    for spread in (1..=12).rev() {
        rgb(cr, (0.0, 0.0, 0.0), 0.025 * ease);
        cr.arc(0.0, 4.0, r + spread as f64, 0.0, TAU);
        let _ = cr.fill();
    }
    rgb(cr, blend(bg, fg, 0.045), ease);
    cr.arc(0.0, 0.0, r + 2.0, 0.0, TAU);
    let _ = cr.fill();
    for (i, e) in menu.entries.iter().enumerate() {
        let step = TAU / count as f64;
        let middle = -PI / 2.0 + i as f64 * step;
        let start = middle - step / 2.0 + 0.012;
        let end = middle + step / 2.0 - 0.012;
        let selected = view.selected.as_ref() == Some(&e.id);
        let previous = view.previous.as_ref() == Some(&e.id);
        let highlight = if !e.enabled {
            0.0
        } else if selected {
            hover
        } else if previous {
            1.0 - hover
        } else {
            0.0
        };
        sector(cr, inner, r, start, end);
        rgb(cr, blend(surface, accent, highlight * 0.30), ease);
        let _ = cr.fill_preserve();
        rgb(
            cr,
            blend(fg, accent, highlight),
            (0.10 + highlight * 0.72) * ease,
        );
        cr.set_line_width(1.0 + highlight * 0.4);
        let _ = cr.stroke();
        if highlight > 0.01 {
            rgb(cr, accent, highlight * ease);
            cr.set_line_width(2.5);
            cr.arc(0.0, 0.0, r - 4.0, start + 0.06, end - 0.06);
            let _ = cr.stroke();
        }
        let (icon_bounds, label_bounds) = geometry::entry_text_bounds(inner, r, middle, step);
        let _ = cr.save();
        // Geometry keeps labels readable inside a slice; clipping also contains
        // unusual font ink overhang and oversized fallback glyphs.
        sector(cr, inner, r, start, end);
        cr.clip();
        rgb(
            cr,
            if e.active || selected { accent } else { fg },
            if e.enabled { ease } else { 0.38 * ease },
        );
        bounded_text(
            area,
            cr,
            &e.icon,
            &view.theme.icon_font,
            26.0 * unit,
            icon_bounds,
            1,
        );
        rgb(
            cr,
            blend(fg, (1.0, 1.0, 1.0), if selected { 0.35 } else { 0.12 }),
            if e.enabled { ease } else { 0.38 * ease },
        );
        bounded_text(
            area,
            cr,
            &e.label,
            &view.theme.font,
            13.5 * unit,
            label_bounds,
            2,
        );
        let _ = cr.restore();
        if e.active {
            rgb(cr, accent, ease);
            cr.new_path();
            cr.arc(
                middle.cos() * (r - 16.0),
                middle.sin() * (r - 16.0),
                3.5,
                0.0,
                TAU,
            );
            let _ = cr.fill();
        } else if e.submenu.is_some() {
            rgb(cr, fg, (0.45 + highlight * 0.5) * ease);
            text(
                area,
                cr,
                "›",
                &view.theme.font,
                17.0 * unit,
                middle.cos() * (r - 17.0),
                middle.sin() * (r - 17.0),
                20.0,
                1,
            );
        }
    }
    rgb(cr, blend(bg, fg, 0.065), ease);
    cr.new_path();
    cr.arc(0.0, 0.0, inner - 5.0, 0.0, TAU);
    let _ = cr.fill_preserve();
    rgb(cr, fg, 0.12 * ease);
    cr.set_line_width(1.0);
    let _ = cr.stroke();
    rgb(cr, blend(fg, (1.0, 1.0, 1.0), 0.3), ease);
    let _ = cr.save();
    cr.new_path();
    cr.arc(0.0, 0.0, inner - 7.0, 0.0, TAU);
    cr.clip();
    bounded_text(
        area,
        cr,
        &menu.label,
        &view.theme.font,
        15.0 * unit,
        geometry::Rect::centered(0.0, -25.0 * unit, inner * 1.42, 34.0 * unit),
        2,
    );
    let selected = menu
        .entries
        .iter()
        .find(|e| view.selected.as_ref() == Some(&e.id));
    let detail = if view.pending {
        "Working…"
    } else {
        selected
            .map(|e| e.detail.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(if view.path.is_empty() {
                "Move to choose"
            } else {
                "‹ Click centre to go back"
            })
    };
    rgb(cr, fg, 0.80 * ease);
    bounded_text(
        area,
        cr,
        detail,
        &view.theme.font,
        11.0 * unit,
        geometry::Rect::centered(0.0, 19.0 * unit, inner * 1.40, 38.0 * unit),
        3,
    );
    let _ = cr.restore();
    // A quiet, legible hint, with the complete key list documented outside the menu.
    rgb(cr, blend(bg, fg, 0.10), 0.98 * ease);
    let hint_width = 244.0 * unit;
    let hint_y = r + 23.0;
    cr.new_path();
    cr.arc(
        -hint_width / 2.0 + 14.0,
        hint_y,
        14.0,
        PI / 2.0,
        3.0 * PI / 2.0,
    );
    cr.arc(hint_width / 2.0 - 14.0, hint_y, 14.0, -PI / 2.0, PI / 2.0);
    cr.close_path();
    let _ = cr.fill();
    rgb(cr, fg, 0.80 * ease);
    text(
        area,
        cr,
        "Esc close  ·  Right click back",
        &view.theme.font,
        11.5 * unit,
        0.0,
        hint_y,
        hint_width - 10.0,
        1,
    );
    if let Some(message) = &view.message {
        rgb(cr, accent, ease);
        text(
            area,
            cr,
            message,
            &view.theme.font,
            12.0 * unit,
            0.0,
            -r - 27.0,
            (r * 2.1).min(w as f64 - 24.0),
            2,
        );
    }
    let _ = cr.restore();
}
fn back(view: &Rc<RefCell<View>>, area: &gtk::DrawingArea, window: &gtk::ApplicationWindow) {
    let mut v = view.borrow_mut();
    if v.path.pop().is_none() {
        drop(v);
        window.close();
        return;
    }
    v.selected = None;
    drop(v);
    animate(area, view);
}
fn choose(
    entry: Option<Entry>,
    view: &Rc<RefCell<View>>,
    area: &gtk::DrawingArea,
    window: &gtk::ApplicationWindow,
) {
    let Some(entry) = entry else {
        back(view, area, window);
        return;
    };
    if !entry.enabled {
        return;
    }
    if entry.submenu.is_some() {
        let mut v = view.borrow_mut();
        v.path.push(entry.id);
        v.selected = None;
        drop(v);
        animate(area, view);
        return;
    }
    if let Some(action) = entry.action {
        if view.borrow().pending {
            return;
        }
        view.borrow_mut().pending = true;
        area.queue_draw();
        // Keep compositor and audio/DBus work off GTK's input/render thread.
        let (mut receive, mut send) = UnixStream::pair().expect("local action channel");
        receive.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let response = rt.block_on(core::client(&Request::Act { action }));
            let line = match response {
                Ok(r) => serde_json::to_string(&r).unwrap(),
                Err(e) => serde_json::to_string(&Response {
                    ok: false,
                    error: Some(e.to_string()),
                    snapshot: Snapshot::default(),
                })
                .unwrap(),
            };
            let _ = writeln!(send, "{line}");
        });
        let view = view.clone();
        let area = area.clone();
        let window = window.clone();
        let mut buffer: Vec<u8> = vec![];
        glib::source::unix_fd_add_local(
            receive.as_raw_fd(),
            glib::IOCondition::IN | glib::IOCondition::HUP,
            move |_, _| {
                let mut bytes = [0; 8192];
                if let Ok(n) = receive.read(&mut bytes) {
                    buffer.extend_from_slice(&bytes[..n]);
                }
                if let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                    view.borrow_mut().pending = false;
                    if let Ok(response) = serde_json::from_slice::<Response>(&buffer[..end]) {
                        if response.ok {
                            if entry.close {
                                window.close();
                            } else {
                                view.borrow_mut().update(response.snapshot);
                                area.queue_draw();
                            }
                        } else {
                            view.borrow_mut().message = response.error;
                            area.queue_draw();
                        }
                    }
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            },
        );
    }
}
fn themed(mut theme: Theme) -> Theme {
    if !theme.follow_desktop_palette {
        return theme;
    }
    // Read the existing desktop palette when opening; declarative defaults remain the fallback.
    if let Ok(data) = std::fs::read_to_string(
        config::base("XDG_STATE_HOME", ".local/state").join("desktop/theme/colors.toml"),
    ) {
        for line in data.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim().trim_matches('"');
                if value.len() == 7 && value.starts_with('#') {
                    match key.trim() {
                        "background" => theme.background = value.into(),
                        "foreground" => theme.foreground = value.into(),
                        "accent" => theme.accent = value.into(),
                        _ => {}
                    }
                }
            }
        }
    }
    theme
}
pub fn run() -> Result<()> {
    let config = config::load()?;
    // A unique GtkApplication makes a repeated global keypress dismiss the open menu.
    let app = gtk::Application::new(Some("org.desktop.Ring"), gio::ApplicationFlags::empty());
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.close();
            return;
        }
        if !gtk4_layer_shell::is_supported() {
            eprintln!("desktop-ring requires Wayland layer-shell");
            app.quit();
            return;
        }
        let result = open(app, themed(config.theme.clone()));
        if let Err(e) = result {
            eprintln!("desktop-ring: {e}");
            app.quit();
        }
    });
    app.run_with_args::<&str>(&[]);
    Ok(())
}
fn open(app: &gtk::Application, theme: Theme) -> Result<()> {
    let mut subscription = UnixStream::connect(core::socket_path()?)
        .context("start desktop-ring.service before opening the menu")?;
    subscription.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
    writeln!(
        subscription,
        "{}",
        serde_json::to_string(&Request::Subscribe)?
    )?;
    let mut initial = std::io::BufReader::new(subscription.try_clone()?);
    let mut line = String::new();
    initial.read_line(&mut line)?;
    let initial: Response = serde_json::from_str(&line)?;
    subscription.set_read_timeout(None)?;
    subscription.set_nonblocking(true)?;
    let display = gdk::Display::default().context("no GDK display")?;
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Desktop Ring")
        .build();
    window.init_layer_shell();
    window.set_namespace(Some("desktop-ring"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    window.set_exclusive_zone(-1);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (monitor_name, pointer) = rt.block_on(async {
        if let Ok(dir) = crate::monitors::ipc::socket_dir()
            && let (Ok(ms), Ok(pos)) = (
                crate::monitors::ipc::discover().await,
                crate::monitors::ipc::request_at(&dir, "j/cursorpos").await,
            )
            && let Ok(pos) = serde_json::from_str::<serde_json::Value>(&pos)
        {
            let (x, y) = (
                pos["x"].as_f64().unwrap_or(0.0),
                pos["y"].as_f64().unwrap_or(0.0),
            );
            for m in ms.iter().filter(|m| !m.disabled) {
                let rule = crate::monitors::current_rule(m);
                if let Ok((w, h)) = crate::monitors::logical_size(&rule.mode, m.scale, m.transform)
                    && x >= m.x as f64
                    && x < (m.x + w) as f64
                    && y >= m.y as f64
                    && y < (m.y + h) as f64
                {
                    return (Some(m.name.clone()), Some((x - m.x as f64, y - m.y as f64)));
                }
            }
        }
        (None, None)
    });
    let monitors = display.monitors();
    let mut chosen = None;
    for i in 0..monitors.n_items() {
        if let Some(m) = monitors.item(i).and_downcast::<gdk::Monitor>()
            && m.connector().as_deref() == monitor_name.as_deref()
        {
            chosen = Some(m);
            break;
        }
    }
    let chosen = chosen.or_else(|| monitors.item(0).and_downcast::<gdk::Monitor>());
    if let Some(m) = chosen {
        window.set_monitor(Some(&m));
        let weak_window = window.downgrade();
        m.connect_invalidate(move |_| {
            if let Some(window) = weak_window.upgrade() {
                window.close();
            }
        });
    }
    let css = gtk::CssProvider::new();
    css.load_from_string("window.desktop-ring { background: transparent; box-shadow: none; }");
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    window.add_css_class("desktop-ring");
    let area = gtk::DrawingArea::new();
    area.set_focusable(true);
    window.set_child(Some(&area));
    let view = Rc::new(RefCell::new(View {
        root: menu::paged(&menu::build(&initial.snapshot)),
        path: vec![],
        selected: None,
        previous: None,
        hover_start: Instant::now(),
        pointer,
        theme,
        start: Instant::now(),
        animating: false,
        pending: false,
        message: initial.snapshot.message,
    }));
    let v = view.clone();
    area.set_draw_func(move |area, cr, w, h| draw(area, cr, w, h, &v.borrow()));
    let motion = gtk::EventControllerMotion::new();
    let v = view.clone();
    let a = area.clone();
    motion.connect_motion(move |_, x, y| {
        let mut state = v.borrow_mut();
        let current = state.current();
        let (cx, cy, r) = geometry::center(
            a.width() as f64,
            a.height() as f64,
            state.theme.radius,
            state.pointer,
        );
        let selected = geometry::sector_at(x, y, (cx, cy), r * 0.39, r, current.entries.len())
            .map(|i| current.entries[i].id.clone());
        let changed = state.select(selected);
        drop(state);
        if changed {
            ticks(&a, &v);
        }
    });
    area.add_controller(motion);
    let click = gtk::GestureClick::new();
    click.set_button(0);
    let v = view.clone();
    let a = area.clone();
    let w = window.clone();
    click.connect_released(move |click, _, x, y| {
        if click.current_button() == 3 {
            back(&v, &a, &w);
            return;
        }
        if click.current_button() != 1 {
            return;
        }
        let entry = {
            let v = v.borrow();
            let current = v.current();
            let (cx, cy, r) = geometry::center(
                a.width() as f64,
                a.height() as f64,
                v.theme.radius,
                v.pointer,
            );
            let distance = (x - cx).hypot(y - cy);
            if distance > r {
                w.close();
                return;
            }
            geometry::sector_at(x, y, (cx, cy), r * 0.39, r, current.entries.len())
                .map(|i| current.entries[i].clone())
        };
        choose(entry, &v, &a, &w);
    });
    area.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let v = view.clone();
    let a = area.clone();
    let w = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        match key {
            gdk::Key::Escape => w.close(),
            gdk::Key::BackSpace | gdk::Key::Left => back(&v, &a, &w),
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => {
                let entry = {
                    let v = v.borrow();
                    v.current()
                        .entries
                        .iter()
                        .find(|e| v.selected.as_ref() == Some(&e.id))
                        .cloned()
                };
                if entry.is_some() {
                    choose(entry, &v, &a, &w);
                }
            }
            gdk::Key::Tab | gdk::Key::Down | gdk::Key::Right | gdk::Key::Up => {
                let mut state = v.borrow_mut();
                let menu = state.current();
                let n = menu.entries.len();
                if n > 0 {
                    let current = menu
                        .entries
                        .iter()
                        .position(|e| state.selected.as_ref() == Some(&e.id));
                    let next = if key == gdk::Key::Up {
                        current.map_or(n - 1, |i| (i + n - 1) % n)
                    } else {
                        current.map_or(0, |i| (i + 1) % n)
                    };
                    let changed = state.select(Some(menu.entries[next].id.clone()));
                    drop(state);
                    if changed {
                        ticks(&a, &v);
                    }
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);
    let v = view.clone();
    let a = area.clone();
    let weak_window = window.downgrade();
    let mut buffer: Vec<u8> = vec![];
    glib::source::unix_fd_add_local(
        subscription.as_raw_fd(),
        glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
        move |_, _| {
            let mut bytes = [0u8; 16384];
            let mut ended = false;
            loop {
                match subscription.read(&mut bytes) {
                    Ok(0) => {
                        ended = true;
                        break;
                    }
                    Ok(n) => buffer.extend_from_slice(&bytes[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        ended = true;
                        break;
                    }
                }
            }
            while let Some(end) = buffer.iter().position(|b| *b == b'\n') {
                let line = buffer[..end].to_vec();
                buffer.drain(..=end);
                if let Ok(response) = serde_json::from_slice::<Response>(&line) {
                    v.borrow_mut().update(response.snapshot);
                    a.queue_draw();
                }
            }
            if ended {
                if let Some(window) = weak_window.upgrade() {
                    window.close();
                }
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        },
    );
    window.present();
    area.grab_focus();
    animate(&area, &view);
    Ok(())
}
