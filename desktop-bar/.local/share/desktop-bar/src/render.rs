use crate::{
    anim::{Spring, ease},
    draw,
    layout::{self, Layout, Module, Rect},
    modules::{Data, workspaces::Snapshot},
    style,
    text::Text,
    theme::{Color, Theme},
};
use std::hash::{Hash, Hasher};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

struct WorkspacePalette {
    background: Color,
    outline: Color,
    text: Color,
    start: Color,
    end: Color,
    monitors: [Color; 6],
}
impl WorkspacePalette {
    fn for_mode(light: bool) -> Self {
        // Keep the existing workspace slider's palette independent of the
        // surrounding bar's accent and terminal palette.
        if light {
            Self {
                background: Color::hex("#ffffff").alpha(0.70),
                outline: Color::hex("#ffffff").alpha(0.55),
                text: Color::hex("#4c4f69"),
                start: Color::hex("#635bff"),
                end: Color::hex("#923df5"),
                monitors: [
                    "#df8e1d", "#179299", "#40a02b", "#d20f39", "#8839ef", "#fe640b",
                ]
                .map(Color::hex),
            }
        } else {
            Self {
                background: Color::hex("#121214").alpha(0.82),
                outline: Color::hex("#52525b").alpha(0.22),
                text: Color::hex("#f4f4f5"),
                start: Color::hex("#60a5fa"),
                end: Color::hex("#a778fa"),
                monitors: [
                    "#f9e2af", "#89dceb", "#a6e3a1", "#f38ba8", "#cba6f7", "#fab387",
                ]
                .map(Color::hex),
            }
        }
    }
}

struct Value {
    previous: String,
    current: String,
    from: Color,
    to: Color,
    changed: Instant,
    old_icon: String,
    icon: String,
    from_accent: Color,
    to_accent: Color,
}
pub struct View {
    pub layout: Layout,
    pub hover: Option<Module>,
    pub long_date: bool,
    pub active: i32,
    pub space: Option<String>,
    pub occupied: HashSet<i32>,
    pub remote: HashMap<i32, usize>,
    pub available: bool,
    pub urgent: HashSet<i32>,
    motion: bool,
    left: Spring,
    right: Spring,
    expansion: HashMap<Module, Spring>,
    values: HashMap<Module, Value>,
    deferred: Option<i32>,
    last: Instant,
    started: Instant,
    signatures: Vec<u64>,
    previous_groups: Vec<Rect>,
    painted_size: Option<(u32, u32)>,
    pub fade: Option<(tiny_skia::Pixmap, Instant)>,
    pub force: bool,
}
impl Default for View {
    fn default() -> Self {
        Self::new(false)
    }
}

/// Pixel rows that the compact notch and its shadow can touch: the body is
/// 32 logical pixels tall and its shadow reaches 15 more, plus antialiasing.
pub fn motion_rows(s: f32) -> u32 {
    (47.0 * s + 2.0).ceil() as u32
}

/// Identifies the status data that the compact notch draws.
pub fn motion_data_signature(data: &Data) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    data.audio.available.hash(&mut hash);
    data.audio.muted.hash(&mut hash);
    data.network.connected().hash(&mut hash);
    data.network.bars().hash(&mut hash);
    data.battery
        .as_ref()
        .map(|battery| (battery.percent.to_bits(), battery.charging))
        .hash(&mut hash);
    for (index, item) in data
        .tray
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.passive)
    {
        index.hash(&mut hash);
        item.icon
            .as_ref()
            .map(std::sync::Arc::as_ptr)
            .hash(&mut hash);
    }
    hash.finish()
}

impl View {
    pub fn new(motion: bool) -> Self {
        Self {
            layout: Layout::default(),
            hover: None,
            long_date: false,
            active: 1,
            space: None,
            occupied: HashSet::new(),
            remote: HashMap::new(),
            available: false,
            urgent: HashSet::new(),
            motion,
            left: Spring::new(0.0, 25.0, 0.7),
            right: Spring::new(25.0, 25.0, 0.7),
            expansion: HashMap::new(),
            values: HashMap::new(),
            deferred: None,
            last: Instant::now(),
            started: Instant::now(),
            signatures: vec![],
            previous_groups: vec![],
            painted_size: None,
            fade: None,
            force: true,
        }
    }
    pub fn settle_workspaces(&mut self) {
        self.left.snap(self.left.target);
        self.right.snap(self.right.target);
        self.deferred = None;
    }
    pub fn visible_workspaces(&self) -> Vec<i32> {
        if !self.motion {
            return (1..=10).collect();
        }
        if !self.available {
            return vec![];
        }
        (1..=10)
            .filter(|n| {
                *n == self.active || (self.occupied.contains(n) && !self.remote.contains_key(n))
            })
            .collect()
    }
    fn render_motion(
        &mut self,
        p: &mut tiny_skia::Pixmap,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        width: f32,
        s: f32,
    ) -> Vec<Rect> {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        self.left.step(dt);
        self.right.step(dt);
        if !self.left.moving() && !self.right.moving() {
            self.deferred = None;
        }
        let workspaces = self.visible_workspaces();
        let layout = Layout::motion(width, data, t, &workspaces);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        self.active.hash(&mut hash);
        self.available.hash(&mut hash);
        self.left.value.to_bits().hash(&mut hash);
        self.right.value.to_bits().hash(&mut hash);
        for n in 1..=10 {
            self.occupied.contains(&n).hash(&mut hash);
            self.remote.get(&n).hash(&mut hash);
            self.urgent.contains(&n).hash(&mut hash);
        }
        crate::modules::clock::format("%H:%M").hash(&mut hash);
        motion_data_signature(data).hash(&mut hash);
        let signature = hash.finish();
        if !self.force && self.signatures == [signature] && self.previous_groups == layout.groups {
            return vec![];
        }
        // Rows beneath the notch stay transparent after the first full clear.
        let full = self.force || self.painted_size != Some((p.width(), p.height()));
        let band = if full {
            p.height()
        } else {
            motion_rows(s).min(p.height())
        };
        let stride = p.width() as usize * 4;
        p.data_mut()[..band as usize * stride].fill(0);
        self.painted_size = Some((p.width(), p.height()));
        draw::notch(
            p,
            layout.groups[0].scale(s),
            s,
            if t.light {
                Color::hex("#f8fafc")
            } else {
                Color::hex("#000000")
            },
        );
        self.draw_motion_content(p, text, data, t, s, &layout);
        self.force = false;
        self.fade = None;
        self.signatures = vec![signature];
        self.previous_groups = layout.groups.clone();
        self.layout = layout;
        text.release_font_pages();
        vec![Rect::new(0.0, 0.0, p.width() as f32, band as f32)]
    }
    pub fn motion_content(
        &self,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        s: f32,
        width: f32,
    ) -> Option<tiny_skia::Pixmap> {
        let layout = Layout::motion(width, data, t, &self.visible_workspaces());
        let body = layout.groups[0];
        let mut content = tiny_skia::Pixmap::new(
            (width * s).round() as u32,
            ((body.h + 12.0) * s).round() as u32,
        )?;
        self.draw_motion_content(&mut content, text, data, t, s, &layout);
        content.clone_rect(tiny_skia::IntRect::from_xywh(
            ((body.x - 16.0) * s).round() as i32,
            0,
            ((body.w + 32.0) * s).round() as u32,
            content.height(),
        )?)
    }
    fn draw_motion_content(
        &self,
        p: &mut tiny_skia::Pixmap,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        s: f32,
        layout: &Layout,
    ) {
        let foreground = if t.light {
            Color::hex("#27272a")
        } else {
            t.text()
        };
        let active_rect = if self.available && self.visible_workspaces().contains(&self.active) {
            let step = Layout::MOTION_WORKSPACE_STEP / 30.0;
            let left = layout.workspace.x + self.left.value * step;
            let right = layout.workspace.x + self.right.value * step;
            let rect = Rect::new(left + 1.0, 5.0, (right - left - 1.0).max(5.0), 22.0).scale(s);
            draw::rounded(p, rect, 8.0 * s, foreground.alpha(0.9));
            Some(rect)
        } else {
            None
        };
        if let Some(tray) = layout
            .cells
            .iter()
            .find(|cell| matches!(cell.label.module, Module::Tray(_)))
        {
            draw::rounded(
                p,
                Rect::new(tray.rect.x - 7.0, 9.0, 1.0, 14.0).scale(s),
                0.5 * s,
                foreground.alpha(0.2),
            );
        }
        for cell in &layout.cells {
            let r = cell.rect.scale(s);
            match cell.label.module {
                Module::Clock => {
                    let label = crate::modules::clock::format("%H:%M");
                    let label_width = text.width(&label, 12.0 * s);
                    text.draw(
                        p,
                        &label,
                        r.x + (r.w - label_width) / 2.0,
                        20.5 * s,
                        12.0 * s,
                        foreground.alpha(0.92),
                    );
                }
                Module::Workspace(n) => {
                    let label = layout::workspace_label(n);
                    let x = r.x + (r.w - text.width(&label, 11.0 * s)) / 2.0;
                    text.draw(
                        p,
                        &label,
                        x,
                        20.0 * s,
                        11.0 * s,
                        if self.urgent.contains(&n) {
                            t.red()
                        } else {
                            foreground.alpha(0.65)
                        },
                    );
                    if let Some(clip) = active_rect {
                        text.draw_clipped(
                            p,
                            &label,
                            x,
                            20.0 * s,
                            11.0 * s,
                            if t.light {
                                Color::hex("#f8fafc")
                            } else {
                                Color::hex("#000000")
                            },
                            clip,
                        );
                    }
                }
                Module::Volume => {
                    let icon = if data.audio.muted { "󰝟" } else { "" };
                    let icon_width = text.width(icon, 13.0 * s);
                    text.draw(
                        p,
                        icon,
                        r.x + (r.w - icon_width) / 2.0,
                        21.0 * s,
                        13.0 * s,
                        foreground.alpha(if data.audio.available { 0.9 } else { 0.35 }),
                    );
                }
                Module::Network => crate::overview::signal(
                    p,
                    Rect::new(cell.rect.x + 5.0, 12.0, 14.0, 9.0).scale(s),
                    data,
                    foreground,
                ),
                Module::Battery => crate::overview::battery(
                    p,
                    Rect::new(cell.rect.x + 2.0, 11.0, 22.0, 11.0).scale(s),
                    data,
                    foreground,
                ),
                Module::Tray(index) => {
                    if let Some(icon) = data.tray.get(index).and_then(|item| item.icon.as_ref()) {
                        let size = 16.0 * s;
                        p.draw_pixmap(
                            0,
                            0,
                            icon.as_ref().as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            tiny_skia::Transform::from_scale(
                                size / icon.width() as f32,
                                size / icon.height() as f32,
                            )
                            .post_translate(r.x + (r.w - size) / 2.0, 8.0 * s),
                            None,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    pub fn snapshot(&mut self, value: Option<Snapshot>) {
        let Some(s) = value else {
            self.available = false;
            self.occupied.clear();
            self.remote.clear();
            self.urgent.clear();
            self.force = true;
            return;
        };
        let before = self.visible_workspaces();
        let changed = !self.available || self.space != s.space;
        let old = self.active;
        self.available = true;
        self.space = s.space;
        self.occupied = s.occupied;
        self.remote = s.remote;
        self.active = s.active;
        self.urgent = s.urgent;
        let visible = self.visible_workspaces();
        let x = visible.iter().position(|&n| n == self.active).unwrap_or(0) as f32 * 30.0;
        if changed {
            self.left.snap(x);
            self.right.snap(x + 28.0);
            self.deferred = None;
        } else if old != self.active || before != visible {
            self.last = Instant::now();
            self.deferred = (!self.occupied.contains(&self.active)).then_some(self.active);
            self.left.target = x;
            self.right.target = x + 28.0;
            let forward = self.active > old;
            // The leading edge arrives first; the trailing edge stretches and settles.
            self.left.tune(if forward { 17.0 } else { 27.0 }, 0.7);
            self.right.tune(if forward { 27.0 } else { 17.0 }, 0.7);
        }
        self.urgent.remove(&self.active);
        self.urgent.retain(|n| visible.contains(n));
    }
    pub fn moving(&self, t: &Theme) -> bool {
        self.left.moving()
            || self.right.moving()
            || self.expansion.values().any(Spring::moving)
            || self.fade.is_some()
            || (!t.motion() && !self.urgent.is_empty())
            || (t.motion()
                && self
                    .values
                    .values()
                    .any(|v| v.changed.elapsed().as_secs_f32() < 0.3))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        p: &mut tiny_skia::Pixmap,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        width: f32,
        minimal: bool,
        s: f32,
    ) -> Vec<Rect> {
        if t.motion() && !minimal {
            return self.render_motion(p, text, data, t, width, s);
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        let mut expand = HashMap::new();
        for module in [Module::Cpu, Module::Memory, Module::Clock, Module::Weather] {
            let spring = self
                .expansion
                .entry(module)
                .or_insert_with(|| Spring::new(0.0, 22.0, 0.9));
            spring.target = if t.motion() && self.hover == Some(module) {
                1.0
            } else {
                0.0
            };
            spring.step(dt);
            expand.insert(module, spring.value.clamp(0.0, 1.0));
        }
        if !t.motion() {
            self.left.tune(28.0, 1.0);
            self.right.tune(28.0, 1.0);
        }
        self.left.step(dt);
        self.right.step(dt);
        if !self.left.moving() && !self.right.moving() {
            self.deferred = None;
        }
        let labels = layout::labels(data, t, self.long_date);
        for l in &labels {
            let value = self.values.entry(l.module).or_insert_with(|| Value {
                old_icon: l.icon.clone(),
                icon: l.icon.clone(),
                from_accent: l.accent,
                to_accent: l.accent,
                previous: l.value.clone(),
                current: l.value.clone(),
                from: l.color,
                to: l.color,
                changed: now - std::time::Duration::from_secs(1),
            });
            if value.current != l.value
                || value.to != l.color
                || value.icon != l.icon
                || value.to_accent != l.accent
            {
                value.old_icon = std::mem::replace(&mut value.icon, l.icon.clone());
                value.from_accent = value.to_accent;
                value.to_accent = l.accent;
                value.previous = std::mem::replace(&mut value.current, l.value.clone());
                value.from = value.to;
                value.to = l.color;
                value.changed = now;
            }
        }
        let layout = Layout::build(
            width,
            minimal,
            &self.visible_workspaces(),
            labels,
            text,
            |m| expand.get(&m).copied().unwrap_or(0.0),
        );
        let geometry_changed = self.previous_groups != layout.groups;
        let workspace_palette = WorkspacePalette::for_mode(t.light);
        let full = self.force || geometry_changed || self.fade.is_some();
        let mut damage = vec![];
        if full {
            p.fill(tiny_skia::Color::TRANSPARENT);
            damage.push(Rect::new(0.0, 0.0, p.width() as f32, p.height() as f32));
        }
        let mut signatures = vec![];
        for (i, group) in layout.groups.iter().enumerate() {
            let cells: Vec<_> = layout
                .cells
                .iter()
                .filter(|c| group.contains(c.rect.x, c.rect.y))
                .collect();
            let workspace = *group == layout.workspace;
            let mut signature = std::collections::hash_map::DefaultHasher::new();
            self.hover.hash(&mut signature);
            if workspace {
                self.available.hash(&mut signature);
                self.left.value.to_bits().hash(&mut signature);
                self.right.value.to_bits().hash(&mut signature);
                for n in 1..=10 {
                    self.occupied.contains(&n).hash(&mut signature);
                    self.remote.get(&n).hash(&mut signature);
                    self.urgent.contains(&n).hash(&mut signature);
                }
                if !self.urgent.is_empty() {
                    ((self.started.elapsed().as_secs_f32() * 60.0) as u32).hash(&mut signature);
                }
            } else {
                for c in &cells {
                    let l = &c.label;
                    l.module.hash(&mut signature);
                    l.icon.hash(&mut signature);
                    l.value.hash(&mut signature);
                    l.suffix.hash(&mut signature);
                    l.detail.hash(&mut signature);
                    for value in [
                        c.rect.x,
                        c.rect.y,
                        c.rect.w,
                        c.rect.h,
                        c.expansion,
                        l.accent.0,
                        l.accent.1,
                        l.accent.2,
                        l.accent.3,
                        l.color.0,
                        l.color.1,
                        l.color.2,
                        l.color.3,
                    ] {
                        value.to_bits().hash(&mut signature);
                    }
                    if let Module::Tray(n) = l.module
                        && let Some(item) = data.tray.get(n)
                    {
                        item.icon
                            .as_ref()
                            .map(std::sync::Arc::as_ptr)
                            .hash(&mut signature);
                        item.attention.hash(&mut signature);
                    }
                    if l.module == Module::Cpu && c.expansion > 0.01 {
                        for core in &data.cpu {
                            core.to_bits().hash(&mut signature);
                        }
                    }
                    if let Some(value) = self.values.get(&l.module)
                        && value.changed.elapsed().as_secs_f32() < 0.3
                    {
                        ((value.changed.elapsed().as_secs_f32() * 60.0) as u32)
                            .hash(&mut signature);
                    }
                }
            }
            let signature = signature.finish();
            let dirty = full || self.signatures.get(i) != Some(&signature);
            signatures.push(signature);
            if !dirty {
                continue;
            }
            let r = group.scale(s);
            let bounds = Rect::new(r.x - 2.0 * s, r.y - 2.0 * s, r.w + 4.0 * s, r.h + 8.0 * s);
            if !full {
                draw::clear(p, bounds);
                damage.push(bounds);
            }
            if minimal {
                draw::rounded(p, r, 18.0 * s, t.color("background", "#1e1e2e"));
            } else if workspace {
                draw::rounded(p, r, 18.0 * s, workspace_palette.background);
                draw::outline(p, r, 18.0 * s, s, workspace_palette.outline);
            } else if t.motion() {
                style::motion::group(p, r, t, s);
            } else {
                style::clean::group(p, r, t, s);
            }
            if workspace && self.available && (1..=10).contains(&self.active) {
                let left = layout.workspace.x + 11.0 + self.left.value;
                let right = layout.workspace.x + 11.0 + self.right.value;
                let pill = Rect::new(left, 19.0, (right - left).max(18.0), 28.0).scale(s);
                for (spread, alpha) in [(4.0, 0.035), (2.0, 0.07)] {
                    draw::rounded(
                        p,
                        Rect::new(
                            pill.x - spread * s,
                            pill.y + (2.0 - spread) * s,
                            pill.w + 2.0 * spread * s,
                            pill.h + 2.0 * spread * s,
                        ),
                        (14.0 + spread) * s,
                        workspace_palette.start.alpha(alpha),
                    );
                }
                draw::gradient(
                    p,
                    pill,
                    14.0 * s,
                    workspace_palette.start,
                    workspace_palette.end,
                );
                draw::outline(p, pill, 14.0 * s, s, workspace_palette.text.alpha(0.20));
            }
            for (n, c) in cells.iter().enumerate() {
                let rect = c.rect.scale(s);
                if minimal {
                    text.draw(
                        p,
                        &c.label.value,
                        rect.x + 15.0 * s,
                        rect.y + 23.0 * s,
                        14.0 * s,
                        t.text(),
                    );
                    continue;
                }
                if self.hover == Some(c.label.module)
                    && !matches!(c.label.module,Module::Workspace(n) if n==self.active)
                {
                    draw::rounded(p, rect.inset(s), 18.0 * s, t.pill_hover());
                }
                if let Module::Workspace(n) = c.label.module {
                    let remote = self.remote.get(&n).copied();
                    let color = if n == self.active {
                        Color::hex("#ffffff")
                    } else if let Some(owner) = remote {
                        workspace_palette.monitors[owner % workspace_palette.monitors.len()]
                    } else if self.occupied.contains(&n) && self.deferred != Some(n) {
                        workspace_palette.text
                    } else {
                        workspace_palette.text.alpha(0.45)
                    };
                    if self.urgent.contains(&n) {
                        let pulse =
                            0.45 + 0.20 * (self.started.elapsed().as_secs_f32() * 5.0).sin();
                        draw::rounded(p, rect, 14.0 * s, t.red().alpha(pulse));
                    }
                    let w = text.width(&c.label.value, 14.0 * s);
                    text.draw(
                        p,
                        &c.label.value,
                        rect.x + (rect.w - w) / 2.0,
                        rect.y + 19.0 * s,
                        14.0 * s,
                        color,
                    );
                    continue;
                }
                if let Module::Tray(index) = c.label.module {
                    if data.tray.get(index).is_some_and(|i| i.attention) {
                        draw::rounded(p, rect.inset(2.0 * s), 14.0 * s, t.red().alpha(0.20));
                    }
                    if let Some(icon) = data.tray.get(index).and_then(|i| i.icon.as_ref()) {
                        let size = 16.0 * s;
                        let transform = tiny_skia::Transform::from_scale(
                            size / icon.width() as f32,
                            size / icon.height() as f32,
                        )
                        .post_translate(
                            rect.x + (rect.w - size) / 2.0,
                            rect.y + (rect.h - size) / 2.0,
                        );
                        p.draw_pixmap(
                            0,
                            0,
                            icon.as_ref().as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            transform,
                            None,
                        );
                    } else {
                        text.draw(
                            p,
                            "",
                            rect.x + 8.0 * s,
                            rect.y + 18.0 * s,
                            12.0 * s,
                            t.blue(),
                        );
                    }
                    continue;
                }
                if n > 0 {
                    draw::rounded(
                        p,
                        Rect::new(rect.x, rect.y + 5.0 * s, s, rect.h - 10.0 * s),
                        0.0,
                        t.divider(),
                    );
                }
                let baseline = rect.y + 18.0 * s;
                let x = rect.x + 12.0 * s;
                let value = &self.values[&c.label.module];
                let progress = if t.motion() {
                    ease(value.changed.elapsed().as_secs_f32() / 0.28)
                } else {
                    1.0
                };
                let accent = value.from_accent.mix(value.to_accent, progress);
                if progress < 1.0 && value.old_icon != value.icon {
                    text.draw_clipped(
                        p,
                        &value.old_icon,
                        x,
                        baseline,
                        13.0 * s,
                        accent.alpha(1.0 - progress),
                        rect,
                    );
                    text.draw_clipped(
                        p,
                        &c.label.icon,
                        x,
                        baseline,
                        13.0 * s,
                        accent.alpha(progress),
                        rect,
                    );
                } else {
                    text.draw_clipped(p, &c.label.icon, x, baseline, 13.0 * s, accent, rect);
                }
                let x = x + text.width(&c.label.icon, 13.0 * s) + 7.0 * s;
                let color = value.from.mix(value.to, progress);
                if progress < 1.0 && value.previous != value.current {
                    // Roll only changed digits, keeping stable columns anchored.
                    let mut dx = x;
                    let old: Vec<_> = value.previous.chars().collect();
                    for (n, ch) in value.current.chars().enumerate() {
                        let current = ch.to_string();
                        if old.get(n) == Some(&ch) {
                            text.draw_clipped(p, &current, dx, baseline, 13.0 * s, color, rect);
                        } else {
                            if let Some(previous) = old.get(n) {
                                text.draw_clipped(
                                    p,
                                    &previous.to_string(),
                                    dx,
                                    baseline - progress * 18.0 * s,
                                    13.0 * s,
                                    color.alpha(1.0 - progress),
                                    rect,
                                );
                            }
                            text.draw_clipped(
                                p,
                                &current,
                                dx,
                                baseline + (1.0 - progress) * 18.0 * s,
                                13.0 * s,
                                color.alpha(progress),
                                rect,
                            );
                        }
                        dx += text.width(&current, 13.0 * s);
                    }
                } else {
                    text.draw_clipped(p, &c.label.value, x, baseline, 13.0 * s, color, rect);
                }
                let x = x + text.width(&c.label.value, 13.0 * s);
                text.draw_clipped(p, &c.label.suffix, x, baseline, 13.0 * s, t.dim(), rect);
                let x = x + text.width(&c.label.suffix, 13.0 * s) + 12.0 * s;
                if c.expansion > 0.01 && c.label.module == Module::Cpu {
                    let count = data.cpu.len().saturating_sub(1).min(16);
                    for (i, value) in data.cpu.iter().skip(1).take(count).enumerate() {
                        let h = (*value / 100.0 * 16.0).max(2.0) * s;
                        let r = Rect::new(x + i as f32 * 4.0 * s, baseline - h, 2.0 * s, h);
                        if r.x + r.w <= rect.x + rect.w - 4.0 * s {
                            draw::rounded(p, r, s, t.green().alpha(c.expansion));
                        }
                    }
                } else if c.expansion > 0.01 {
                    text.draw_clipped(
                        p,
                        &c.label.detail,
                        x,
                        baseline,
                        11.0 * s,
                        t.dim().alpha(c.expansion),
                        rect,
                    );
                }
            }
        }
        if let Some((old, started)) = &self.fade {
            let progress = ease(started.elapsed().as_secs_f32() / 0.25);
            if old.width() == p.width() && old.height() == p.height() {
                for (pixel, previous) in p.data_mut().iter_mut().zip(old.data()) {
                    *pixel = (f32::from(*pixel) * progress
                        + f32::from(*previous) * (1.0 - progress))
                        .round() as u8;
                }
            }
            if progress >= 1.0 {
                self.fade = None;
            }
        }
        self.force = false;
        self.layout = layout;
        self.signatures = signatures;
        self.previous_groups = self.layout.groups.clone();
        text.release_font_pages();
        damage
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_frames_have_no_damage_or_animation_callbacks() {
        let t = Theme::default();
        let mut text = Text::new(&t.font).unwrap();
        let mut p = tiny_skia::Pixmap::new(1920, 56).unwrap();
        let mut view = View::new(t.motion());
        let data = crate::sample_data();
        assert!(
            !view
                .render(&mut p, &mut text, &data, &t, 1920.0, false, 1.0)
                .is_empty()
        );
        assert!(
            view.render(&mut p, &mut text, &data, &t, 1920.0, false, 1.0)
                .is_empty()
        );
        assert!(!view.moving(&t));
    }
    #[test]
    fn compact_notch_ignores_hidden_metrics_and_static_urgency() {
        let t = Theme {
            style: "native-motion".into(),
            ..Theme::default()
        };
        let mut text = Text::new(&t.font).unwrap();
        let mut p = tiny_skia::Pixmap::new(960, 560).unwrap();
        let mut view = View::new(t.motion());
        view.snapshot(Some(Snapshot {
            active: 1,
            space: None,
            occupied: HashSet::from([2]),
            remote: HashMap::new(),
            urgent: HashSet::from([2]),
        }));
        let mut data = crate::sample_data();
        assert!(
            !view
                .render(&mut p, &mut text, &data, &t, 960.0, false, 1.0)
                .is_empty()
        );
        let before = p.data().to_vec();
        data.cpu = vec![97.0; 8];
        data.memory.as_mut().unwrap().used += 2.0;
        data.audio.volume = 3;
        data.battery.as_mut().unwrap().watts += 10.0;
        assert!(
            view.render(&mut p, &mut text, &data, &t, 960.0, false, 1.0)
                .is_empty()
        );
        assert_eq!(p.data(), before);
        data.audio.muted = !data.audio.muted;
        assert!(
            !view
                .render(&mut p, &mut text, &data, &t, 960.0, false, 1.0)
                .is_empty()
        );
        assert_ne!(p.data(), before);
        assert!(
            view.render(&mut p, &mut text, &data, &t, 960.0, false, 1.0)
                .is_empty()
        );
        assert!(
            !view.moving(&t),
            "Static urgency must not keep requesting frames"
        );
        data.battery.as_mut().unwrap().percent -= 15.0;
        assert!(
            !view
                .render(&mut p, &mut text, &data, &t, 960.0, false, 1.0)
                .is_empty()
        );
        assert_ne!(p.data(), before);
        data.network.state = "connected".into();
        data.network.rssi = Some(-90);
        let before = p.data().to_vec();
        assert!(
            !view
                .render(&mut p, &mut text, &data, &t, 960.0, false, 1.0)
                .is_empty()
        );
        assert_ne!(p.data(), before);
    }
    #[test]
    fn compact_notch_partial_clears_match_full_redraws() {
        let t = Theme {
            style: "native-motion".into(),
            ..Theme::default()
        };
        let mut text = Text::new(&t.font).unwrap();
        let snapshot = Snapshot {
            active: 3,
            space: None,
            occupied: HashSet::from([1, 2, 3, 7]),
            remote: HashMap::new(),
            urgent: HashSet::new(),
        };
        let item = crate::modules::tray::Item {
            address: String::new(),
            bus: String::new(),
            path: String::new(),
            owner: String::new(),
            title: String::new(),
            icon: Some(std::sync::Arc::new(tiny_skia::Pixmap::new(8, 8).unwrap())),
            symbolic: false,
            menu: None,
            attention: false,
            passive: false,
            is_menu: false,
        };
        let changes: [fn(&mut Data, &crate::modules::tray::Item); 4] = [
            |data, item| data.tray = vec![item.clone(); 3],
            |data, _| data.audio.muted = !data.audio.muted,
            |data, _| data.tray.clear(),
            |data, _| data.battery.as_mut().unwrap().percent -= 20.0,
        ];
        for s in [1.0, 1.2, 2.0] {
            let size = ((960.0 * s) as u32, (560.0 * s) as u32);
            let mut view = View::new(true);
            view.snapshot(Some(snapshot.clone()));
            view.settle_workspaces();
            let mut p = tiny_skia::Pixmap::new(size.0, size.1).unwrap();
            let mut data = crate::sample_data();
            view.render(&mut p, &mut text, &data, &t, 960.0, false, s);
            let rows = motion_rows(s) as usize * p.width() as usize * 4;
            assert!(p.data()[rows..].iter().all(|&byte| byte == 0));
            for change in changes {
                change(&mut data, &item);
                let damage = view.render(&mut p, &mut text, &data, &t, 960.0, false, s);
                assert!(damage[0].h < size.1 as f32 / 4.0);
                let mut fresh = View::new(true);
                fresh.snapshot(Some(snapshot.clone()));
                fresh.settle_workspaces();
                let mut q = tiny_skia::Pixmap::new(size.0, size.1).unwrap();
                fresh.render(&mut q, &mut text, &data, &t, 960.0, false, s);
                assert!(p.data() == q.data(), "partial redraw differs at scale {s}");
            }
        }
    }
    #[test]
    fn urgent_workspaces_animate_until_the_namespace_is_lost() {
        let mut view = View::default();
        view.snapshot(Some(Snapshot {
            active: 1,
            space: None,
            occupied: HashSet::from([2]),
            remote: HashMap::new(),
            urgent: HashSet::from([2]),
        }));
        assert!(view.moving(&Theme::default()));
        view.snapshot(None);
        assert!(!view.moving(&Theme::default()));
    }
    #[test]
    fn groups_and_hit_targets_remain_separate_on_narrow_outputs() {
        let t = Theme::default();
        let mut text = Text::new(&t.font).unwrap();
        for width in [640.0, 1280.0, 1920.0] {
            let layout = Layout::build(
                width,
                false,
                &(1..=10).collect::<Vec<_>>(),
                layout::labels(&crate::sample_data(), &t, false),
                &mut text,
                |_| 0.0,
            );
            for (i, a) in layout.groups.iter().enumerate() {
                for b in layout.groups.iter().skip(i + 1) {
                    assert!(
                        a.x + a.w <= b.x || b.x + b.w <= a.x,
                        "overlapping groups at {width}: {a:?} {b:?}"
                    );
                }
            }
            for cell in layout.cells {
                if let Module::Workspace(n) = cell.label.module {
                    assert!((1..=10).contains(&n));
                }
            }
        }
    }
    #[test]
    fn motion_shows_only_its_used_workspaces_and_current_empty_destination() {
        let mut view = View::new(true);
        let mut snapshot = Snapshot {
            active: 4,
            space: Some("home".into()),
            occupied: HashSet::from([1, 2, 5, 10]),
            remote: HashMap::from([(2, 0), (10, 1)]),
            urgent: HashSet::from([2, 5]),
        };
        view.snapshot(Some(snapshot.clone()));
        assert_eq!(view.visible_workspaces(), [1, 4, 5]);
        assert_eq!(view.urgent, HashSet::from([5]));
        snapshot.remote.insert(5, 0);
        view.snapshot(Some(snapshot));
        assert_eq!(view.visible_workspaces(), [1, 4]);
        assert_eq!(view.left.target, 30.0);
    }
}
