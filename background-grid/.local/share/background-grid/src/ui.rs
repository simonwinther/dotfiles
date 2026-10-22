use background_grid::Result;
use background_grid::catalog::{Folder, catalog};
use background_grid::config::Paths;
use background_grid::thumbnail::thumbnail;
use background_grid::wallpaper::set_wallpaper;
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const COLUMNS: usize = 4;
const WINDOW_WIDTH: i32 = 900;
const HINT: &str = "H J K L / arrows  ·  Enter opens / applies  ·  Ctrl+click / Enter keeps open  ·  q / Esc closes";

enum Work {
    Preview(usize, PathBuf),
    Apply(PathBuf, bool),
    Stop,
}

enum Event {
    Preview(usize, std::result::Result<PathBuf, String>),
    Applied(PathBuf, bool, std::result::Result<(), String>),
}

struct Workers {
    sender: mpsc::Sender<Work>,
    results: Rc<mpsc::Receiver<Event>>,
    stopped: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl Workers {
    fn new(paths: &Paths) -> Self {
        let (sender, jobs) = mpsc::channel();
        let jobs = Arc::new(Mutex::new(jobs));
        let (events, results) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let (jobs, events, stopped, paths) =
                (jobs.clone(), events.clone(), stopped.clone(), paths.clone());
            handles.push(thread::spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let job = jobs.lock().expect("thumbnail queue poisoned").recv();
                    let event = match job {
                        Ok(Work::Preview(id, path)) => Event::Preview(
                            id,
                            thumbnail(&path, &paths.cache).map_err(|error| error.to_string()),
                        ),
                        Ok(Work::Apply(path, keep_open)) => {
                            let result = set_wallpaper(&paths, &path, None)
                                .map_err(|error| error.to_string());
                            Event::Applied(path, keep_open, result)
                        }
                        _ => break,
                    };
                    let _ = events.send(event);
                }
            }));
        }
        Self {
            sender,
            results: Rc::new(results),
            stopped,
            handles,
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        for _ in &self.handles {
            let _ = self.sender.send(Work::Stop);
        }
        // Finish an in-flight wallpaper change before the process exits.
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    Image(usize),
    Group(usize),
}

struct Tile {
    button: gtk::Button,
    picture: gtk::Picture,
    path: PathBuf,
}

struct Group {
    node: Folder,
    header: gtk::Button,
    body: gtk::Box,
    arrow: gtk::Image,
    expanded: bool,
    loaded: bool,
    buttons: Vec<usize>,
    children: Vec<usize>,
}

#[derive(Default)]
struct State {
    closed: bool,
    busy: bool,
    keep_open: bool,
    focused: Option<Control>,
    active: PathBuf,
    tiles: Vec<Tile>,
    groups: Vec<Group>,
    root_buttons: Vec<usize>,
    root_groups: Vec<usize>,
    remaining: usize,
    loaded_images: usize,
}

struct Grid {
    app: gtk::Application,
    window: gtk::ApplicationWindow,
    sections: gtk::Box,
    scroll: gtk::ScrolledWindow,
    status: gtk::Label,
    sender: mpsc::Sender<Work>,
    results: Rc<mpsc::Receiver<Event>>,
    state: RefCell<State>,
    started: Instant,
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::builder().label(text).xalign(0.0).build();
    label.add_css_class(class);
    label
}

fn debug(message: &str) {
    if std::env::var_os("BACKGROUND_GRID_DEBUG").is_some() {
        println!("{message}");
    }
}

impl Grid {
    fn new(
        app: &gtk::Application,
        paths: &Paths,
        workers: &Workers,
        collection: &Folder,
    ) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Backgrounds")
            .decorated(false)
            .resizable(false)
            .default_width(WINDOW_WIDTH)
            .build();
        window.add_css_class("background-grid");
        let provider = gtk::CssProvider::new();
        provider.load_from_path(paths.config.join("style.css"));
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 14);
        outer.set_margin_top(20);
        outer.set_margin_bottom(20);
        outer.set_margin_start(20);
        outer.set_margin_end(20);
        window.set_child(Some(&outer));
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.append(&label("Backgrounds", "heading"));
        let count = label(
            &format!("{} images", collection.all_images().len()),
            "muted",
        );
        count.set_hexpand(true);
        header.append(&count);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("close-button");
        close.set_tooltip_text(Some("Close (q / Esc)"));
        header.append(&close);
        outer.append(&header);
        let sections = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(550);
        scroll.set_child(Some(&sections));
        outer.append(&scroll);
        let status = label(HINT, "muted");
        outer.append(&status);
        let grid = Rc::new(Self {
            app: app.clone(),
            window,
            sections,
            scroll,
            status,
            sender: workers.sender.clone(),
            results: workers.results.clone(),
            state: RefCell::new(State {
                active: paths.current().canonicalize().unwrap_or_default(),
                ..State::default()
            }),
            started: Instant::now(),
        });
        let weak = Rc::downgrade(&grid);
        close.connect_clicked(move |_| {
            if let Some(grid) = weak.upgrade() {
                grid.app.quit();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&grid);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            weak.upgrade()
                .map_or(glib::Propagation::Proceed, |grid| grid.key(key, modifiers))
        });
        grid.window.add_controller(keys);
        let buttons = grid.add_grid(&grid.sections, &collection.images);
        let groups = collection
            .children
            .iter()
            .map(|node| grid.add_group(&grid.sections, node.clone()))
            .collect();
        {
            let mut state = grid.state.borrow_mut();
            state.root_buttons = buttons;
            state.root_groups = groups;
        }
        let rows = grid.visible_rows();
        let selected = {
            let state = grid.state.borrow();
            state
                .root_buttons
                .iter()
                .find(|&&id| state.tiles[id].path == state.active)
                .map(|&id| Control::Image(id))
                .or_else(|| rows.first().and_then(|row| row.first()).copied())
        };
        if let Some(selected) = selected {
            grid.focus(selected);
        } else {
            grid.status
                .set_label("No images found in your wallpaper folders.");
        }
        let weak = Rc::downgrade(&grid);
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(grid) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if grid.state.borrow().closed {
                return glib::ControlFlow::Break;
            }
            for event in grid.results.try_iter().take(32) {
                grid.event(event);
            }
            glib::ControlFlow::Continue
        });
        grid.window.present();
        grid
    }

    fn button(&self, control: Control) -> gtk::Button {
        let state = self.state.borrow();
        match control {
            Control::Image(id) => state.tiles[id].button.clone(),
            Control::Group(id) => state.groups[id].header.clone(),
        }
    }

    fn visible_rows(&self) -> Vec<Vec<Control>> {
        fn image_rows(result: &mut Vec<Vec<Control>>, buttons: &[usize]) {
            result.extend(
                buttons
                    .chunks(COLUMNS)
                    .map(|row| row.iter().map(|&id| Control::Image(id)).collect()),
            );
        }
        fn group_rows(result: &mut Vec<Vec<Control>>, state: &State, id: usize) {
            let group = &state.groups[id];
            result.push(vec![Control::Group(id)]);
            if group.expanded {
                image_rows(result, &group.buttons);
                for &child in &group.children {
                    group_rows(result, state, child);
                }
            }
        }
        let state = self.state.borrow();
        let mut rows = Vec::new();
        image_rows(&mut rows, &state.root_buttons);
        for &group in &state.root_groups {
            group_rows(&mut rows, &state, group);
        }
        rows
    }

    fn focus(self: &Rc<Self>, control: Control) {
        let previous = self.state.borrow_mut().focused.replace(control);
        if let Some(previous) = previous {
            self.button(previous).remove_css_class("keyboard-focus");
        }
        let button = self.button(control);
        button.add_css_class("keyboard-focus");
        button.grab_focus();
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(grid) = weak.upgrade() {
                grid.ensure_visible(&button);
            }
        });
        let controls: Vec<_> = self.visible_rows().into_iter().flatten().collect();
        if let Some(index) = controls.iter().position(|&candidate| candidate == control) {
            debug(&format!("Focus: {index}"));
        }
    }

    fn ensure_visible(&self, button: &gtk::Button) {
        if self.state.borrow().closed {
            return;
        }
        if let Some(bounds) = button.compute_bounds(&self.sections) {
            let adjustment = self.scroll.vadjustment();
            let top = f64::from(bounds.y());
            let bottom = top + f64::from(bounds.height());
            if top < adjustment.value() {
                adjustment.set_value((top - 4.0).max(0.0));
            } else if bottom > adjustment.value() + adjustment.page_size() {
                adjustment.set_value(bottom - adjustment.page_size() + 4.0);
            }
        }
    }

    fn key(self: &Rc<Self>, key: gdk::Key, modifiers: gdk::ModifierType) -> glib::Propagation {
        if key == gdk::Key::q || key == gdk::Key::Escape {
            self.app.quit();
            return glib::Propagation::Stop;
        }
        let rows = self.visible_rows();
        let controls: Vec<_> = rows.iter().flatten().copied().collect();
        if controls.is_empty() || self.state.borrow().busy {
            return glib::Propagation::Proceed;
        }
        let direction = match key {
            gdk::Key::Left | gdk::Key::h => Some((true, -1)),
            gdk::Key::Right | gdk::Key::l => Some((true, 1)),
            gdk::Key::Up | gdk::Key::k => Some((false, -1)),
            gdk::Key::Down | gdk::Key::j => Some((false, 1)),
            _ => None,
        };
        let activate = matches!(key, gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space);
        if direction.is_none() && !activate {
            return glib::Propagation::Proceed;
        }
        let mut widget = gtk::prelude::GtkWindowExt::focus(&self.window);
        let mut focused = None;
        while let Some(current) = widget {
            focused = controls
                .iter()
                .find(|&&control| self.button(control).upcast_ref::<gtk::Widget>() == &current)
                .copied();
            if focused.is_some() {
                break;
            }
            widget = current.parent();
        }
        let focused = focused
            .or_else(|| {
                self.state
                    .borrow()
                    .focused
                    .filter(|value| controls.contains(value))
            })
            .unwrap_or(controls[0]);
        if let Some((horizontal, delta)) = direction {
            if horizontal && matches!(focused, Control::Group(_)) {
                let Control::Group(id) = focused else {
                    unreachable!()
                };
                let expanded = self.state.borrow().groups[id].expanded;
                if (delta < 0 && expanded) || (delta > 0 && !expanded) {
                    self.toggle(id);
                } else if delta > 0 {
                    let index = controls
                        .iter()
                        .position(|&control| control == focused)
                        .unwrap();
                    self.focus(controls[(index + 1).min(controls.len() - 1)]);
                }
            } else if horizontal {
                let index = controls
                    .iter()
                    .position(|&control| control == focused)
                    .unwrap();
                self.focus(controls[index.saturating_add_signed(delta).min(controls.len() - 1)]);
            } else {
                let row = rows.iter().position(|row| row.contains(&focused)).unwrap();
                let column = rows[row]
                    .iter()
                    .position(|&control| control == focused)
                    .unwrap();
                let target = &rows[row.saturating_add_signed(delta).min(rows.len() - 1)];
                self.focus(target[column.min(target.len() - 1)]);
            }
        } else {
            self.state.borrow_mut().keep_open = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            self.button(focused).emit_clicked();
        }
        glib::Propagation::Stop
    }

    fn add_group(self: &Rc<Self>, parent: &gtk::Box, node: Folder) -> usize {
        let container = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let header = gtk::Button::new();
        header.add_css_class("category");
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let arrow = gtk::Image::from_icon_name("pan-end-symbolic");
        line.append(&arrow);
        let name = gtk::Label::builder()
            .label(&node.name)
            .xalign(0.0)
            .hexpand(true)
            .build();
        line.append(&name);
        let count = node.all_images().len();
        line.append(&label(&count.to_string(), "muted"));
        header.set_child(Some(&line));
        header.update_property(&[gtk::accessible::Property::Label(&format!(
            "{}, {count} images",
            node.name
        ))]);
        header.update_state(&[gtk::accessible::State::Expanded(Some(false))]);
        container.append(&header);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        body.set_visible(false);
        container.append(&body);
        parent.append(&container);
        let id = {
            let mut state = self.state.borrow_mut();
            let id = state.groups.len();
            state.groups.push(Group {
                node,
                header: header.clone(),
                body,
                arrow,
                expanded: false,
                loaded: false,
                buttons: Vec::new(),
                children: Vec::new(),
            });
            id
        };
        let weak = Rc::downgrade(self);
        header.connect_clicked(move |_| {
            if let Some(grid) = weak.upgrade() {
                grid.toggle(id);
            }
        });
        id
    }

    fn toggle(self: &Rc<Self>, id: usize) {
        if self.state.borrow().busy {
            return;
        }
        let (node, body, header, arrow, expanded, load) = {
            let mut state = self.state.borrow_mut();
            state.keep_open = false;
            let group = &mut state.groups[id];
            group.expanded = !group.expanded;
            let load = group.expanded && !group.loaded;
            if load {
                group.loaded = true;
            }
            (
                group.node.clone(),
                group.body.clone(),
                group.header.clone(),
                group.arrow.clone(),
                group.expanded,
                load,
            )
        };
        if load {
            let buttons = self.add_grid(&body, &node.images);
            let children = node
                .children
                .iter()
                .map(|child| self.add_group(&body, child.clone()))
                .collect();
            if node.images.is_empty() && node.children.is_empty() {
                body.append(&label("No images in this folder yet", "muted"));
            }
            let mut state = self.state.borrow_mut();
            state.groups[id].buttons = buttons;
            state.groups[id].children = children;
        }
        body.set_visible(expanded);
        arrow.set_icon_name(Some(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
        header.update_state(&[gtk::accessible::State::Expanded(Some(expanded))]);
        self.window.set_default_size(WINDOW_WIDTH, -1);
        self.focus(Control::Group(id));
        if expanded {
            let weak = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_millis(80), move || {
                if let Some(grid) = weak.upgrade()
                    && !grid.state.borrow().closed
                    && let Some(bounds) = header.compute_bounds(&grid.sections)
                {
                    let adjustment = grid.scroll.vadjustment();
                    adjustment.set_value(
                        f64::from(bounds.y())
                            .min((adjustment.upper() - adjustment.page_size()).max(0.0)),
                    );
                }
            });
        }
        debug(&format!("Category: {} expanded={expanded}", node.name));
    }

    fn add_grid(self: &Rc<Self>, parent: &gtk::Box, paths: &[PathBuf]) -> Vec<usize> {
        if paths.is_empty() {
            return Vec::new();
        }
        let flow = gtk::FlowBox::new();
        flow.set_homogeneous(true);
        flow.set_min_children_per_line(COLUMNS as u32);
        flow.set_max_children_per_line(COLUMNS as u32);
        flow.set_row_spacing(12);
        flow.set_column_spacing(12);
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_valign(gtk::Align::Start);
        parent.append(&flow);
        let mut buttons = Vec::new();
        for path in paths {
            let button = gtk::Button::new();
            button.add_css_class("tile");
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "Wallpaper: {}",
                path.file_stem().unwrap_or_default().to_string_lossy()
            ))]);
            let picture = gtk::Picture::new();
            picture.set_content_fit(gtk::ContentFit::Cover);
            picture.set_can_shrink(true);
            picture.set_size_request(196, 126);
            button.set_child(Some(&picture));
            if path == &self.state.borrow().active {
                button.add_css_class("current");
            }
            button.set_tooltip_text(Some("Ctrl-click to apply and keep the picker open"));
            let click = gtk::GestureClick::new();
            click.set_button(1);
            click.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = Rc::downgrade(self);
            click.connect_pressed(move |gesture, _, _, _| {
                if let Some(grid) = weak.upgrade() {
                    grid.state.borrow_mut().keep_open = gesture
                        .current_event_state()
                        .contains(gdk::ModifierType::CONTROL_MASK);
                }
            });
            button.add_controller(click);
            let id = {
                let mut state = self.state.borrow_mut();
                let id = state.tiles.len();
                state.tiles.push(Tile {
                    button: button.clone(),
                    picture,
                    path: path.clone(),
                });
                state.remaining += 1;
                id
            };
            let weak = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(grid) = weak.upgrade() {
                    grid.choose(id);
                }
            });
            flow.insert(&button, -1);
            buttons.push(id);
            let _ = self.sender.send(Work::Preview(id, path.clone()));
        }
        buttons
    }

    fn choose(self: &Rc<Self>, id: usize) {
        let (path, keep_open) = {
            let mut state = self.state.borrow_mut();
            let keep_open = std::mem::take(&mut state.keep_open);
            if state.busy {
                return;
            }
            state.busy = true;
            (state.tiles[id].path.clone(), keep_open)
        };
        self.focus(Control::Image(id));
        self.status.set_label("Applying…");
        self.sections.set_sensitive(false);
        if !keep_open {
            self.window.set_visible(false);
        }
        if self.sender.send(Work::Apply(path, keep_open)).is_err() {
            self.error("The wallpaper worker stopped.");
        }
    }

    fn event(&self, event: Event) {
        match event {
            Event::Preview(id, result) => {
                let (button, picture) = {
                    let state = self.state.borrow();
                    (
                        state.tiles[id].button.clone(),
                        state.tiles[id].picture.clone(),
                    )
                };
                match result {
                    Ok(path) => picture.set_filename(Some(path)),
                    Err(error) => {
                        let unavailable = gtk::Label::new(Some("Preview unavailable"));
                        unavailable.set_size_request(196, 126);
                        button.set_child(Some(&unavailable));
                        button.set_tooltip_text(Some(&error));
                    }
                }
                let mut state = self.state.borrow_mut();
                state.remaining -= 1;
                state.loaded_images += 1;
                if state.remaining == 0 {
                    println!(
                        "Grid ready: {} previews in {:.3}s",
                        state.loaded_images,
                        self.started.elapsed().as_secs_f64()
                    );
                }
            }
            Event::Applied(path, true, Ok(())) => {
                {
                    let mut state = self.state.borrow_mut();
                    state.active = path.clone();
                    state.busy = false;
                    for tile in &state.tiles {
                        if tile.path == path {
                            tile.button.add_css_class("current");
                        } else {
                            tile.button.remove_css_class("current");
                        }
                    }
                }
                self.sections.set_sensitive(true);
                self.status.set_label(HINT);
                debug("Applied: keep-open");
            }
            Event::Applied(_, false, Ok(())) => self.app.quit(),
            Event::Applied(_, _, Err(error)) => self.error(&error),
        }
    }

    fn error(&self, message: &str) {
        debug(&format!("Apply failed: {message}"));
        self.state.borrow_mut().busy = false;
        self.status.set_label(message);
        self.sections.set_sensitive(true);
        self.window.present();
    }
}

pub fn launch(paths: Paths) -> Result<i32> {
    let collection = catalog(&paths.folders()?)?;
    let app = gtk::Application::builder()
        .application_id("com.simon.BackgroundGrid")
        .build();
    let workers = Rc::new(Workers::new(&paths));
    let holder = Rc::new(RefCell::new(None::<Rc<Grid>>));
    let (held, pool) = (holder.clone(), workers.clone());
    let activation = app.connect_activate(move |app| {
        if let Some(grid) = held.borrow().as_ref() {
            grid.window.present();
            return;
        }
        let grid = Grid::new(app, &paths, &pool, &collection);
        *held.borrow_mut() = Some(grid);
    });
    let held = holder.clone();
    app.connect_shutdown(move |_| {
        if let Some(grid) = held.borrow().as_ref() {
            grid.state.borrow_mut().closed = true;
        }
    });
    let code = app.run_with_args(&["background-grid"]);
    holder.borrow_mut().take();
    app.disconnect(activation);
    drop(app);
    drop(workers);
    Ok(code.value())
}
