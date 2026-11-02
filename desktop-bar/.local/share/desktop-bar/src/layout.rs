use crate::{
    modules::Data,
    text::Text,
    theme::{Color, Theme},
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn scale(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s, self.w * s, self.h * s)
    }
    pub fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self::new(
            x,
            y,
            (self.x + self.w).max(other.x + other.w) - x,
            (self.y + self.h).max(other.y + other.h) - y,
        )
    }
    pub fn clip(self, bounds: Self) -> Self {
        let x = self.x.max(bounds.x);
        let y = self.y.max(bounds.y);
        Self::new(
            x,
            y,
            ((self.x + self.w).min(bounds.x + bounds.w) - x).max(0.0),
            ((self.y + self.h).min(bounds.y + bounds.h) - y).max(0.0),
        )
    }
    /// Whole pixel rows touched by this rectangle, limited to `height`.
    pub fn rows(self, height: u32) -> std::ops::Range<usize> {
        let start = self.y.floor().clamp(0.0, height as f32) as usize;
        let end = (self.y + self.h).ceil().clamp(0.0, height as f32) as usize;
        start..end.max(start)
    }
    pub fn inset(self, d: f32) -> Self {
        Self::new(
            self.x + d,
            self.y + d,
            (self.w - 2.0 * d).max(0.0),
            (self.h - 2.0 * d).max(0.0),
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Module {
    Shell,
    Weather,
    Cpu,
    Memory,
    Disk,
    Volume,
    Microphone,
    Network,
    Battery,
    Clock,
    Workspace(i32),
    Tray(usize),
}
#[derive(Clone, Debug)]
pub struct Label {
    pub module: Module,
    pub icon: String,
    pub value: String,
    pub suffix: String,
    pub detail: String,
    pub accent: Color,
    pub color: Color,
}
#[derive(Clone, Debug)]
pub struct Cell {
    pub label: Label,
    pub rect: Rect,
    pub expansion: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub cells: Vec<Cell>,
    pub groups: Vec<Rect>,
    pub workspace: Rect,
    targets: Vec<(Module, Rect)>,
}

pub fn workspace_label(number: i32) -> String {
    if number == 10 { 0 } else { number }.to_string()
}

impl Layout {
    pub const MOTION_WORKSPACE_STEP: f32 = 22.0;

    pub fn motion(width: f32, data: &Data, t: &Theme, workspaces: &[i32]) -> Self {
        let workspace_width = (workspaces.len() as f32 * Self::MOTION_WORKSPACE_STEP).max(40.0);
        let left_width = workspace_width.max(58.0);
        let core_width = 182.0 + left_width;
        let tray: Vec<_> = data
            .tray
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.passive)
            .collect();
        let tray_width = if tray.is_empty() {
            0.0
        } else {
            12.0 + tray.len() as f32 * 24.0
        };
        let body = Rect::new(
            (width - core_width - tray_width) / 2.0,
            0.0,
            core_width + tray_width,
            32.0,
        );
        let slots = Rect::new(body.x + 18.0, 0.0, workspace_width, 32.0);
        let mut layout = Self {
            workspace: slots,
            groups: vec![body],
            ..Default::default()
        };
        let mut add = |module, x, w| {
            layout.cells.push(Cell {
                label: Label {
                    module,
                    icon: String::new(),
                    value: String::new(),
                    suffix: String::new(),
                    detail: String::new(),
                    accent: t.accent(),
                    color: t.text(),
                },
                rect: Rect::new(x, 0.0, w, 32.0),
                expansion: 0.0,
            });
        };
        add(Module::Clock, body.x + left_width + 32.0, 60.0);
        for (position, &n) in workspaces.iter().enumerate() {
            add(
                Module::Workspace(n),
                slots.x + position as f32 * Self::MOTION_WORKSPACE_STEP,
                Self::MOTION_WORKSPACE_STEP,
            );
        }
        let x = body.x + core_width - 64.0;
        add(Module::Volume, x - 24.0, 22.0);
        add(Module::Network, x, 24.0);
        if data.battery.is_some() {
            add(Module::Battery, x + 26.0, 26.0);
        }
        for (slot, (index, _)) in tray.into_iter().enumerate() {
            add(
                Module::Tray(index),
                body.x + core_width + slot as f32 * 24.0,
                24.0,
            );
        }
        add(Module::Shell, body.x, body.w);
        let mut controls: Vec<_> = layout
            .cells
            .iter()
            .filter(|cell| !matches!(cell.label.module, Module::Shell | Module::Workspace(_)))
            .map(|cell| (cell.label.module, cell.rect))
            .collect();
        controls.sort_by(|left, right| left.1.x.total_cmp(&right.1.x));
        let boundary = |left: Rect, right: Rect| {
            ((left.x + left.w / 2.0 + right.x + right.w / 2.0) / 2.0)
                .clamp(left.x + left.w, right.x)
        };
        for (index, &(module, rect)) in controls.iter().enumerate() {
            let left = if index == 0 {
                rect.x
            } else {
                boundary(controls[index - 1].1, rect)
            };
            let right = controls.get(index + 1).map_or_else(
                || (rect.x + rect.w + 6.0).min(body.x + body.w),
                |&(_, next)| boundary(rect, next),
            );
            layout
                .targets
                .push((module, Rect::new(left, body.y, right - left, body.h)));
        }
        layout
    }
    pub fn hit(&self, x: f32, y: f32) -> Option<Module> {
        self.targets
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(module, _)| *module)
            .or_else(|| {
                self.cells
                    .iter()
                    .find(|c| c.rect.contains(x, y))
                    .map(|c| c.label.module)
            })
    }
    pub fn build(
        width: f32,
        minimal: bool,
        workspaces: &[i32],
        labels: Vec<Label>,
        text: &mut Text,
        expand: impl Fn(Module) -> f32,
    ) -> Self {
        if minimal {
            let cells = labels
                .into_iter()
                .filter(|l| l.module == Module::Clock)
                .map(|mut l| {
                    l.icon.clear();
                    l.suffix.clear();
                    l.detail.clear();
                    let w = text.width(&l.value, 14.0) + 30.0;
                    Cell {
                        label: l,
                        rect: Rect::new(width - w - 20.0, 15.0, w, 36.0),
                        expansion: 0.0,
                    }
                })
                .collect::<Vec<_>>();
            return Self {
                groups: cells.iter().map(|c| c.rect).collect(),
                cells,
                ..Default::default()
            };
        }
        let workspace_width = (workspaces.len() as f32 * 30.0 + 22.0).max(52.0);
        let workspace = Rect::new((width - workspace_width) / 2.0, 15.0, workspace_width, 36.0);
        let budget = ((width - workspace_width) / 2.0 - 28.0).max(0.0);
        let mut result = Self {
            workspace,
            ..Default::default()
        };
        let tray_labels: Vec<_> = labels
            .iter()
            .filter(|l| matches!(l.module, Module::Tray(_)))
            .cloned()
            .collect();
        let tray_width = if tray_labels.is_empty() {
            0.0
        } else {
            12.0 + tray_labels.len() as f32 * 30.0
        };
        let tray_reserved = if tray_width > 0.0 {
            tray_width + 8.0
        } else {
            0.0
        };
        for left in [true, false] {
            let budget = if left {
                budget
            } else {
                (budget - tray_reserved).max(0.0)
            };
            let mut labels: Vec<_> = labels
                .iter()
                .filter(|l| {
                    matches!(
                        l.module,
                        Module::Weather | Module::Cpu | Module::Memory | Module::Disk
                    ) == left
                        && !matches!(l.module, Module::Tray(_))
                })
                .cloned()
                .collect();
            let cell_width = |l: &Label, text: &mut Text| {
                let icon_width = text.width(&l.icon, 13.0);
                if l.value.is_empty() && l.suffix.is_empty() && l.detail.is_empty() {
                    return 24.0 + icon_width;
                }
                let extra = expand(l.module).clamp(0.0, 1.0);
                24.0 + icon_width
                    + 7.0
                    + text.width(&l.value, 13.0)
                    + text.width(&l.suffix, 13.0)
                    + (text.width(&l.detail, 11.0) + 12.0) * extra
            };
            // Keep the switcher unobstructed on narrow outputs. Details fold first.
            let mut widths: Vec<_> = labels.iter().map(|l| cell_width(l, text)).collect();
            for remove in [
                Module::Disk,
                Module::Weather,
                Module::Memory,
                Module::Microphone,
                Module::Battery,
            ] {
                if widths.iter().sum::<f32>() + 12.0 <= budget {
                    break;
                }
                if let Some(i) = labels.iter().position(|l| l.module == remove) {
                    labels.remove(i);
                    widths.remove(i);
                }
            }
            let total = (widths.iter().sum::<f32>() + 12.0).min(budget);
            let x = if left {
                10.0
            } else {
                width - total - 10.0 - tray_reserved
            };
            let group = Rect::new(x, 15.0, total, 36.0);
            if total > 12.0 {
                result.groups.push(group);
            }
            let mut x = x + 6.0;
            for (label, w) in labels.into_iter().zip(widths) {
                let w = w.min((group.x + group.w - 6.0 - x).max(0.0));
                if w > 20.0 {
                    result.cells.push(Cell {
                        expansion: expand(label.module),
                        label,
                        rect: Rect::new(x, 19.0, w, 28.0),
                    });
                }
                x += w;
            }
        }
        if tray_width > 0.0 {
            let group = Rect::new(width - tray_width - 10.0, 15.0, tray_width, 36.0);
            result.groups.push(group);
            for (i, label) in tray_labels.into_iter().enumerate() {
                result.cells.push(Cell {
                    label,
                    rect: Rect::new(group.x + 6.0 + i as f32 * 30.0, 19.0, 30.0, 28.0),
                    expansion: 0.0,
                });
            }
        }
        result.groups.push(workspace);
        for (position, &n) in workspaces.iter().enumerate() {
            result.cells.push(Cell {
                label: Label {
                    module: Module::Workspace(n),
                    icon: String::new(),
                    value: workspace_label(n),
                    suffix: String::new(),
                    detail: String::new(),
                    accent: Color::hex("#ffffff"),
                    color: Color::hex("#ffffff"),
                },
                rect: Rect::new(
                    workspace.x + 11.0 + position as f32 * 30.0,
                    19.0,
                    28.0,
                    28.0,
                ),
                expansion: 0.0,
            });
        }
        result
    }
}

pub fn labels(data: &Data, t: &Theme, long_date: bool) -> Vec<Label> {
    let label =
        |module, icon: &str, value: String, suffix: String, detail: String, accent, color| Label {
            module,
            icon: icon.into(),
            value,
            suffix,
            detail,
            accent,
            color,
        };
    let level = |n: f32, warn: f32, critical: f32| {
        if n >= critical {
            t.red()
        } else if n >= warn {
            t.yellow()
        } else {
            t.text()
        }
    };
    let mut v = vec![];
    let (icon, temperature, wind, condition) = data
        .weather
        .observation
        .as_ref()
        .map(|w| {
            let day = (w.sunrise..=w.sunset).contains(&crate::modules::clock::now());
            let icon = match w.id {
                200..=232 => "",
                300..=531 => "",
                600..=622 => "",
                700..=781 => "",
                800 if day => "",
                800 => "",
                _ => "",
            };
            (
                icon,
                format!("{:.0}°", w.temperature),
                format!("  󰖝 {:.1} km/h", w.wind_kmh),
                w.description.clone(),
            )
        })
        .unwrap_or(("", "—".into(), String::new(), "Weather unavailable".into()));
    v.push(label(
        Module::Weather,
        icon,
        temperature,
        wind,
        condition,
        t.yellow(),
        if data.weather.stale {
            t.dim()
        } else {
            t.text()
        },
    ));
    let cpu = data.cpu.first().copied().unwrap_or(0.0);
    v.push(label(
        Module::Cpu,
        "󰍛",
        format!("{cpu:.0}%"),
        String::new(),
        "cores".into(),
        t.green(),
        level(cpu, 70.0, 90.0),
    ));
    if let Some(m) = &data.memory {
        v.push(label(
            Module::Memory,
            "",
            format!("{:.1}", m.used),
            format!(" / {:.0}G", m.total),
            format!("swap {:.1}/{:.0}G", m.swap_used, m.swap_total),
            t.yellow(),
            level(m.used / m.total * 100.0, 80.0, 92.0),
        ));
    }
    if let Some(d) = &data.disk {
        v.push(label(
            Module::Disk,
            "",
            format!("{:.0}", d.free),
            "G free".into(),
            format!("{:.0}G total", d.total),
            t.blue(),
            level(d.used_percent, 85.0, 95.0),
        ));
    }
    for (module, a) in [
        (Module::Volume, &data.audio),
        (Module::Microphone, &data.microphone),
    ] {
        let icon = match (module, a.muted) {
            (Module::Microphone, true) => "",
            (Module::Microphone, false) => "",
            (_, true) => "󰝟",
            (_, false) if a.volume == 0 => "",
            (_, false) if a.volume <= 50 => "",
            _ => "",
        };
        v.push(label(
            module,
            icon,
            if a.available {
                format!("{}%", a.volume)
            } else {
                "—".into()
            },
            String::new(),
            String::new(),
            if a.muted { t.red() } else { t.blue() },
            if a.muted { t.dim() } else { t.text() },
        ));
    }
    v.push(label(
        Module::Network,
        data.network.icon(),
        String::new(),
        String::new(),
        String::new(),
        t.blue(),
        if data.network.connected() {
            t.text()
        } else {
            t.dim()
        },
    ));
    if let Some(b) = &data.battery {
        v.push(label(
            Module::Battery,
            if b.charging {
                "󰂄"
            } else if b.plugged {
                "󰚥"
            } else {
                ["󰁺", "󰁼", "󰁾", "󰂀", "󰁹"][(b.percent / 20.0).clamp(0.0, 4.0) as usize]
            },
            format!("{:.0}%", b.percent),
            String::new(),
            String::new(),
            if b.plugged || b.percent > 40.0 {
                t.green()
            } else if b.percent <= 15.0 {
                t.red()
            } else {
                t.yellow()
            },
            if b.plugged {
                t.text()
            } else {
                level(100.0 - b.percent, 60.0, 85.0)
            },
        ));
    }
    v.push(label(
        Module::Clock,
        "󰥔",
        crate::modules::clock::format(if long_date { "%Y-%m-%d" } else { "%H:%M" }),
        if long_date {
            String::new()
        } else {
            crate::modules::clock::format("  %a %d %b")
        },
        crate::modules::clock::format("%A %d %B %Y"),
        t.blue(),
        t.text(),
    ));
    for (i, item) in data
        .tray
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.passive)
    {
        v.push(label(
            Module::Tray(i),
            "",
            String::new(),
            String::new(),
            item.title.clone(),
            t.blue(),
            t.text(),
        ));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hit_testing_uses_exclusive_right_and_bottom_edges() {
        let rect = Rect::new(10.0, 15.0, 20.0, 30.0);
        assert!(rect.contains(10.0, 15.0));
        assert!(!rect.contains(30.0, 15.0));
        assert!(!rect.contains(10.0, 45.0));
        let layout = Layout::default();
        assert_eq!(layout.hit(0.0, 0.0), None);
    }

    #[test]
    fn tenth_workspace_displays_zero_without_changing_its_destination() {
        let theme = Theme::default();
        let mut text = Text::new(&theme.font).unwrap();
        let layout = Layout::build(1920.0, false, &[1, 10], vec![], &mut text, |_| 0.0);
        let cell = layout.cells.last().unwrap();
        assert_eq!(cell.label.value, "0");
        assert_eq!(
            layout.hit(cell.rect.x, cell.rect.y),
            Some(Module::Workspace(10))
        );
        assert_eq!(workspace_label(1), "1");
    }

    #[test]
    fn motion_gaps_route_to_the_nearest_control_at_every_height() {
        let mut data = crate::sample_data();
        for battery in [true, false] {
            if !battery {
                data.battery = None;
            }
            for count in [1, 3, 10] {
                let layout = Layout::motion(
                    960.0,
                    &data,
                    &Theme::default(),
                    &(1..=count).collect::<Vec<_>>(),
                );
                let controls: Vec<_> = layout
                    .cells
                    .iter()
                    .filter(|cell| {
                        !matches!(cell.label.module, Module::Shell | Module::Workspace(_))
                    })
                    .collect();
                for pair in controls.windows(2) {
                    let left = pair[0];
                    let right = pair[1];
                    let mut x = left.rect.x + left.rect.w;
                    while x < right.rect.x {
                        let distance = |cell: &Cell| (x - cell.rect.x - cell.rect.w / 2.0).abs();
                        let nearest = if distance(left) < distance(right) {
                            left
                        } else {
                            right
                        };
                        for y in [0.0, 16.0, 31.99] {
                            assert_eq!(layout.hit(x, y), Some(nearest.label.module));
                        }
                        x += 0.25;
                    }
                }
                assert!(
                    layout
                        .targets
                        .windows(2)
                        .all(|pair| pair[0].1.x + pair[0].1.w == pair[1].1.x)
                );
                assert_eq!(layout.hit(layout.groups[0].x - 0.1, 16.0), None);
                assert_eq!(layout.hit(layout.groups[0].x, 32.0), None);
            }
        }
    }

    #[test]
    fn motion_status_targets_stay_inside_the_body_without_overlapping() {
        let data = crate::sample_data();
        for count in [1, 3, 10] {
            let workspaces: Vec<_> = (1..=count).collect();
            let layout = Layout::motion(960.0, &data, &Theme::default(), &workspaces);
            let body = layout.groups[0];
            assert_eq!(body.w, 182.0 + (count as f32 * 22.0).max(58.0));
            assert!(
                layout
                    .cells
                    .iter()
                    .any(|cell| cell.label.module == Module::Volume)
            );
            assert!(
                layout
                    .cells
                    .iter()
                    .any(|cell| cell.label.module == Module::Network)
            );
            for cell in layout
                .cells
                .iter()
                .filter(|cell| cell.label.module != Module::Shell)
            {
                let rect = cell.rect;
                assert!(rect.w >= 10.0 && rect.h == 32.0);
                assert!(rect.x >= body.x && rect.x + rect.w <= body.x + body.w);
                assert!(rect.y >= body.y && rect.y + rect.h <= body.y + body.h);
                assert_eq!(
                    layout.hit(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
                    Some(cell.label.module)
                );
            }
            let mut cells: Vec<_> = layout
                .cells
                .iter()
                .filter(|cell| cell.label.module != Module::Shell)
                .collect();
            cells.sort_by(|left, right| left.rect.x.total_cmp(&right.rect.x));
            assert!(
                cells
                    .windows(2)
                    .all(|pair| pair[0].rect.x + pair[0].rect.w <= pair[1].rect.x)
            );
        }
    }
}
