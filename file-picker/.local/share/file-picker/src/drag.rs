use file_picker::{
    Colors, Result, appearance_colors, checked_files, config_dir, copy_selection, file_uris, home,
    notify,
};
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use std::cell::Cell;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::builder().label(text).xalign(0.0).build();
    label.add_css_class(class);
    label
}

fn metadata(paths: &[PathBuf]) -> (String, String) {
    if paths.len() > 1 {
        let mut location = paths
            .iter()
            .take(3)
            .map(|path| path.file_name().unwrap_or_default().to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ");
        if paths.len() > 3 {
            location.push_str(&format!(" + {} more", paths.len() - 3));
        }
        return ("Attach together in one drop".into(), location);
    }
    let path = &paths[0];
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let kind = match extension.as_str() {
        "pdf" => "PDF document".into(),
        "rs" => "Rust source".into(),
        "py" => "Python source".into(),
        "md" => "Markdown".into(),
        "txt" => "Text document".into(),
        "" => "File".into(),
        _ => extension.to_uppercase(),
    };
    let detail = path
        .metadata()
        .map(|metadata| {
            let mut size = metadata.len() as f64;
            let mut unit = "B";
            for next in ["B", "KiB", "MiB", "GiB", "TiB"] {
                unit = next;
                if size < 1024.0 || unit == "TiB" {
                    break;
                }
                size /= 1024.0;
            }
            if unit == "B" {
                format!("{kind}  ·  {size:.0} {unit}")
            } else {
                format!("{kind}  ·  {size:.1} {unit}")
            }
        })
        .unwrap_or(kind);
    let parent = path.parent().unwrap_or(Path::new("/"));
    let location = parent
        .strip_prefix(home())
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|_| parent.display().to_string());
    (detail, location)
}

fn apply_style(colors: Colors) {
    let Ok(style) = fs::read_to_string(config_dir().join("file-picker/style.css")) else {
        return;
    };
    let definitions = [
        ("bg", colors.bg),
        ("fg", colors.fg),
        ("surface", colors.surface),
        ("selected", colors.selected),
        ("accent", colors.accent),
        ("muted", colors.muted),
        ("border", colors.border),
        ("on_accent", colors.on_accent),
    ]
    .iter()
    .map(|(name, color)| format!("@define-color fp_{name} {color};"))
    .collect::<Vec<_>>()
    .join("\n");
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&format!("{definitions}\n{style}"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn copy(app: &gtk::Application, paths: &[PathBuf], as_path: bool) {
    match copy_selection(paths, as_path) {
        Ok(()) => app.quit(),
        Err(error) => notify("File picker", &error.to_string(), true),
    }
}

fn activate(app: &gtk::Application, paths: Rc<Vec<PathBuf>>, colors: Colors) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Drag files")
        .default_width(660)
        .default_height(360)
        .decorated(false)
        .build();
    window.add_css_class("file-attachment");
    apply_style(colors);
    let container = gtk::Box::new(gtk::Orientation::Vertical, 16);
    container.set_margin_top(24);
    container.set_margin_bottom(24);
    container.set_margin_start(24);
    container.set_margin_end(24);
    window.set_child(Some(&container));

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 6);
    titles.set_hexpand(true);
    titles.append(&label("FILE ATTACHMENT", "eyebrow"));
    titles.append(&label("Ready to drop", "attachment-title"));
    titles.append(&label("Drag the card into an app.", "secondary"));
    header.append(&titles);
    let close = gtk::Button::from_icon_name("window-close-symbolic");
    close.add_css_class("close-button");
    close.set_valign(gtk::Align::Start);
    close.set_tooltip_text(Some("Close (Esc)"));
    let app_weak = app.downgrade();
    close.connect_clicked(move |_| {
        if let Some(app) = app_weak.upgrade() {
            app.quit();
        }
    });
    header.append(&close);
    container.append(&header);

    let card = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    card.add_css_class("drop-zone");
    card.set_vexpand(true);
    card.set_tooltip_text(Some(
        &paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    let icon_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    icon_box.add_css_class("file-icon");
    icon_box.set_valign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(
        if paths.len() == 1
            && paths[0]
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            "application-pdf-symbolic"
        } else {
            "text-x-generic-symbolic"
        },
    );
    icon.set_pixel_size(32);
    icon_box.append(&icon);
    card.append(&icon_box);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 6);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    let names = if paths.len() == 1 {
        paths[0]
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        format!("{} files selected", paths.len())
    };
    let heading = label(&names, "filename");
    heading.set_ellipsize(pango::EllipsizeMode::Middle);
    heading.set_max_width_chars(44);
    heading.set_single_line_mode(true);
    text.append(&heading);
    let (detail, location) = metadata(&paths);
    let detail = label(&detail, "secondary");
    detail.set_ellipsize(pango::EllipsizeMode::End);
    detail.set_single_line_mode(true);
    text.append(&detail);
    let location = label(&location, "location");
    location.set_ellipsize(pango::EllipsizeMode::Middle);
    location.set_single_line_mode(true);
    text.append(&location);
    card.append(&text);
    container.append(&card);

    let source = gtk::DragSource::new();
    source.set_actions(gdk::DragAction::COPY);
    let files: Vec<_> = paths.iter().map(gio::File::for_path).collect();
    let content = gdk::ContentProvider::new_union(&[
        gdk::ContentProvider::for_value(&gdk::FileList::from_array(&files).to_value()),
        gdk::ContentProvider::for_bytes(
            "text/uri-list",
            &glib::Bytes::from_owned(file_uris(&paths)),
        ),
    ]);
    source.set_content(Some(&content));
    let cancelled = Rc::new(Cell::new(false));
    let cancelled_begin = cancelled.clone();
    let app_weak = app.downgrade();
    source.connect_drag_begin(move |_, drag| {
        cancelled_begin.set(false);
        let cancelled = cancelled_begin.clone();
        let app_weak = app_weak.clone();
        drag.connect_dnd_finished(move |_| {
            if !cancelled.get()
                && let Some(app) = app_weak.upgrade()
            {
                glib::idle_add_local_once(move || app.quit());
            }
        });
    });
    source.connect_drag_cancel(move |_, _, _| {
        cancelled.set(true);
        false
    });
    card.add_controller(source);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let hint = label("Ctrl C  copy   ·   Esc  close", "shortcut-hint");
    hint.set_hexpand(true);
    buttons.append(&hint);
    if paths.len() == 1 {
        let button = gtk::Button::with_label("Copy path");
        let app_weak = app.downgrade();
        let selected = paths.clone();
        button.connect_clicked(move |_| {
            if let Some(app) = app_weak.upgrade() {
                copy(&app, &selected, true);
            }
        });
        buttons.append(&button);
    }
    let button = gtk::Button::with_label(if paths.len() == 1 {
        "Copy file"
    } else {
        "Copy files"
    });
    button.add_css_class("suggested-action");
    let app_weak = app.downgrade();
    let selected = paths.clone();
    button.connect_clicked(move |_| {
        if let Some(app) = app_weak.upgrade() {
            copy(&app, &selected, false);
        }
    });
    buttons.append(&button);
    container.append(&buttons);

    let keys = gtk::EventControllerKey::new();
    let app_weak = app.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let Some(app) = app_weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if key == gdk::Key::Escape {
            app.quit();
            return glib::Propagation::Stop;
        }
        if modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            && [gdk::Key::c, gdk::Key::y].contains(&key)
        {
            copy(&app, &paths, key == gdk::Key::y);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
    window.present();
}

fn run() -> Result<()> {
    let paths = Rc::new(checked_files(env::args_os().skip(1).map(PathBuf::from))?);
    let colors = appearance_colors();
    let app = gtk::Application::builder()
        .application_id("local.filepicker.drag")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| activate(app, paths.clone(), colors));
    app.run_with_args(&["file-picker-drag"]);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("file-picker: {error}");
        notify("File picker", &error.to_string(), true);
        std::process::exit(1);
    }
}
