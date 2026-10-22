use anyhow::Result;
use formula_plot::{
    model::{Plot, Viewport},
    render,
};
use gtk::{gdk, gio, glib, prelude::*};
use std::{cell::RefCell, rc::Rc};

struct State {
    plot: Plot,
    view: Viewport,
    initial: Viewport,
    pointer: Option<(f64, f64)>,
    drag: Option<Viewport>,
    save_dialog: Option<gtk::FileChooserNative>,
}

pub fn run(plot: Plot, view: Viewport, formula: String, width: u32, height: u32) -> Result<()> {
    gtk::init()?;
    let app = gtk::Application::builder()
        .application_id("org.local.FormulaPlot")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let state = Rc::new(RefCell::new(State {
            plot: plot.clone(),
            view,
            initial: view,
            pointer: None,
            drag: None,
            save_dialog: None,
        }));
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Plot")
            .default_width(width as i32)
            .default_height(height as i32 + 130)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        for (name, value) in [
            ("margin-start", 12i32),
            ("margin-end", 12),
            ("margin-top", 10),
            ("margin-bottom", 10),
        ] {
            toolbar.set_property(name, value);
        }
        let label = gtk::Label::new(Some("Curves"));
        toolbar.append(&label);
        let entry = gtk::Entry::builder()
            .text(&formula)
            .hexpand(true)
            .placeholder_text("sin(x), x^2, 2x + 1")
            .tooltip_text("Comma-separated formulas; x is the variable")
            .build();
        toolbar.append(&entry);
        let draw = gtk::Button::with_label("Plot");
        draw.add_css_class("suggested-action");
        toolbar.append(&draw);
        let reset = gtk::Button::with_label("Reset");
        reset.set_tooltip_text(Some("Restore the starting ranges (Home or double-click)"));
        toolbar.append(&reset);
        let fit = gtk::Button::with_label("Fit Y");
        fit.set_tooltip_text(Some("Fit the vertical range to the visible curves"));
        toolbar.append(&fit);
        let save = gtk::Button::with_label("Save…");
        save.set_tooltip_text(Some("Export PNG, SVG, or PDF (Ctrl+S)"));
        toolbar.append(&save);
        root.append(&toolbar);
        let warnings = gtk::Label::new(None);
        warnings.set_wrap(true);
        warnings.set_xalign(0.0);
        warnings.set_margin_start(12);
        warnings.set_margin_end(12);
        warnings.add_css_class("error");
        warnings.set_text(&plot.warnings.join("\n"));
        warnings.set_visible(!plot.warnings.is_empty());
        root.append(&warnings);
        let area = gtk::DrawingArea::builder()
            .content_width(400)
            .content_height(300)
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .build();
        area.set_cursor_from_name(Some("crosshair"));
        root.append(&area);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        footer.set_margin_start(12);
        footer.set_margin_end(12);
        footer.set_margin_top(7);
        footer.set_margin_bottom(7);
        let help = gtk::Label::new(Some("Drag to pan · Scroll to zoom · Double-click to reset"));
        help.add_css_class("dim-label");
        help.set_hexpand(true);
        help.set_xalign(0.0);
        footer.append(&help);
        let position = gtk::Label::new(Some("x, y"));
        position.add_css_class("dim-label");
        position.set_width_chars(26);
        position.set_xalign(1.0);
        footer.append(&position);
        root.append(&footer);
        window.set_child(Some(&root));
        {
            let state = state.clone();
            area.set_draw_func(move |_, context, width, height| {
                let state = state.borrow();
                if let Err(error) =
                    render::scene(&state.plot, state.view, width as f64, height as f64)
                        .draw(context)
                {
                    eprintln!("plot: could not draw graph: {error:#}");
                }
            });
        }
        {
            let state = state.clone();
            let area = area.clone();
            let entry = entry.clone();
            let warnings = warnings.clone();
            draw.connect_clicked(move |_| match Plot::parse(entry.text().as_str()) {
                Ok(plot) => {
                    warnings.set_text(&plot.warnings.join("\n"));
                    warnings.set_visible(!plot.warnings.is_empty());
                    state.borrow_mut().plot = plot;
                    area.queue_draw();
                    area.grab_focus();
                }
                Err(error) => {
                    warnings.set_text(&error.to_string());
                    warnings.set_visible(true);
                }
            });
        }
        {
            let draw = draw.clone();
            entry.connect_activate(move |_| draw.emit_clicked());
        }
        {
            let state = state.clone();
            let area = area.clone();
            reset.connect_clicked(move |_| {
                let mut state = state.borrow_mut();
                state.view = state.initial;
                area.queue_draw();
            });
        }
        {
            let state = state.clone();
            let area = area.clone();
            fit.connect_clicked(move |_| {
                let mut state = state.borrow_mut();
                state.view = state.plot.fit_y(state.view);
                area.queue_draw();
            });
        }
        {
            let state = state.clone();
            let window = window.clone();
            let area = area.clone();
            let warnings = warnings.clone();
            save.connect_clicked(move |_| {
                if let Some(dialog) = &state.borrow().save_dialog {
                    dialog.show();
                    return;
                }
                let chooser = gtk::FileChooserNative::new(
                    Some("Save plot"),
                    Some(&window),
                    gtk::FileChooserAction::Save,
                    Some("_Save"),
                    Some("_Cancel"),
                );
                chooser.set_current_name("plot.png");
                let filter = gtk::FileFilter::new();
                filter.set_name(Some("Plot images (PNG, SVG, PDF)"));
                filter.add_pattern("*.png");
                filter.add_pattern("*.svg");
                filter.add_pattern("*.pdf");
                chooser.add_filter(&filter);
                let weak_state = Rc::downgrade(&state);
                let warnings = warnings.clone();
                let width = area.width().max(400) as u32;
                let height = area.height().max(300) as u32;
                chooser.connect_response(move |chooser, response| {
                    let dialog = chooser.clone();
                    if let Some(state) = weak_state.upgrade() {
                        if response == gtk::ResponseType::Accept
                            && let Some(mut path) = chooser.file().and_then(|file| file.path())
                        {
                            if path.extension().is_none() {
                                path.set_extension("png");
                            }
                            let state = state.borrow();
                            if let Err(error) =
                                render::export(&state.plot, state.view, &path, width, height)
                            {
                                warnings.set_text(&format!("Could not save plot: {error:#}"));
                                warnings.set_visible(true);
                            }
                        }
                        state.borrow_mut().save_dialog = None;
                    }
                    dialog.destroy();
                });
                state.borrow_mut().save_dialog = Some(chooser.clone());
                chooser.set_modal(true);
                chooser.show();
            });
        }
        let motion = gtk::EventControllerMotion::new();
        {
            let state = state.clone();
            let area = area.clone();
            motion.connect_motion(move |_, x, y| {
                let mut state = state.borrow_mut();
                state.pointer = Some((x, y));
                let rect = render::layout(&state.plot, area.width() as f64, area.height() as f64);
                if rect.contains(x, y) {
                    let (x, y) = rect.coordinates(state.view, x, y);
                    position.set_text(&format!("x={x:.5}   y={y:.5}"));
                }
            });
        }
        area.add_controller(motion);
        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        {
            let state = state.clone();
            let area = area.clone();
            scroll.connect_scroll(move |_, _, dy| {
                let mut state = state.borrow_mut();
                let rect = render::layout(&state.plot, area.width() as f64, area.height() as f64);
                let (px, py) = state
                    .pointer
                    .unwrap_or((rect.x + rect.width / 2.0, rect.y + rect.height / 2.0));
                if !rect.contains(px, py) {
                    return glib::Propagation::Proceed;
                }
                let (x, y) = rect.coordinates(state.view, px, py);
                state.view = state.view.zoom((dy * 0.15).exp().clamp(0.1, 10.0), x, y);
                area.queue_draw();
                glib::Propagation::Stop
            });
        }
        area.add_controller(scroll);
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        {
            let state = state.clone();
            let area = area.clone();
            drag.connect_drag_begin(move |_, x, y| {
                let mut state = state.borrow_mut();
                let rect = render::layout(&state.plot, area.width() as f64, area.height() as f64);
                state.drag = rect.contains(x, y).then_some(state.view);
                area.grab_focus();
            });
        }
        {
            let state = state.clone();
            let area = area.clone();
            drag.connect_drag_update(move |_, dx, dy| {
                let mut state = state.borrow_mut();
                if let Some(start) = state.drag {
                    let rect =
                        render::layout(&state.plot, area.width() as f64, area.height() as f64);
                    state.view = start.pan(
                        -dx / rect.width * (start.xmax - start.xmin),
                        dy / rect.height * (start.ymax - start.ymin),
                    );
                    area.queue_draw();
                }
            });
        }
        {
            let state = state.clone();
            drag.connect_drag_end(move |_, _, _| state.borrow_mut().drag = None);
        }
        area.add_controller(drag);
        let click = gtk::GestureClick::new();
        click.set_button(1);
        {
            let state = state.clone();
            let area = area.clone();
            click.connect_pressed(move |_, count, _, _| {
                if count == 2 {
                    let mut state = state.borrow_mut();
                    state.view = state.initial;
                    area.queue_draw();
                }
            });
        }
        area.add_controller(click);
        let keys = gtk::EventControllerKey::new();
        {
            let state = state.clone();
            let area = area.clone();
            let save = save.clone();
            let window = window.clone();
            let entry = entry.clone();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                if modifiers.contains(gdk::ModifierType::CONTROL_MASK) && key == gdk::Key::s {
                    save.emit_clicked();
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::Escape {
                    window.close();
                    return glib::Propagation::Stop;
                }
                if entry.has_focus() || entry.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN)
                {
                    return glib::Propagation::Proceed;
                }
                let mut state = state.borrow_mut();
                let view = state.view;
                match key {
                    gdk::Key::Home => state.view = state.initial,
                    gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                        state.view = view.zoom(
                            0.8,
                            (view.xmin + view.xmax) / 2.0,
                            (view.ymin + view.ymax) / 2.0,
                        )
                    }
                    gdk::Key::minus | gdk::Key::KP_Subtract => {
                        state.view = view.zoom(
                            1.25,
                            (view.xmin + view.xmax) / 2.0,
                            (view.ymin + view.ymax) / 2.0,
                        )
                    }
                    gdk::Key::Left => state.view = view.pan(-(view.xmax - view.xmin) * 0.1, 0.0),
                    gdk::Key::Right => state.view = view.pan((view.xmax - view.xmin) * 0.1, 0.0),
                    gdk::Key::Up => state.view = view.pan(0.0, (view.ymax - view.ymin) * 0.1),
                    gdk::Key::Down => state.view = view.pan(0.0, -(view.ymax - view.ymin) * 0.1),
                    _ => return glib::Propagation::Proceed,
                }
                area.queue_draw();
                glib::Propagation::Stop
            });
        }
        window.add_controller(keys);
        window.present();
        area.grab_focus();
    });
    let code = app.run_with_args(&["plot"]);
    anyhow::ensure!(
        code == glib::ExitCode::SUCCESS,
        "Plot window exited with status {}",
        code.value()
    );
    Ok(())
}
