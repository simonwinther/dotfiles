mod thumbs;

use anyhow::{Context, Result, bail, ensure};
use clipboard_picker::{Hyprland, Item, Payload};
use gtk::{gdk, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
use thumbs::Pixels;

#[derive(Clone)]
struct Options {
    target: String,
    delay: u64,
    check: bool,
}

fn options() -> Result<Options> {
    let mut opts = Options {
        target: String::new(),
        delay: 800,
        check: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => opts.target = args.next().context("--target needs a window address")?,
            "--delay-ms" => {
                opts.delay = args.next().context("--delay-ms needs a number")?.parse()?
            }
            "--check" => opts.check = true,
            "--help" | "-h" => {
                println!(
                    "Clipboard picker\n\nClick images or press Space to select in order. Enter pastes the selection.\nTab / Shift+Tab: next / previous category. hjkl / arrows: move.\nCtrl+J / Ctrl+K: down / up, including from search. Ctrl+D / Ctrl+U: half-page down / up.\n/ or i: search. Esc: leave search / preview, then close. p or Ctrl+Space: preview.\nCtrl+Enter: copy. Ctrl+Shift+Enter: copy image files.\nCtrl+I: images. Ctrl+T: text. Ctrl+R: refresh. Ctrl+P: pin.\nCtrl+A: select visible cards. Ctrl+Shift+A: clear selection.\n\nOptions: --target ADDRESS --delay-ms 800 --check"
                );
                std::process::exit(0);
            }
            _ => bail!("Unknown option: {arg}"),
        }
    }
    ensure!(
        (200..=10000).contains(&opts.delay),
        "Delay must be 200–10000 ms"
    );
    if opts.target.is_empty() && !opts.check {
        opts.target = clipboard_picker::active_window()
            .ok()
            .and_then(|v| v["address"].as_str().map(str::to_owned))
            .unwrap_or_default();
    }
    Ok(opts)
}

fn xdg(variable: &str, fallback: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback)
        })
}

fn apply_style() {
    let state = xdg("XDG_STATE_HOME", ".local/state").join("desktop");
    let config = xdg("XDG_CONFIG_HOME", ".config");
    let read_json = |path: PathBuf| {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .unwrap_or_default()
    };
    let preferences = read_json(state.join("state.json"));
    let settings = read_json(config.join("desktop/settings.json"));
    let font = preferences["font"]
        .as_str()
        .or(settings["defaultFont"].as_str())
        .unwrap_or("monospace");
    let palette = std::fs::read_to_string(state.join("theme/walker.css")).unwrap_or_default();
    let custom =
        std::fs::read_to_string(config.join("clipboard-picker/style.css")).unwrap_or_default();
    let css = gtk::CssProvider::new();
    css.load_from_data(&format!(
        "@define-color text #cfc9c2; @define-color base #000; @define-color border #33ccff; @define-color selected-text #7dcfff;\n{palette}\n{}\n* {{ font-family: {}; }}\n{custom}",
        include_str!("../style.css"), serde_json::to_string(font).unwrap()));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn label(text: &str, class: &str) -> gtk::Label {
    let widget = gtk::Label::new(Some(text));
    widget.set_xalign(0.);
    if !class.is_empty() {
        widget.add_css_class(class);
    }
    widget
}

struct Ui {
    window: gtk::ApplicationWindow,
    search: gtk::SearchEntry,
    all: gtk::ToggleButton,
    images: gtk::ToggleButton,
    texts: gtk::ToggleButton,
    flow: gtk::FlowBox,
    scroll: gtk::ScrolledWindow,
    stack: gtk::Stack,
    picture: gtk::Picture,
    text_scroll: gtk::ScrolledWindow,
    preview_text: gtk::Label,
    detail: gtk::Label,
    count: gtk::Label,
    results: gtk::Label,
    error: gtk::Label,
    paste: gtk::Button,
    files: gtk::Button,
    pace: gtk::SpinButton,
    pin: gtk::Button,
    remove: gtk::Button,
    menu: gtk::MenuButton,
    empty: gtk::Label,
    items: RefCell<Vec<Item>>,
    cards: RefCell<Vec<gtk::ToggleButton>>,
    badges: RefCell<Vec<gtk::Label>>,
    pictures: RefCell<Vec<Option<gtk::Picture>>>,
    selected: RefCell<Vec<String>>,
    current: Cell<Option<usize>>,
    generation: Cell<u64>,
    preview_ticket: Rc<Cell<u64>>,
    busy: Cell<bool>,
    target: String,
}

impl Ui {
    fn show_error(&self, text: &str) {
        self.error.set_text(text);
        self.error.set_visible(!text.is_empty());
    }

    fn chosen(&self) -> Vec<Item> {
        let items = self.items.borrow();
        let selected = self.selected.borrow();
        if selected.is_empty() {
            self.current
                .get()
                .and_then(|i| items.get(i).cloned())
                .into_iter()
                .collect()
        } else {
            selected
                .iter()
                .filter_map(|id| items.iter().find(|i| &i.identifier == id).cloned())
                .collect()
        }
    }

    fn update(&self) {
        let count = self.selected.borrow().len();
        self.count.set_text(&if count == 0 {
            "space select · hjkl move".into()
        } else {
            format!("{count} selected")
        });
        if count > 0 {
            self.count.add_css_class("selected");
        } else {
            self.count.remove_css_class("selected");
        }
        self.paste.set_label(&if count > 1 {
            format!("paste {count}  ↵")
        } else {
            "paste  ↵".into()
        });
        let items = self.chosen();
        self.files
            .set_sensitive(!items.is_empty() && items.iter().all(Item::image));
        self.paste
            .set_sensitive(!self.target.is_empty() && !items.is_empty());
    }

    fn preview(&self, index: usize) {
        self.current.set(Some(index));
        let items = self.items.borrow();
        let Some(item) = items.get(index) else {
            return;
        };
        if let Some(title) = self
            .pin
            .child()
            .and_then(|row| row.first_child())
            .and_then(|w| w.downcast::<gtk::Label>().ok())
        {
            title.set_text(if item.pinned() { "Unpin" } else { "Pin" });
        }
        self.pin.set_sensitive(true);
        self.remove
            .set_sensitive(item.actions.iter().any(|a| a == "remove"));
        if self.stack.visible_child_name().as_deref() != Some("preview") {
            return;
        }
        self.detail.set_text(&item.subtext);
        self.picture.set_visible(item.image());
        self.text_scroll.set_visible(!item.image());
        if !item.image() {
            self.preview_text
                .set_text(&item.preview.chars().take(20000).collect::<String>());
            return;
        }
        // Full-size decoding happens only when requested, away from the UI thread.
        if let Some(Some(thumbnail)) = self.pictures.borrow().get(index) {
            self.picture.set_paintable(thumbnail.paintable().as_ref());
        }
        let ticket = self.preview_ticket.get() + 1;
        self.preview_ticket.set(ticket);
        let current = self.preview_ticket.clone();
        let picture = self.picture.clone();
        let path = item.preview.clone();
        let (send, recv) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(Pixels::load(&path, 760, 350, None));
        });
        glib::timeout_add_local(Duration::from_millis(16), move || {
            if current.get() != ticket {
                return glib::ControlFlow::Break;
            }
            match recv.try_recv() {
                Ok(Some(pixels)) => {
                    picture.set_paintable(Some(&pixels.texture()));
                    glib::ControlFlow::Break
                }
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    }

    fn toggle_preview(&self) {
        if self.stack.visible_child_name().as_deref() == Some("preview") {
            self.stack.set_visible_child_name("gallery");
            self.focus_current();
        } else if let Some(index) = self.current.get() {
            self.stack.set_visible_child_name("preview");
            self.stack.grab_focus();
            self.preview(index);
        }
    }

    fn focus_current(&self) {
        if self.stack.visible_child_name().as_deref() == Some("preview") {
            self.stack.grab_focus();
        } else if let Some(cell) = self
            .current
            .get()
            .and_then(|i| self.cards.borrow().get(i).and_then(|c| c.parent()))
        {
            cell.grab_focus();
        } else {
            self.flow.grab_focus();
        }
    }

    fn cycle_category(&self, reverse: bool) {
        let searching = self.search_focused();
        let tabs = [&self.all, &self.images, &self.texts];
        let current = tabs.iter().position(|t| t.is_active()).unwrap_or(0);
        let next = (current + if reverse { tabs.len() - 1 } else { 1 }) % tabs.len();
        self.stack.set_visible_child_name("gallery");
        tabs[next].set_active(true);
        if !searching {
            self.focus_current();
        }
    }

    fn focus_search(&self) {
        self.stack.set_visible_child_name("gallery");
        self.search.grab_focus();
    }

    fn filter(&self) {
        let query = self.search.text().to_string();
        let mut visible = Vec::new();
        for (index, (item, card)) in self
            .items
            .borrow()
            .iter()
            .zip(self.cards.borrow().iter())
            .enumerate()
        {
            let show = item.matches(&query, self.images.is_active())
                && (!self.texts.is_active() || !item.image());
            if show {
                visible.push(index);
            }
            if let Some(cell) = card.parent() {
                cell.set_visible(show);
            }
        }
        self.results.set_text(&format!("{} items", visible.len()));
        self.empty.set_visible(visible.is_empty());
        if !self.current.get().is_some_and(|i| visible.contains(&i)) {
            self.current.set(visible.first().copied());
            if let Some(&index) = visible.first() {
                self.preview(index);
            } else {
                self.pin.set_sensitive(false);
                self.remove.set_sensitive(false);
                self.stack.set_visible_child_name("gallery");
            }
        }
        self.update();
    }

    fn search_focused(&self) -> bool {
        gtk::prelude::RootExt::focus(&self.window)
            .is_some_and(|f| f == self.search || f.is_ancestor(&self.search))
    }

    fn navigate(&self, key: gdk::Key) {
        let cards = self.cards.borrow();
        let visible: Vec<usize> = cards
            .iter()
            .enumerate()
            .filter(|(_, c)| c.parent().is_some_and(|p| p.is_visible()))
            .map(|(i, _)| i)
            .collect();
        let Some(&first) = visible.first() else {
            return;
        };
        let first_y = cards[first].parent().unwrap().allocation().y();
        let columns = visible
            .iter()
            .take_while(|&&i| cards[i].parent().unwrap().allocation().y() == first_y)
            .count()
            .max(1);
        let position = visible
            .iter()
            .position(|&i| self.current.get() == Some(i))
            .unwrap_or(0);
        let row_height = (cards[first].parent().unwrap().allocation().height()
            + self.flow.row_spacing() as i32)
            .max(1) as usize;
        let half_page =
            (self.scroll.allocation().height().max(1) as usize / 2 / row_height).max(1) * columns;
        let next =
            if self.search_focused() && !matches!(key, gdk::Key::Page_Down | gdk::Key::Page_Up) {
                0
            } else {
                match key {
                    gdk::Key::Left => position.saturating_sub(1),
                    gdk::Key::Right => (position + 1).min(visible.len() - 1),
                    gdk::Key::Up => position.saturating_sub(columns),
                    gdk::Key::Page_Up => position.saturating_sub(half_page),
                    gdk::Key::Page_Down => (position + half_page).min(visible.len() - 1),
                    _ => (position + columns).min(visible.len() - 1),
                }
            };
        let index = visible[next];
        if self.stack.visible_child_name().as_deref() == Some("gallery")
            && let Some(cell) = cards[index].parent()
        {
            cell.grab_focus();
        } else if self.stack.visible_child_name().as_deref() == Some("preview") {
            self.stack.grab_focus();
        }
        drop(cards);
        self.preview(index);
        self.update();
    }

    fn reload(self: &Rc<Self>) {
        self.show_error("");
        let (send, recv) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(clipboard_picker::history().map_err(|e| format!("{e:#}")));
        });
        let ui = self.clone();
        glib::timeout_add_local(Duration::from_millis(16), move || match recv.try_recv() {
            Ok(Ok(items)) => {
                ui.populate(items);
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                ui.show_error(&error);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                ui.show_error("History connection closed");
                glib::ControlFlow::Break
            }
        });
    }

    fn populate(self: &Rc<Self>, items: Vec<Item>) {
        self.generation.set(self.generation.get() + 1);
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.cards.borrow_mut().clear();
        self.badges.borrow_mut().clear();
        self.pictures.borrow_mut().clear();
        self.selected
            .borrow_mut()
            .retain(|id| items.iter().any(|i| &i.identifier == id));
        *self.items.borrow_mut() = items.clone();
        self.current.set(None);
        let mut jobs = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let button = gtk::ToggleButton::new();
            button.set_focusable(false);
            button.add_css_class("clip-card");
            button.set_size_request(166, 104);
            button.set_hexpand(true);
            let body = gtk::Box::new(gtk::Orientation::Vertical, 5);
            let overlay = gtk::Overlay::new();
            if item.image() {
                let picture = gtk::Picture::new();
                picture.set_can_shrink(true);
                picture.set_content_fit(gtk::ContentFit::Contain);
                picture.set_size_request(154, 72);
                picture.add_css_class("clip-image");
                overlay.set_child(Some(&picture));
                self.pictures.borrow_mut().push(Some(picture));
                jobs.push((index, item.identifier.clone(), item.preview.clone()));
            } else {
                let excerpt = label(
                    &item.preview.chars().take(200).collect::<String>(),
                    "clip-excerpt",
                );
                excerpt.set_wrap(true);
                excerpt.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                excerpt.set_ellipsize(gtk::pango::EllipsizeMode::End);
                excerpt.set_lines(4);
                excerpt.set_max_width_chars(22);
                excerpt.set_size_request(154, 72);
                excerpt.set_valign(gtk::Align::Start);
                overlay.set_child(Some(&excerpt));
                self.pictures.borrow_mut().push(None);
            }
            let badge = label("", "clip-order");
            badge.set_halign(gtk::Align::End);
            badge.set_valign(gtk::Align::Start);
            badge.set_xalign(0.5);
            badge.set_can_target(false);
            badge.set_visible(false);
            overlay.add_overlay(&badge);
            self.badges.borrow_mut().push(badge);
            body.append(&overlay);
            let caption = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let kind = if item.image() {
                PathBuf::from(&item.preview)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("image")
                    .to_uppercase()
            } else {
                "TEXT".into()
            };
            let kind = label(
                &format!("{}{}", if item.pinned() { "★ " } else { "" }, kind),
                "clip-caption",
            );
            caption.append(&kind);
            let time = label(
                item.subtext
                    .split_whitespace()
                    .nth(4)
                    .unwrap_or("")
                    .get(..5)
                    .unwrap_or(""),
                "clip-caption",
            );
            time.set_hexpand(true);
            time.set_xalign(1.);
            caption.append(&time);
            body.append(&caption);
            button.set_child(Some(&body));
            button.set_active(self.selected.borrow().contains(&item.identifier));
            self.flow.insert(&button, -1);
            let cell = button.parent().unwrap();
            cell.set_focusable(true);
            self.cards.borrow_mut().push(button.clone());
            let ui = self.clone();
            let id = item.identifier.clone();
            button.connect_toggled(move |button| {
                if let Some(cell) = button.parent() {
                    cell.grab_focus();
                }
                {
                    let mut selected = ui.selected.borrow_mut();
                    if button.is_active() {
                        if !selected.contains(&id) {
                            selected.push(id.clone());
                        }
                    } else {
                        selected.retain(|s| s != &id);
                    }
                }
                ui.preview(index);
                ui.update();
                ui.order_labels();
            });
            let ui = self.clone();
            cell.connect_has_focus_notify(move |cell| {
                if cell.has_focus() {
                    ui.preview(index);
                    ui.update();
                }
            });
        }
        if !items.is_empty() {
            self.preview(0);
        }
        self.filter();
        self.order_labels();
        // Keep initial mapping and keyboard handling independent of image I/O.
        let (send, recv) = mpsc::channel();
        std::thread::spawn(move || {
            for (index, key, path) in jobs {
                let pixels = Pixels::load(&path, 154, 72, Some(&key));
                if send.send((index, pixels)).is_err() {
                    break;
                }
            }
        });
        let ui = self.clone();
        let generation = self.generation.get();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            if ui.generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            for _ in 0..8 {
                match recv.try_recv() {
                    Ok((index, Some(pixels))) => {
                        if let Some(Some(picture)) = ui.pictures.borrow().get(index) {
                            picture.set_paintable(Some(&pixels.texture()));
                        }
                    }
                    Ok((_, None)) => (),
                    Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn order_labels(&self) {
        let items = self.items.borrow();
        let selected = self.selected.borrow();
        for (index, badge) in self.badges.borrow().iter().enumerate() {
            let order = selected
                .iter()
                .position(|id| *id == items[index].identifier);
            badge.set_visible(order.is_some());
            badge.set_text(&order.map(|p| (p + 1).to_string()).unwrap_or_default());
        }
    }

    fn select_visible(&self, selected: bool) {
        for card in self.cards.borrow().clone() {
            if !selected || card.parent().is_some_and(|p| p.is_visible()) {
                card.set_active(selected);
            }
        }
        self.update();
    }

    fn operate(self: &Rc<Self>, pinning: bool) {
        let item = self
            .current
            .get()
            .and_then(|i| self.items.borrow().get(i).cloned());
        let Some(item) = item else {
            return;
        };
        let action = if pinning {
            if item.pinned() { "unpin" } else { "pin" }
        } else {
            "remove"
        };
        if let Err(error) = clipboard_picker::activate(&item, action) {
            self.show_error(&format!("{error:#}"));
        } else {
            let ui = self.clone();
            glib::timeout_add_local_once(Duration::from_millis(120), move || ui.reload());
        }
    }

    fn action(self: &Rc<Self>, action: &str) {
        if self.busy.get() {
            return;
        }
        let items = self.chosen();
        if items.is_empty() {
            return;
        }
        self.show_error("");
        if action == "paste" {
            let payloads = match clipboard_picker::prepare_batch(&items) {
                Ok(value) => value,
                Err(error) => {
                    self.show_error(&format!("{error:#}"));
                    return;
                }
            };
            self.busy.set(true);
            self.window.set_visible(false);
            let target = self.target.clone();
            let delay = self.pace.value_as_int() as u64;
            let app = self.window.application().unwrap();
            let mut hold = Some(app.hold());
            let (send, recv) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(
                    clipboard_picker::paste_batch(
                        &mut Hyprland,
                        &target,
                        &payloads,
                        Duration::from_millis(delay),
                    )
                    .map_err(|e| format!("{e:#}")),
                );
            });
            let ui = self.clone();
            glib::timeout_add_local(Duration::from_millis(40), move || match recv.try_recv() {
                Ok(result) => {
                    hold.take();
                    if let Err(error) = result {
                        ui.busy.set(false);
                        ui.show_error(&error);
                        ui.window.present();
                    } else {
                        app.quit();
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(_) => {
                    hold.take();
                    ui.busy.set(false);
                    ui.show_error("Paste worker stopped");
                    ui.window.present();
                    glib::ControlFlow::Break
                }
            });
        } else {
            let result = if action == "files" || (items.len() > 1 && items.iter().all(Item::image))
            {
                clipboard_picker::export_files(&items).and_then(|p| clipboard_picker::copy(&p))
            } else if items.len() == 1 {
                clipboard_picker::activate(&items[0], "copy")
            } else if items.iter().all(|i| !i.image() && i.preview_type == "text") {
                clipboard_picker::copy(&Payload {
                    mime: "text/plain;charset=utf-8".into(),
                    bytes: items
                        .iter()
                        .map(|i| i.preview.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                        .into_bytes(),
                })
            } else {
                Err(anyhow::anyhow!("For mixed text and images, use Paste"))
            };
            if let Err(error) = result {
                self.show_error(&format!("{error:#}"));
            } else {
                self.window.application().unwrap().quit();
            }
        }
    }
}

fn menu_action(title: &str, shortcut: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("clip-option");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let name = label(title, "");
    name.set_hexpand(true);
    row.append(&name);
    row.append(&label(shortcut, "clip-option-key"));
    button.set_child(Some(&row));
    button
}

fn build(app: &gtk::Application, opts: &Options) -> Rc<Ui> {
    apply_style();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Clipboard")
        .default_width(800)
        .default_height(480)
        .decorated(false)
        .build();
    if gtk4_layer_shell::is_supported() {
        window.init_layer_shell();
        window.set_namespace(Some("clipboard-picker"));
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
    }
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("clip-root");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let search = gtk::SearchEntry::new();
    search.add_css_class("clip-search");
    search.set_placeholder_text(Some("Search clipboard…"));
    search.set_hexpand(true);
    header.append(&search);
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("view-more-symbolic");
    menu.add_css_class("clip-menu");
    header.append(&menu);
    root.append(&header);
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toolbar.add_css_class("clip-toolbar");
    let all = gtk::ToggleButton::with_label("all");
    let images = gtk::ToggleButton::with_label("images");
    let texts = gtk::ToggleButton::with_label("text");
    images.set_group(Some(&all));
    texts.set_group(Some(&all));
    all.set_active(true);
    for button in [&all, &images, &texts] {
        button.add_css_class("clip-tab");
        toolbar.append(button);
    }
    let results = label("loading…", "clip-meta");
    results.set_hexpand(true);
    results.set_xalign(1.);
    toolbar.append(&results);
    root.append(&toolbar);
    let stack = gtk::Stack::new();
    stack.set_focusable(true);
    stack.set_vexpand(true);
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    let flow = gtk::FlowBox::new();
    flow.set_focusable(true);
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(4);
    flow.set_column_spacing(8);
    flow.set_row_spacing(8);
    flow.set_valign(gtk::Align::Start);
    flow.set_homogeneous(true);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_child(Some(&flow));
    scroll.set_vexpand(true);
    let gallery = gtk::Overlay::new();
    gallery.set_child(Some(&scroll));
    let empty = label("Copy an image or text to start your history", "clip-empty");
    empty.set_halign(gtk::Align::Center);
    empty.set_valign(gtk::Align::Center);
    gallery.add_overlay(&empty);
    stack.add_named(&gallery, Some("gallery"));
    let preview = gtk::Box::new(gtk::Orientation::Vertical, 8);
    preview.add_css_class("clip-preview");
    let picture = gtk::Picture::new();
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_vexpand(true);
    preview.append(&picture);
    let preview_text = label("", "clip-preview-text");
    preview_text.set_wrap(true);
    preview_text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    preview_text.set_selectable(true);
    preview_text.set_valign(gtk::Align::Start);
    preview_text.set_max_width_chars(90);
    let text_scroll = gtk::ScrolledWindow::new();
    text_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    text_scroll.set_child(Some(&preview_text));
    text_scroll.set_vexpand(true);
    preview.append(&text_scroll);
    let detail = label("", "clip-meta");
    preview.append(&detail);
    stack.add_named(&preview, Some("preview"));
    stack.set_visible_child_name("gallery");
    root.append(&stack);
    let error = label("", "clip-error");
    error.set_wrap(true);
    error.set_visible(false);
    root.append(&error);
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("clip-footer");
    let count = label("space select · hjkl move", "clip-count");
    count.set_hexpand(true);
    footer.append(&count);
    footer.append(&label("tab category", "clip-hint"));
    footer.append(&label("p preview", "clip-hint"));
    footer.append(&label("esc back", "clip-hint"));
    let paste = gtk::Button::with_label("paste  ↵");
    paste.add_css_class("clip-action");
    footer.append(&paste);
    root.append(&footer);
    let popover = gtk::Popover::new();
    popover.set_halign(gtk::Align::End);
    let options = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let copy = menu_action("Copy", "Ctrl Enter");
    let files = menu_action("Copy files", "Ctrl Shift Enter");
    let pin = menu_action("Pin", "Ctrl P");
    let remove = menu_action("Remove", "Delete");
    let select = menu_action("Select visible", "Ctrl A");
    let clear = menu_action("Clear selection", "Ctrl Shift A");
    let refresh = menu_action("Refresh", "Ctrl R");
    for button in [&copy, &files, &pin, &remove, &select, &clear, &refresh] {
        options.append(button);
    }
    let delay = gtk::Box::new(gtk::Orientation::Horizontal, 20);
    delay.add_css_class("clip-pace");
    delay.append(&label("Paste interval · ms", "clip-meta"));
    let pace = gtk::SpinButton::with_range(200., 10000., 100.);
    pace.set_value(opts.delay as f64);
    delay.append(&pace);
    options.append(&delay);
    popover.set_child(Some(&options));
    menu.set_popover(Some(&popover));
    window.set_child(Some(&root));
    let ui = Rc::new(Ui {
        window,
        search,
        all,
        images,
        texts,
        flow,
        scroll,
        stack,
        picture,
        text_scroll,
        preview_text,
        detail,
        count,
        results,
        error,
        paste,
        files,
        pace,
        pin,
        remove,
        menu,
        empty,
        items: RefCell::new(Vec::new()),
        cards: RefCell::new(Vec::new()),
        badges: RefCell::new(Vec::new()),
        pictures: RefCell::new(Vec::new()),
        selected: RefCell::new(Vec::new()),
        current: Cell::new(None),
        generation: Cell::new(0),
        preview_ticket: Rc::new(Cell::new(0)),
        busy: Cell::new(false),
        target: opts.target.clone(),
    });
    let u = ui.clone();
    ui.search.connect_search_changed(move |_| u.filter());
    for button in [&ui.all, &ui.images, &ui.texts] {
        let u = ui.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                u.filter();
            }
        });
    }
    let u = ui.clone();
    ui.paste.connect_clicked(move |_| u.action("paste"));
    for (button, action) in [(&copy, "copy"), (&ui.files, "files")] {
        let u = ui.clone();
        let pop = popover.clone();
        button.connect_clicked(move |_| {
            pop.popdown();
            u.action(action);
        });
    }
    for (button, pinning) in [(&ui.pin, true), (&ui.remove, false)] {
        let u = ui.clone();
        let pop = popover.clone();
        button.connect_clicked(move |_| {
            pop.popdown();
            u.operate(pinning);
        });
    }
    let u = ui.clone();
    let pop = popover.clone();
    select.connect_clicked(move |_| {
        pop.popdown();
        u.select_visible(true);
    });
    let u = ui.clone();
    let pop = popover.clone();
    clear.connect_clicked(move |_| {
        pop.popdown();
        u.select_visible(false);
    });
    let u = ui.clone();
    refresh.connect_clicked(move |_| {
        popover.popdown();
        u.reload();
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let u = ui.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        if u.menu.popover().is_some_and(|p| p.is_visible()) {
            return glib::Propagation::Proceed;
        }
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let plain = !mods.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        );
        match key {
            gdk::Key::Escape => {
                if u.stack.visible_child_name().as_deref() == Some("preview") {
                    u.toggle_preview();
                } else if u.search_focused() {
                    u.focus_current();
                } else {
                    u.window.application().unwrap().quit();
                }
            }
            gdk::Key::Return | gdk::Key::KP_Enter => u.action(if ctrl && shift {
                "files"
            } else if ctrl {
                "copy"
            } else {
                "paste"
            }),
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
                u.cycle_category(shift || key == gdk::Key::ISO_Left_Tab)
            }
            gdk::Key::space if ctrl => u.toggle_preview(),
            gdk::Key::p if plain && !u.search_focused() => u.toggle_preview(),
            gdk::Key::comma if ctrl => u.menu.popup(),
            gdk::Key::a | gdk::Key::A if ctrl && shift => u.select_visible(false),
            gdk::Key::a | gdk::Key::A if ctrl && !u.search_focused() => u.select_visible(true),
            gdk::Key::l | gdk::Key::L if ctrl => u.focus_search(),
            gdk::Key::slash | gdk::Key::i if plain && !u.search_focused() => u.focus_search(),
            gdk::Key::i | gdk::Key::I if ctrl => {
                if u.images.is_active() {
                    u.all.set_active(true);
                } else {
                    u.images.set_active(true);
                }
            }
            gdk::Key::t | gdk::Key::T if ctrl => {
                if u.texts.is_active() {
                    u.all.set_active(true);
                } else {
                    u.texts.set_active(true);
                }
            }
            gdk::Key::r | gdk::Key::R if ctrl => u.reload(),
            gdk::Key::p | gdk::Key::P if ctrl => u.operate(true),
            gdk::Key::j | gdk::Key::J if ctrl => u.navigate(gdk::Key::Down),
            gdk::Key::k | gdk::Key::K if ctrl => u.navigate(gdk::Key::Up),
            gdk::Key::d | gdk::Key::D if ctrl => u.navigate(gdk::Key::Page_Down),
            gdk::Key::u | gdk::Key::U if ctrl => u.navigate(gdk::Key::Page_Up),
            gdk::Key::h if plain && !u.search_focused() => u.navigate(gdk::Key::Left),
            gdk::Key::j if plain && !u.search_focused() => u.navigate(gdk::Key::Down),
            gdk::Key::k if plain && !u.search_focused() => u.navigate(gdk::Key::Up),
            gdk::Key::l if plain && !u.search_focused() => u.navigate(gdk::Key::Right),
            gdk::Key::Delete if !u.search_focused() => u.operate(false),
            gdk::Key::Down => u.navigate(key),
            gdk::Key::Page_Down | gdk::Key::Page_Up => u.navigate(key),
            gdk::Key::Left | gdk::Key::Right | gdk::Key::Up if !u.search_focused() => {
                u.navigate(key)
            }
            gdk::Key::space if !u.search_focused() => {
                let card = u
                    .current
                    .get()
                    .and_then(|i| u.cards.borrow().get(i).cloned());
                if let Some(card) = card {
                    card.set_active(!card.is_active());
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    ui.window.add_controller(keys);
    ui.window.present();
    ui.search.grab_focus();
    ui.reload();
    ui
}

fn run() -> Result<()> {
    let opts = options()?;
    if opts.check {
        let items = clipboard_picker::history()?;
        println!(
            "Connected to history: {} entries, {} images",
            items.len(),
            items.iter().filter(|i| i.image()).count()
        );
        return Ok(());
    }
    let app = gtk::Application::builder()
        .application_id("local.desktop.ClipboardPicker")
        .build();
    let ui: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if let Some(ui) = ui.borrow().as_ref() {
            if !ui.busy.get() {
                ui.window.present();
            }
            return;
        }
        *ui.borrow_mut() = Some(build(app, &opts));
    });
    app.run_with_args::<&str>(&[]);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("clipboard-picker: {error:#}");
        std::process::exit(1);
    }
}
