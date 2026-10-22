//! GTK stays in this executable; the shortcut/worker never loads GUI libraries.
use cairo::{Context, Operator};
use dictate::ui::View;
use std::{
    ffi::{CStr, c_void},
    io::{BufRead, BufReader, Read},
    ptr,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[link(name = "pangocairo-1.0")]
unsafe extern "C" {
    fn pango_cairo_create_layout(cr: *mut cairo::ffi::cairo_t) -> *mut pango::PangoLayout;
    fn pango_cairo_show_layout(cr: *mut cairo::ffi::cairo_t, layout: *mut pango::PangoLayout);
}
struct Incoming {
    view: View,
    revision: u64,
    alive: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct DisplayCost {
    ceiling: f64,
    decimals: i32,
    dkk: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct DisplayStamp {
    seconds: u64,
    cost: Option<DisplayCost>,
}
struct Overlay {
    incoming: Arc<Mutex<Incoming>>,
    view: View,
    revision: u64,
    bars: [f64; 42],
    window: *mut gtk::GtkWindow,
    area: *mut gtk::GtkWidget,
    main_loop: *mut glib::GMainLoop,
    layout: *mut pango::PangoLayout,
    last_text: String,
    monitor: String,
    began: Instant,
    last_display: Option<DisplayStamp>,
}
fn recording(view: &View) -> bool {
    matches!(view.phase.as_str(), "starting" | "recording")
}
fn display_stamp(view: &View, now_ms: u64) -> DisplayStamp {
    let elapsed = if recording(view) {
        now_ms.saturating_sub(view.start_ms)
    } else {
        view.recording_ms
    };
    let cost = if view.cost_rate > 0. && view.cost_rate.is_finite() {
        let usd = elapsed as f64 / 60000. * view.cost_rate;
        let dkk = view.currency.eq_ignore_ascii_case("DKK");
        let amount = if dkk { usd * view.exchange } else { usd };
        let decimals = if amount < 0.01 {
            4
        } else if amount < 1. {
            3
        } else {
            2
        };
        let ceiling = (amount * 10f64.powi(decimals)).ceil() / 10f64.powi(decimals);
        Some(DisplayCost {
            ceiling,
            decimals,
            dkk,
        })
    } else {
        None
    };
    DisplayStamp {
        seconds: elapsed / 1000,
        cost,
    }
}
impl Overlay {
    fn advance_frame(&mut self, now_ms: u64, content_changed: bool) -> bool {
        let recording = recording(&self.view);
        let mut bars_changed = false;
        if recording {
            for (i, bar) in self.bars.iter_mut().enumerate() {
                let target = self.view.bars.get(i).copied().unwrap_or(0.) as f64;
                let delta = target - *bar;
                // Stop repainting once movement is below 0.024 pixels at full bar height.
                if delta.abs() >= 0.001 {
                    let factor = if delta > 0. { 0.65 } else { 0.22 };
                    *bar += delta * factor;
                    bars_changed = true;
                }
            }
        }
        let display = display_stamp(&self.view, now_ms);
        let display_changed = self.last_display != Some(display);
        self.last_display = Some(display);
        content_changed || !recording || bars_changed || display_changed
    }
}
fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn main() {
    let mut input = BufReader::new(std::io::stdin());
    let mut first = String::new();
    if input.read_line(&mut first).is_err() {
        return;
    }
    let Ok(view) = serde_json::from_str::<View>(&first) else {
        return;
    };
    let incoming = Arc::new(Mutex::new(Incoming {
        view: view.clone(),
        revision: 1,
        alive: true,
    }));
    let updates = incoming.clone();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match input.by_ref().take(1024 * 1024).read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if let Ok(view) = serde_json::from_str::<View>(&line) {
                        let mut data = updates.lock().unwrap();
                        data.view = view;
                        data.revision += 1;
                    }
                }
            }
        }
        updates.lock().unwrap().alive = false;
    });
    // All GTK objects and callbacks stay on this thread. Callback data outlives the main loop.
    unsafe {
        if gtk::gtk_init_check() == 0 || layer::gtk_layer_is_supported() == 0 {
            eprintln!("Dictate overlay needs a Wayland compositor with layer-shell");
            return;
        }
        let window = gtk::gtk_window_new() as *mut gtk::GtkWindow;
        gtk::gtk_window_set_title(window, c"Dictate".as_ptr());
        gtk::gtk_window_set_decorated(window, 0);
        gtk::gtk_window_set_resizable(window, 0);
        gtk::gtk_widget_set_focusable(window.cast(), 0);
        gtk::gtk_widget_set_can_target(window.cast(), 0);
        let height = if view.live { 204 } else { 76 };
        gtk::gtk_window_set_default_size(window, 430, height);
        layer::gtk_layer_init_for_window(window);
        layer::gtk_layer_set_namespace(window, c"dictate".as_ptr());
        layer::gtk_layer_set_keyboard_mode(window, layer::GTK_LAYER_SHELL_KEYBOARD_MODE_NONE);
        layer::gtk_layer_set_layer(window, layer::GTK_LAYER_SHELL_LAYER_OVERLAY);
        layer::gtk_layer_set_anchor(window, layer::GTK_LAYER_SHELL_EDGE_BOTTOM, 1);
        layer::gtk_layer_set_margin(window, layer::GTK_LAYER_SHELL_EDGE_BOTTOM, 86);
        layer::gtk_layer_set_exclusive_zone(window, -1);
        let provider = gtk::gtk_css_provider_new();
        let css = c"window { background: transparent; } drawingarea { background: transparent; }";
        gtk::gtk_css_provider_load_from_data(provider, css.as_ptr(), -1);
        let display = gdk::gdk_display_get_default();
        gtk::gtk_style_context_add_provider_for_display(display, provider.cast(), 600);
        gobject::g_object_unref(provider.cast());
        let area = gtk::gtk_drawing_area_new();
        gtk::gtk_drawing_area_set_content_width(area.cast(), 430);
        gtk::gtk_drawing_area_set_content_height(area.cast(), height);
        gtk::gtk_window_set_child(window, area);
        let main_loop = glib::g_main_loop_new(ptr::null_mut(), 0);
        let mut overlay = Box::new(Overlay {
            incoming,
            view,
            revision: 0,
            bars: [0.; 42],
            window,
            area,
            main_loop,
            layout: ptr::null_mut(),
            last_text: String::new(),
            monitor: String::new(),
            began: Instant::now(),
            last_display: None,
        });
        let data = (&mut *overlay) as *mut Overlay as *mut c_void;
        gtk::gtk_drawing_area_set_draw_func(area.cast(), Some(draw), data, None);
        glib::g_timeout_add(16, Some(tick), data);
        gtk::gtk_window_present(window);
        let surface = gtk::gtk_native_get_surface(gtk::gtk_widget_get_native(window.cast()));
        if !surface.is_null() {
            let region = cairo::Region::create();
            gdk::gdk_surface_set_input_region(surface, region.to_raw_none());
        }
        glib::g_main_loop_run(main_loop);
        gtk::gtk_drawing_area_set_draw_func(area.cast(), None, ptr::null_mut(), None);
        gtk::gtk_window_destroy(window);
        if !overlay.layout.is_null() {
            gobject::g_object_unref(overlay.layout.cast());
        }
        glib::g_main_loop_unref(main_loop);
    }
}
unsafe extern "C" fn tick(data: *mut c_void) -> i32 {
    // No panicking operations or borrowed GTK values cross this FFI boundary.
    let o = unsafe { &mut *(data as *mut Overlay) };
    let Ok(data) = o.incoming.lock() else {
        return 0;
    };
    if !data.alive || data.view.phase == "finished" {
        unsafe {
            glib::g_main_loop_quit(o.main_loop);
        }
        return 0;
    }
    let mut content_changed = false;
    if data.revision != o.revision {
        content_changed = o.view.phase != data.view.phase
            || o.view.live != data.view.live
            || o.view.transcript != data.view.transcript
            || o.view.monitor != data.view.monitor;
        o.view = data.view.clone();
        o.revision = data.revision;
    }
    drop(data);
    if o.monitor != o.view.monitor && !o.view.monitor.is_empty() {
        unsafe {
            let monitors = gdk::gdk_display_get_monitors(gdk::gdk_display_get_default());
            for i in 0..gio::g_list_model_get_n_items(monitors) {
                let monitor = gio::g_list_model_get_item(monitors, i) as *mut gdk::GdkMonitor;
                let name = gdk::gdk_monitor_get_connector(monitor);
                if !name.is_null() && CStr::from_ptr(name).to_bytes() == o.view.monitor.as_bytes() {
                    layer::gtk_layer_set_monitor(o.window, monitor);
                    o.monitor = o.view.monitor.clone();
                }
                gobject::g_object_unref(monitor.cast());
            }
        }
    }
    if o.advance_frame(epoch_ms(), content_changed) {
        unsafe {
            gtk::gtk_widget_queue_draw(o.area);
        }
    }
    1
}
fn rounded(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.);
    cr.arc(x + w - r, y + h - r, r, 0., FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    cr.arc(x + r, y + r, r, PI, PI + FRAC_PI_2);
    cr.close_path();
}
fn label(cr: &Context, text: &str, x: f64, y: f64, size: f64, color: (f64, f64, f64)) {
    unsafe {
        let layout = pango_cairo_create_layout(cr.to_raw_none());
        if layout.is_null() {
            return;
        }
        pango::pango_layout_set_text(
            layout,
            text.as_ptr().cast(),
            text.len().min(i32::MAX as usize) as i32,
        );
        let font = pango::pango_font_description_from_string(c"monospace".as_ptr());
        pango::pango_font_description_set_absolute_size(font, size * 1024.);
        pango::pango_layout_set_font_description(layout, font);
        pango::pango_font_description_free(font);
        cr.set_source_rgb(color.0, color.1, color.2);
        cr.move_to(x, y);
        pango_cairo_show_layout(cr.to_raw_none(), layout);
        gobject::g_object_unref(layout.cast());
    }
}
unsafe extern "C" fn draw(
    _area: *mut gtk::GtkDrawingArea,
    raw: *mut cairo::ffi::cairo_t,
    width: i32,
    height: i32,
    data: *mut c_void,
) {
    let o = unsafe { &mut *(data as *mut Overlay) };
    let cr = unsafe { Context::from_raw_none(raw) };
    cr.set_operator(Operator::Source);
    cr.set_source_rgba(0., 0., 0., 0.);
    let _ = cr.paint();
    cr.set_operator(Operator::Over);
    rounded(&cr, 0.5, 0.5, width as f64 - 1., height as f64 - 1., 10.);
    cr.set_source_rgba(0.004, 0.004, 0.016, 1.);
    let _ = cr.fill_preserve();
    cr.set_source_rgb(0.094, 0.835, 1.);
    cr.set_line_width(1.);
    let _ = cr.stroke();
    let recording = recording(&o.view);
    if o.view.live {
        label(
            &cr,
            if recording {
                "LIVE TRANSCRIPT"
            } else {
                "FINISHING…"
            },
            18.,
            12.,
            11.,
            (0.4, 0.88, 1.),
        );
        let text = if o.view.transcript.is_empty() {
            if recording {
                "Listening…"
            } else {
                "Waiting for final transcript…"
            }
        } else {
            &o.view.transcript
        };
        unsafe {
            if o.layout.is_null() {
                o.layout = pango_cairo_create_layout(raw);
                let font = pango::pango_font_description_from_string(c"monospace".as_ptr());
                pango::pango_font_description_set_absolute_size(font, 12. * 1024.);
                pango::pango_layout_set_font_description(o.layout, font);
                pango::pango_font_description_free(font);
                pango::pango_layout_set_width(o.layout, (width - 36) * 1024);
                pango::pango_layout_set_wrap(o.layout, pango::PANGO_WRAP_WORD_CHAR);
            }
            let start = text
                .char_indices()
                .rev()
                .nth(3999)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let text = &text[start..];
            if text != o.last_text {
                pango::pango_layout_set_text(o.layout, text.as_ptr().cast(), text.len() as i32);
                o.last_text = text.to_owned();
            }
            let mut h = 0;
            pango::pango_layout_get_pixel_size(o.layout, ptr::null_mut(), &mut h);
            let _ = cr.save();
            cr.rectangle(18., 34., (width - 36) as f64, 82.);
            cr.clip();
            cr.set_source_rgb(0.75, 0.76, 0.87);
            cr.move_to(18., 34. - (h - 82).max(0) as f64);
            pango_cairo_show_layout(raw, o.layout);
            let _ = cr.restore();
        }
        cr.set_source_rgb(0.15, 0.19, 0.25);
        cr.move_to(18., 128.5);
        cr.line_to((width - 18) as f64, 128.5);
        let _ = cr.stroke();
    }
    let y = height as f64 - 38.;
    cr.set_source_rgb(
        if recording { 0.95 } else { 0.09 },
        if recording { 0.19 } else { 0.7 },
        if recording { 0.31 } else { 0.36 },
    );
    cr.arc(26., y, 7., 0., std::f64::consts::TAU);
    let _ = cr.fill();
    let spacing = (width as f64 - 108.) / 42.;
    for i in 0..42 {
        let value = if recording {
            o.bars[i]
        } else {
            let distance = (i as f64 - (o.began.elapsed().as_secs_f64() * 22.) % 54. + 6.).abs();
            ((6. - distance) / 6.).clamp(0., 1.)
        };
        let h = 8. + value * 24.;
        cr.set_source_rgb(0.69, 0.7, 0.83);
        cr.rectangle(
            49. + i as f64 * spacing,
            y - h / 2.,
            (spacing - 2.).max(1.),
            h,
        );
        let _ = cr.fill();
    }
    let display = display_stamp(&o.view, epoch_ms());
    let seconds = display.seconds;
    label(
        &cr,
        &format!("{:02}:{:02}", seconds / 60, seconds % 60),
        width as f64 - 45.,
        height as f64 - 23.,
        10.,
        (0.69, 0.7, 0.83),
    );
    if let Some(DisplayCost {
        ceiling,
        decimals,
        dkk,
    }) = display.cost
    {
        let cost = if dkk {
            format!("≤{ceiling:.precision$} kr", precision = decimals as usize).replace('.', ",")
        } else {
            format!("≤${ceiling:.precision$}", precision = decimals as usize)
        };
        label(&cr, &cost, 18., height as f64 - 23., 10., (0.69, 0.7, 0.83));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[link(name = "cairo")]
    unsafe extern "C" {
        fn cairo_surface_write_to_png(
            surface: *mut cairo::ffi::cairo_surface_t,
            filename: *const std::ffi::c_char,
        ) -> i32;
    }

    fn frame_overlay(view: View) -> Overlay {
        Overlay {
            incoming: Arc::new(Mutex::new(Incoming {
                view: view.clone(),
                revision: 1,
                alive: true,
            })),
            view,
            revision: 1,
            bars: [0.; 42],
            window: ptr::null_mut(),
            area: ptr::null_mut(),
            main_loop: ptr::null_mut(),
            layout: ptr::null_mut(),
            last_text: String::new(),
            monitor: String::new(),
            began: Instant::now(),
            last_display: None,
        }
    }

    #[test]
    fn settled_recording_redraws_only_when_visible_time_changes() {
        let mut overlay = frame_overlay(View {
            phase: "recording".into(),
            start_ms: 1000,
            ..Default::default()
        });
        let redraws = (1000..11000)
            .step_by(16)
            .filter(|&now| overlay.advance_frame(now, false))
            .count();
        assert_eq!(redraws, 10, "625 input polls need only ten timer repaints");
        assert!(
            overlay.advance_frame(10984, true),
            "text updates repaint immediately"
        );
    }

    #[test]
    fn recording_waveform_settles_and_resumes_for_new_audio() {
        let mut overlay = frame_overlay(View {
            bars: vec![0.5; 42],
            ..Default::default()
        });
        assert!(overlay.advance_frame(0, false));
        for _ in 0..40 {
            overlay.advance_frame(0, false);
        }
        assert!(!overlay.advance_frame(0, false));
        assert!(overlay.bars.iter().all(|&bar| (0.5 - bar).abs() < 0.001));
        overlay.view.bars.fill(0.);
        assert!(overlay.advance_frame(0, false));
        for _ in 0..40 {
            overlay.advance_frame(0, false);
        }
        assert!(!overlay.advance_frame(0, false));
        assert!(overlay.bars.iter().all(|&bar| bar < 0.001));
    }

    #[test]
    fn cost_changes_and_finishing_animation_continue_to_redraw() {
        let mut overlay = frame_overlay(View {
            cost_rate: 0.006,
            currency: "USD".into(),
            ..Default::default()
        });
        assert!(overlay.advance_frame(0, false));
        assert!(overlay.advance_frame(16, false));
        assert!(!overlay.advance_frame(32, false));
        assert!(overlay.advance_frame(1001, false));
        assert_eq!(overlay.last_display.unwrap().cost.unwrap().ceiling, 0.0002);
        overlay.view.phase = "processing".into();
        assert!(overlay.advance_frame(1001, false));
        assert!(overlay.advance_frame(1017, false));
    }

    #[test]
    fn long_transcript_reuses_layout_until_visible_tail_changes() {
        let tail = "∇é🙂 ".repeat(1000);
        let view = View {
            transcript: format!("Earlier context: {tail}"),
            ..Default::default()
        };
        let mut overlay = Overlay {
            incoming: Arc::new(Mutex::new(Incoming {
                view: view.clone(),
                revision: 1,
                alive: true,
            })),
            view,
            revision: 1,
            bars: [0.; 42],
            window: ptr::null_mut(),
            area: ptr::null_mut(),
            main_loop: ptr::null_mut(),
            layout: ptr::null_mut(),
            last_text: String::new(),
            monitor: String::new(),
            began: Instant::now(),
            last_display: None,
        };
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 430, 204).unwrap();
        let context = Context::new(&surface).unwrap();
        let render = |overlay: &mut Overlay| unsafe {
            draw(
                ptr::null_mut(),
                context.to_raw_none(),
                430,
                204,
                (overlay as *mut Overlay).cast(),
            );
        };
        render(&mut overlay);
        assert_eq!(
            overlay.last_text, tail,
            "tail must retain complete UTF-8 characters"
        );
        let serial = unsafe { pango::pango_layout_get_serial(overlay.layout) };
        render(&mut overlay);
        assert_eq!(
            unsafe { pango::pango_layout_get_serial(overlay.layout) },
            serial
        );
        overlay.view.transcript = format!("Corrected earlier context: {tail}");
        render(&mut overlay);
        assert_eq!(
            unsafe { pango::pango_layout_get_serial(overlay.layout) },
            serial
        );
        overlay.view.transcript.push('β');
        render(&mut overlay);
        assert_ne!(
            unsafe { pango::pango_layout_get_serial(overlay.layout) },
            serial
        );
        assert_eq!(overlay.last_text.chars().count(), 4000);
        assert!(overlay.last_text.ends_with(" β"));
        unsafe {
            gobject::g_object_unref(overlay.layout.cast());
        }
    }

    #[test]
    fn native_frame_has_rounded_cyan_border_and_readable_content() {
        let view=View{phase:"recording".into(),transcript:"Testing the native Rust dictation overlay.\nPyTorch · nnU-Net · 1e-4\nEnglish and dansk.".into(),bars:(0..42).map(|i|0.25+0.45*(i as f32*0.5).sin().abs()).collect(),start_ms:epoch_ms().saturating_sub(1200),..Default::default()};
        let incoming = Arc::new(Mutex::new(Incoming {
            view: view.clone(),
            revision: 1,
            alive: true,
        }));
        let mut overlay = Overlay {
            incoming,
            view,
            revision: 1,
            bars: [0.5; 42],
            window: ptr::null_mut(),
            area: ptr::null_mut(),
            main_loop: ptr::null_mut(),
            layout: ptr::null_mut(),
            last_text: String::new(),
            monitor: String::new(),
            began: Instant::now(),
            last_display: None,
        };
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 430, 204).unwrap();
        {
            let context = Context::new(&surface).unwrap();
            unsafe {
                draw(
                    ptr::null_mut(),
                    context.to_raw_none(),
                    430,
                    204,
                    (&mut overlay as *mut Overlay).cast(),
                );
            }
        }
        if let Some(path) = std::env::var_os("DICTATE_TEST_SNAPSHOT") {
            let path = std::ffi::CString::new(path.as_encoded_bytes()).unwrap();
            assert_eq!(
                unsafe { cairo_surface_write_to_png(surface.to_raw_none(), path.as_ptr()) },
                0
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let pixels = surface.data().unwrap();
        assert_eq!(pixels[3], 0, "rounded outer corner must be transparent");
        let top = 215 * 4;
        assert!(
            pixels[top] > 200 && pixels[top + 1] > 150 && pixels[top + 2] < 60,
            "top border must be cyan"
        );
        let inside = 120 * stride + 215 * 4;
        assert!(
            pixels[inside] < 10
                && pixels[inside + 1] < 10
                && pixels[inside + 2] < 10
                && pixels[inside + 3] == 255,
            "inside must be opaque black"
        );
        let mut text_pixels = 0;
        for y in 34..116 {
            for x in 18..412 {
                let i = y * stride + x * 4;
                if pixels[i] > 60 && pixels[i + 1] > 60 && pixels[i + 2] > 60 {
                    text_pixels += 1;
                }
            }
        }
        assert!(text_pixels > 300, "transcript text must be visible");
        drop(pixels);
        unsafe {
            gobject::g_object_unref(overlay.layout.cast());
        }
    }
}
