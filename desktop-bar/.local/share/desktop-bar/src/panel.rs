use crate::{
    anim::Spring,
    draw,
    layout::{Module, Rect},
    modules::{Data, Update, clock, tray::MenuEntry},
    text::Text,
    theme::Theme,
};
#[derive(Clone, Debug)]
pub enum Kind {
    Shell(Box<crate::shell::Model>),
    Overview(crate::overview::Overview),
    Prompt(crate::prompt::Model),
    Help(crate::help::Model),
    Volume,
    Calendar,
    Battery,
    Network,
    System,
    Menu {
        address: String,
        parent: i32,
        parents: Vec<i32>,
        entries: Vec<MenuEntry>,
    },
    Tooltip(String),
}
impl Kind {
    pub fn observes(&self, update: &Update) -> bool {
        match self {
            Self::Shell(model) => {
                model.page == crate::shell::Page::Controls
                    && matches!(
                        update,
                        Update::Cpu(_)
                            | Update::Memory(_)
                            | Update::Disk(_)
                            | Update::Media(_)
                            | Update::MediaArt(..)
                    )
            }
            Self::Overview(_) => matches!(
                update,
                Update::Battery(_)
                    | Update::Network(_)
                    | Update::NetworkSignals(_)
                    | Update::Media(_)
                    | Update::MediaArt(..)
                    | Update::Tray(_)
            ),
            Self::Volume => matches!(update, Update::Audio(..)),
            Self::Battery => matches!(update, Update::Battery(_)),
            Self::Network => matches!(update, Update::Network(_) | Update::NetworkSignals(_)),
            Self::System => matches!(
                update,
                Update::Cpu(_) | Update::Memory(_) | Update::Disk(_) | Update::Weather(_)
            ),
            Self::Menu { .. } => matches!(update, Update::Tray(_)),
            Self::Calendar | Self::Prompt(_) | Self::Help(_) | Self::Tooltip(_) => false,
        }
    }
    pub fn uses_clock(&self) -> bool {
        matches!(self, Self::Calendar | Self::Overview(_))
            || matches!(self, Self::Shell(model) if model.page == crate::shell::Page::Controls)
    }
}
#[derive(Clone, Debug)]
pub struct Panel {
    pub kind: Kind,
    pub growth: Spring,
    pub closing: bool,
    pub month: i32,
    pub hover: Option<usize>,
    pub scroll: usize,
    pub origin: String,
}
impl Panel {
    pub fn new(kind: Kind) -> Self {
        let mut growth = Spring::new(0.0, 24.0, 0.82);
        growth.target = 1.0;
        Self {
            kind,
            growth,
            closing: false,
            month: 0,
            hover: None,
            scroll: 0,
            origin: String::new(),
        }
    }
    pub fn size(&self) -> (u32, u32) {
        if let Kind::Shell(model) = &self.kind {
            return model.size();
        }
        if let Kind::Help(model) = &self.kind {
            return model.size();
        }
        if matches!(self.kind, Kind::Prompt(_)) {
            return (480, 150);
        }
        if matches!(self.kind, Kind::Overview(_)) {
            return (640, 128);
        }
        (
            320,
            match &self.kind {
                Kind::Shell(_) | Kind::Overview(_) | Kind::Prompt(_) | Kind::Help(_) => {
                    unreachable!()
                }
                Kind::Volume => 174,
                Kind::Calendar => 310,
                Kind::Battery => 144,
                Kind::Network => 184,
                Kind::System => 288,
                Kind::Menu { entries, .. } => (entries.len().min(16) as u32 + 1) * 28 + 24,
                Kind::Tooltip(text) => 40 + text.lines().count().min(10) as u32 * 20,
            },
        )
    }
    pub fn close(&mut self) {
        if let Kind::Prompt(model) = &self.kind {
            let _ = model.request.response.try_send(None);
        }
        self.closing = true;
        self.growth.target = 0.0;
    }
    pub fn animated_size(&self, origin_width: f32) -> (f32, f32) {
        let (w, h) = self.size();
        let g = self.growth.value.clamp(0.0, 1.0);
        (
            origin_width + (w as f32 - origin_width) * g,
            28.0 + (h as f32 - 28.0) * g,
        )
    }
    pub fn row(&self, y: f32) -> Option<usize> {
        let n = ((y - 48.0) / 28.0).floor();
        (n >= 0.0).then_some(n as usize + self.scroll)
    }
    pub fn slider(&self, x: f32, y: f32) -> Option<(Module, u32)> {
        if self.growth.value < 0.92
            || !matches!(self.kind, Kind::Volume)
            || !(24.0..=296.0).contains(&x)
        {
            return None;
        }
        let module = if (57.0..90.0).contains(&y) {
            Module::Volume
        } else if (111.0..148.0).contains(&y) {
            Module::Microphone
        } else {
            return None;
        };
        Some((
            module,
            ((x - 24.0) / 272.0 * 100.0).round().clamp(0.0, 100.0) as u32,
        ))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_shell(
        &self,
        p: &mut tiny_skia::Pixmap,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        s: f32,
        dimensions: (f32, f32),
        cached: &mut Option<tiny_skia::Pixmap>,
        stale: &mut bool,
        clip_mask: &mut Option<tiny_skia::Mask>,
        painted: &mut Option<Rect>,
    ) -> Rect {
        let body = Rect::new(
            (p.width() as f32 - dimensions.0 * s) / 2.0 + 16.0 * s,
            if t.motion() {
                0.0
            } else {
                (p.height() as f32 - (dimensions.1 - 16.0) * s) / 2.0
            },
            ((dimensions.0 - 32.0) * s).max(s),
            ((dimensions.1 - 16.0) * s).max(s),
        );
        let background = if t.light {
            crate::theme::Color::hex("#f8fafc")
        } else {
            crate::theme::Color::hex("#000000")
        };
        let radius = (8.0 + 12.0 * crate::anim::ease(self.growth.value)) * s;
        // Everything below is clipped to the body or is its shadow, so only
        // the previous and current footprints need clearing and damage.
        let bounds = if t.motion() {
            Rect::new(
                body.x - 10.0 * s - 2.0,
                0.0,
                body.w + 20.0 * s + 4.0,
                body.y + body.h + 15.0 * s + 2.0,
            )
        } else {
            Rect::new(body.x - 2.0, body.y - 2.0, body.w + 4.0, body.h + 4.0)
        };
        let full = Rect::new(0.0, 0.0, p.width() as f32, p.height() as f32);
        let damage = painted.map_or(full, |previous| previous.union(bounds).clip(full));
        *painted = Some(bounds);
        let rows = damage.rows(p.height());
        let stride = p.width() as usize * 4;
        p.data_mut()[rows.start * stride..rows.end * stride].fill(0);
        if t.motion() {
            draw::notch(p, body, s, background);
        } else {
            draw::rounded(p, body, radius, background);
        }
        let (w, h) = self.size();
        let opacity = crate::anim::ease((self.growth.value - 0.12) / 0.78);
        if cached.is_none() || *stale {
            let size = (
                (w as f32 * s).round().max(1.0) as u32,
                (h as f32 * s).round().max(1.0) as u32,
            );
            // Reuse the allocation while the size holds; scrolling galleries
            // redraw content on every frame.
            match cached {
                Some(content) if (content.width(), content.height()) == size => {
                    content.fill(tiny_skia::Color::TRANSPARENT);
                }
                _ => {
                    let Some(content) = tiny_skia::Pixmap::new(size.0, size.1) else {
                        *cached = None;
                        return damage;
                    };
                    *cached = Some(content);
                }
            }
            self.draw_content(cached.as_mut().unwrap(), text, data, t, s);
            *stale = false;
        }
        let content = cached.as_ref().unwrap();
        if clip_mask
            .as_ref()
            .is_none_or(|mask| mask.width() != p.width() || mask.height() != p.height())
        {
            *clip_mask = tiny_skia::Mask::new(p.width(), p.height());
        }
        let mask = clip_mask.as_mut().unwrap();
        let stride = mask.width() as usize;
        mask.data_mut()[rows.start * stride..rows.end * stride].fill(0);
        let path = if t.motion() {
            draw::notch_path(body, 10.0 * s, 20.0 * s)
        } else {
            draw::rounded_path(body, radius)
        };
        if let Some(path) = path {
            mask.fill_path(
                &path,
                tiny_skia::FillRule::Winding,
                true,
                tiny_skia::Transform::identity(),
            );
        }
        let transform = tiny_skia::Transform::from_translate(
            (p.width() as f32 - content.width() as f32) / 2.0,
            (if t.motion() {
                0.0
            } else {
                (p.height() as f32 - content.height() as f32) / 2.0
            }) + (1.0 - opacity) * -8.0 * s,
        );
        // A transparent source leaves the destination unchanged.
        if opacity > 0.0 {
            p.draw_pixmap(
                0,
                0,
                content.as_ref(),
                &tiny_skia::PixmapPaint {
                    opacity,
                    ..Default::default()
                },
                transform,
                Some(mask),
            );
        }
        damage
    }
    pub fn draw(&self, p: &mut tiny_skia::Pixmap, text: &mut Text, data: &Data, t: &Theme, s: f32) {
        p.fill(tiny_skia::Color::TRANSPARENT);
        let growth = self.growth.value.clamp(0.0, 1.0);
        let r = Rect::new(0.0, 0.0, p.width() as f32, p.height() as f32);
        if !t.motion() {
            draw::rounded(p, r, 20.0 * s, t.accent().alpha(0.32));
        }
        draw::rounded(
            p,
            r.inset(s),
            19.0 * s,
            if t.motion() && !t.light {
                crate::theme::Color::hex("#000000")
            } else {
                t.color("background", if t.light { "#eff1f5" } else { "#18171f" })
            },
        );
        if growth < 0.92 && !t.motion() {
            if !self.origin.is_empty() {
                let width = text.width(&self.origin, 13.0 * s);
                text.draw(
                    p,
                    &self.origin,
                    (r.w - width) / 2.0,
                    22.0 * s,
                    13.0 * s,
                    t.text().alpha((1.0 - growth * 2.0).max(0.0)),
                );
            }
            return;
        }
        self.draw_content(p, text, data, t, s);
    }
    fn draw_content(
        &self,
        p: &mut tiny_skia::Pixmap,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        s: f32,
    ) {
        let mut line = |p: &mut tiny_skia::Pixmap, label: &str, x: f32, y: f32, color| {
            text.draw(p, label, x * s, y * s, 13.0 * s, color)
        };
        match &self.kind {
            Kind::Shell(model) => model.draw(p, text, data, t, s, 1.0),
            Kind::Overview(model) => model.draw(p, text, data, t, s),
            Kind::Prompt(model) => model.draw(p, text, t, s),
            Kind::Help(model) => model.draw(p, text, t, s),
            Kind::Volume => {
                line(p, "Sound", 24.0, 28.0, t.text());
                for (a, label, y) in [
                    (&data.audio, "Output", 62.0),
                    (&data.microphone, "Microphone", 118.0),
                ] {
                    line(
                        p,
                        &format!(
                            "{label}    {}%{}",
                            a.volume,
                            if a.muted { " · muted" } else { "" }
                        ),
                        24.0,
                        y - 12.0,
                        t.dim(),
                    );
                    draw::rounded(
                        p,
                        Rect::new(24.0, y + 5.0, 272.0, 8.0).scale(s),
                        4.0 * s,
                        t.text().alpha(0.15),
                    );
                    draw::rounded(
                        p,
                        Rect::new(24.0, y + 5.0, 272.0 * a.volume.min(100) as f32 / 100.0, 8.0)
                            .scale(s),
                        4.0 * s,
                        t.blue(),
                    );
                }
                draw::rounded(
                    p,
                    Rect::new(24.0, 149.0, 272.0, 22.0).scale(s),
                    6.0 * s,
                    t.text().alpha(0.07),
                );
                line(p, "Audio devices  ›", 34.0, 164.0, t.dim());
            }
            Kind::Calendar => {
                let (title, cells) = clock::calendar(self.month);
                line(p, &format!("‹   {title}   ›"), 20.0, 30.0, t.text());
                for (i, day) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"]
                    .iter()
                    .enumerate()
                {
                    line(p, day, 22.0 + i as f32 * 42.0, 64.0, t.dim());
                }
                for (i, (day, today)) in cells.iter().enumerate() {
                    let x = 22.0 + (i % 7) as f32 * 42.0;
                    let y = 100.0 + (i / 7) as f32 * 34.0;
                    if *today {
                        draw::rounded(
                            p,
                            Rect::new(x - 7.0, y - 19.0, 32.0, 28.0).scale(s),
                            14.0 * s,
                            t.accent(),
                        );
                    }
                    line(
                        p,
                        day,
                        x,
                        y,
                        if *today {
                            crate::theme::Color::hex("#ffffff")
                        } else {
                            t.text()
                        },
                    );
                }
            }
            Kind::Battery => {
                line(p, "Battery", 24.0, 30.0, t.text());
                if let Some(b) = &data.battery {
                    line(
                        p,
                        &format!(
                            "{:.0}% · {}",
                            b.percent,
                            if b.charging {
                                "Charging"
                            } else if b.plugged {
                                "Plugged in"
                            } else {
                                "Discharging"
                            }
                        ),
                        24.0,
                        62.0,
                        t.green(),
                    );
                    line(p, &format!("Power  {:.1} W", b.watts), 24.0, 90.0, t.dim());
                    line(
                        p,
                        &if b.hours > 0.0 {
                            format!(
                                "{}h {:02}m {}",
                                b.hours as u32,
                                ((b.hours.fract()) * 60.0) as u32,
                                if b.charging {
                                    "until full"
                                } else {
                                    "remaining"
                                }
                            )
                        } else {
                            "Time estimate unavailable".into()
                        },
                        24.0,
                        118.0,
                        t.dim(),
                    );
                }
            }
            Kind::Network => {
                line(p, "Wi-Fi", 24.0, 30.0, t.text());
                let name: String = data.network.name.chars().take(32).collect();
                line(
                    p,
                    if name.is_empty() {
                        data.network.status()
                    } else {
                        &name
                    },
                    24.0,
                    62.0,
                    t.text(),
                );
                line(
                    p,
                    &format!("{}  {}", data.network.status(), data.network.interface),
                    24.0,
                    88.0,
                    t.dim(),
                );
                let signal = data
                    .network
                    .rssi
                    .map(|rssi| format!("Signal {rssi} dBm"))
                    .unwrap_or_else(|| "Signal unavailable".into());
                line(p, &signal, 24.0, 112.0, t.dim());
                draw::rounded(
                    p,
                    Rect::new(24.0, 132.0, 272.0, 32.0).scale(s),
                    9.0 * s,
                    t.accent().alpha(0.16),
                );
                line(p, "Open network settings  ↗", 36.0, 153.0, t.text());
            }
            Kind::System => {
                line(p, "System", 24.0, 30.0, t.text());
                let cpu = data.cpu.first().copied().unwrap_or(0.0);
                line(p, &format!("CPU    {cpu:.0}%"), 24.0, 67.0, t.text());
                if let Some(memory) = &data.memory {
                    line(
                        p,
                        &format!("Memory {:.1} / {:.0} GiB", memory.used, memory.total),
                        24.0,
                        101.0,
                        t.text(),
                    );
                    line(
                        p,
                        &format!(
                            "Swap   {:.1} / {:.0} GiB",
                            memory.swap_used, memory.swap_total
                        ),
                        24.0,
                        131.0,
                        t.dim(),
                    );
                }
                if let Some(disk) = &data.disk {
                    line(
                        p,
                        &format!("Home   {:.0}G free", disk.free),
                        24.0,
                        168.0,
                        t.text(),
                    );
                }
                let weather = tooltip(Module::Weather, data);
                for (i, row) in weather.lines().take(3).enumerate() {
                    let row: String = row.chars().take(34).collect();
                    line(p, &row, 24.0, 214.0 + i as f32 * 22.0, t.dim());
                }
            }
            Kind::Menu {
                entries, parents, ..
            } => {
                line(
                    p,
                    if parents.is_empty() {
                        "Menu"
                    } else {
                        "‹ Back"
                    },
                    20.0,
                    30.0,
                    t.dim(),
                );
                for (i, entry) in entries.iter().enumerate().skip(self.scroll).take(16) {
                    let y = 48.0 + (i - self.scroll) as f32 * 28.0;
                    if entry.separator {
                        draw::rounded(
                            p,
                            Rect::new(16.0, y + 12.0, 288.0, 1.0).scale(s),
                            0.0,
                            t.text().alpha(0.15),
                        );
                        continue;
                    }
                    if self.hover == Some(i) {
                        draw::rounded(
                            p,
                            Rect::new(8.0, y - 2.0, 304.0, 28.0).scale(s),
                            8.0 * s,
                            t.accent().alpha(0.20),
                        );
                    }
                    let prefix = match entry.toggle {
                        Some(true) => "✓ ",
                        Some(false) => "○ ",
                        None => "",
                    };
                    let label = format!(
                        "{prefix}{}{}",
                        entry.label,
                        if entry.submenu { "  ›" } else { "" }
                    );
                    let label: String = label.chars().take(35).collect();
                    line(
                        p,
                        &label,
                        20.0,
                        y + 17.0,
                        if entry.enabled { t.text() } else { t.dim() },
                    );
                }
            }
            Kind::Tooltip(label) => {
                for (i, label) in label.lines().take(10).enumerate() {
                    let label: String = label.chars().take(38).collect();
                    line(p, &label, 18.0, 28.0 + i as f32 * 20.0, t.text());
                }
            }
        }
    }
}

pub fn tooltip(module: Module, data: &Data) -> String {
    match module {
        Module::Network => format!(
            "{}\n{}\nClick: network settings",
            data.network.name,
            data.network.status()
        ),
        Module::Shell => "Desktop controls · Right click: bar style".into(),
        Module::Cpu => format!(
            "CPU: {:.0}%\n{} logical cores\nClick: Mission Center",
            data.cpu.first().copied().unwrap_or(0.0),
            data.cpu.len().saturating_sub(1)
        ),
        Module::Memory => data
            .memory
            .as_ref()
            .map(|m| {
                format!(
                    "Memory: {:.1} / {:.1} GiB\nSwap: {:.1} / {:.1} GiB",
                    m.used, m.total, m.swap_used, m.swap_total
                )
            })
            .unwrap_or_default(),
        Module::Disk => data
            .disk
            .as_ref()
            .map(|d| format!("/home: {:.0}G free of {:.0}G", d.free, d.total))
            .unwrap_or_default(),
        Module::Volume => format!(
            "{}\nClick: mixer · Right click: mute\nScroll: volume ±10%",
            data.audio.description
        ),
        Module::Microphone => format!(
            "{}\nClick: mute · Scroll: volume ±5%",
            data.microphone.description
        ),
        Module::Weather => data
            .weather
            .observation
            .as_ref()
            .map(|w| {
                format!(
                    "{}\n{:.0}°C · Wind {:.1} km/h{}",
                    w.description,
                    w.temperature,
                    w.wind_kmh,
                    if data.weather.stale {
                        "\nCached reading; refresh unavailable"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|| data.weather.reason.clone()),
        Module::Clock => clock::format("%A %d %B %Y"),
        Module::Battery => data
            .battery
            .as_ref()
            .map(|b| format!("{:.0}% · {:.1} W", b.percent, b.watts))
            .unwrap_or_default(),
        Module::Tray(i) => data
            .tray
            .get(i)
            .map(|i| i.title.clone())
            .unwrap_or_default(),
        Module::Workspace(n) => format!("Workspace {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn morph_frames_clearing_only_their_footprint_match_full_redraws() {
        for style in ["native-motion", "native-clean"] {
            let t = Theme {
                style: style.into(),
                ..Theme::default()
            };
            let mut text = Text::new(&t.font).unwrap();
            let data = crate::sample_data();
            let mut panel = Panel::new(Kind::Volume);
            let mut p = tiny_skia::Pixmap::new(1152, 672).unwrap();
            let (mut content, mut stale, mut mask, mut painted) = (None, false, None, None);
            for (frame, (growth, dimensions)) in [
                (0.1, (330.0, 50.0)),
                (0.6, (520.0, 300.0)),
                (1.0, (460.0, 260.0)),
                (0.3, (330.0, 50.0)),
            ]
            .into_iter()
            .enumerate()
            {
                panel.growth.snap(growth);
                let damage = panel.draw_shell(
                    &mut p,
                    &mut text,
                    &data,
                    &t,
                    1.2,
                    dimensions,
                    &mut content,
                    &mut stale,
                    &mut mask,
                    &mut painted,
                );
                assert!(frame == 0 || damage.w * damage.h < (p.width() * p.height()) as f32);
                let mut q = tiny_skia::Pixmap::new(p.width(), p.height()).unwrap();
                let (mut fresh_mask, mut fresh_painted) = (None, None);
                panel.draw_shell(
                    &mut q,
                    &mut text,
                    &data,
                    &t,
                    1.2,
                    dimensions,
                    &mut content,
                    &mut stale,
                    &mut fresh_mask,
                    &mut fresh_painted,
                );
                assert!(p.data() == q.data(), "{style} differs at {dimensions:?}");
            }
        }
    }
}
