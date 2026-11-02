//! Searchable providers share one surface, focus owner and animated geometry.
use crate::{
    anim::Spring,
    draw,
    layout::Rect,
    modules::{Data, Update},
    text::Text,
    theme::{Color, Theme},
};
use serde::Deserialize;
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::{
    collections::HashMap,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Apps,
    Themes,
    Wallpapers,
    Clipboard,
    Styles,
    Controls,
    Menu,
    Keybindings,
    Notifications,
    Audio,
    Bluetooth,
    Reminder,
    Wifi,
    Search,
    Picker,
}
impl Page {
    pub const ALL: [Self; 6] = [
        Self::Apps,
        Self::Themes,
        Self::Wallpapers,
        Self::Clipboard,
        Self::Controls,
        Self::Styles,
    ];
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "apps" => Self::Apps,
            "themes" => Self::Themes,
            "wallpapers" => Self::Wallpapers,
            "clipboard" => Self::Clipboard,
            "styles" => Self::Styles,
            "controls" => Self::Controls,
            "keybindings" => Self::Keybindings,
            "notifications" => Self::Notifications,
            "audio" => Self::Audio,
            "bluetooth" => Self::Bluetooth,
            "reminder" => Self::Reminder,
            "wifi" => Self::Wifi,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Apps => "Apps",
            Self::Themes => "Themes",
            Self::Wallpapers => "Wallpapers",
            Self::Clipboard => "Clipboard",
            Self::Styles => "Bar style",
            Self::Controls => "Controls",
            Self::Menu => "Menu",
            Self::Keybindings => "Keybindings",
            Self::Notifications => "Notifications",
            Self::Audio => "Audio",
            Self::Bluetooth => "Bluetooth",
            Self::Reminder => "Reminder",
            Self::Wifi => "Wi-Fi",
            Self::Search => "Search",
            Self::Picker => "Select",
        }
    }
    fn provider(self) -> &'static str {
        match self {
            Self::Apps => "apps",
            Self::Themes => "themes",
            Self::Wallpapers => "wallpapers",
            Self::Clipboard => "clipboard",
            Self::Controls => "controls",
            Self::Keybindings => "keybindings",
            Self::Notifications => "notifications",
            Self::Audio => "audio",
            Self::Bluetooth => "bluetooth",
            Self::Wifi => "wifi",
            _ => "",
        }
    }
    pub fn gallery(self) -> bool {
        matches!(self, Self::Themes | Self::Wallpapers | Self::Styles)
    }
}

#[derive(Clone, Debug, Default, Deserialize, serde::Serialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub keywords: String,
    pub image: String,
    pub icon: String,
    pub colors: Vec<String>,
    pub selected: bool,
    pub command: Vec<String>,
    #[serde(skip_serializing)]
    pub clipboard: Option<clipboard_history::Item>,
    pub category: String,
    pub level: Option<u32>,
    pub muted: bool,
    pub actions: Vec<String>,
    pub action_provider: String,
}
#[derive(Clone, Debug)]
pub struct Model {
    pub page: Page,
    pub provider: String,
    pub generation: u64,
    pub entries: Vec<Entry>,
    pub query: String,
    pub search_focused: bool,
    pub title: String,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub loading: bool,
    pub error: String,
    pub images: HashMap<String, Arc<tiny_skia::Pixmap>>,
    pub requested: Vec<String>,
    pub cursor: Spring,
    pub clipboard: crate::clipboard::State,
    pub section: usize,
    pub refreshing: bool,
    pub media_visible: bool,
    pub remote: bool,
    pub revision: u64,
    pub picker: Option<Arc<crate::picker::Request>>,
    pub clear_after_action: Option<String>,
}
impl Model {
    pub fn new(page: Page, generation: u64, theme: &Theme) -> Self {
        let entries = match page {
            Page::Styles => [
                (
                    "native-clean",
                    "Clean",
                    "Floating pills · full desktop status",
                ),
                (
                    "native-motion",
                    "Motion",
                    "One notch · everything within reach",
                ),
            ]
            .into_iter()
            .map(|(id, name, detail)| Entry {
                id: id.into(),
                name: name.into(),
                detail: detail.into(),
                selected: theme.style == id,
                command: vec!["bar".into(), "set".into(), id.into()],
                ..Default::default()
            })
            .collect(),
            Page::Controls => [
                (
                    "󰍜",
                    "Applications",
                    "Search installed apps",
                    "panel",
                    "apps",
                ),
                ("󰏘", "Themes", "Choose a desktop palette", "panel", "themes"),
                (
                    "󰸉",
                    "Wallpapers",
                    "Browse this theme's backgrounds",
                    "panel",
                    "wallpapers",
                ),
                (
                    "",
                    "Sound",
                    "Output and microphone levels",
                    "panel",
                    "audio",
                ),
                ("󰃭", "Calendar", "Date and month view", "panel", "calendar"),
                (
                    "󰁹",
                    "Battery",
                    "Charge, power draw and time remaining",
                    "panel",
                    "battery",
                ),
                (
                    "󰍛",
                    "System",
                    "CPU, memory, storage and weather",
                    "panel",
                    "system",
                ),
                (
                    "󰕮",
                    "Bar style",
                    "Switch between Clean and Motion",
                    "panel",
                    "styles",
                ),
                (
                    "󰂯",
                    "Bluetooth",
                    "Manage connected devices",
                    "panel",
                    "bluetooth",
                ),
                (
                    "󰖩",
                    "Network",
                    "Connection and signal strength",
                    "panel",
                    "network",
                ),
                (
                    "󰒓",
                    "Desktop settings",
                    "Appearance, input and setup",
                    "menu",
                    "setup",
                ),
                (
                    "󰐥",
                    "Power",
                    "Lock, sleep and session actions",
                    "menu",
                    "system",
                ),
                (
                    "",
                    "Clipboard",
                    "History, images and ordered paste",
                    "panel",
                    "clipboard",
                ),
            ]
            .into_iter()
            .map(|(icon, name, detail, command, arg)| Entry {
                id: name.into(),
                name: name.into(),
                icon: icon.into(),
                detail: detail.into(),
                command: vec![command.into(), arg.into()],
                ..Default::default()
            })
            .collect(),
            Page::Reminder => vec![Entry {
                id: "reminder".into(),
                name: "Set reminder".into(),
                detail: "Type minutes and a message above · e.g. 15 Take a break".into(),
                icon: "󰀠".into(),
                ..Default::default()
            }],
            _ => vec![],
        };
        let loading = !matches!(page, Page::Styles | Page::Controls | Page::Reminder);
        let mut model = Self {
            page,
            provider: page.provider().into(),
            generation,
            entries,
            query: String::new(),
            search_focused: page != Page::Controls,
            title: page.name().into(),
            filtered: vec![],
            selected: 0,
            loading,
            error: String::new(),
            images: HashMap::new(),
            requested: vec![],
            cursor: Spring::new(0.0, 23.0, 0.86),
            clipboard: crate::clipboard::State::default(),
            section: 0,
            refreshing: false,
            media_visible: false,
            remote: false,
            revision: 0,
            picker: None,
            clear_after_action: None,
        };
        model.filter();
        model.select_current();
        model
    }
    pub fn size(&self) -> (u32, u32) {
        match self.page {
            Page::Apps => (480, 336),
            Page::Search => (560, 336),
            Page::Picker
                if self
                    .picker
                    .as_ref()
                    .is_some_and(|request| request.options.input_only) =>
            {
                (480, 140)
            }
            Page::Picker => (560, 336),
            Page::Menu => (480, 336),
            Page::Keybindings | Page::Notifications => (560, 336),
            Page::Audio => (600, 366),
            Page::Bluetooth | Page::Wifi => (560, 336),
            Page::Reminder => (480, 150),
            Page::Controls => (560, if self.media_visible { 360 } else { 296 }),
            Page::Themes => (680, 184),
            Page::Wallpapers => (760, 210),
            Page::Clipboard => (760, 448),
            Page::Styles => (600, 210),
        }
    }
    pub fn sync_media(&mut self, data: &Data) {
        self.media_visible =
            self.page == Page::Controls && crate::now_playing::visible(&data.media);
    }
    pub fn media_rect(&self) -> Option<Rect> {
        self.media_visible
            .then_some(Rect::new(20.0, 268.0, 512.0, 56.0))
    }
    pub fn select_current(&mut self) {
        self.selected = self
            .filtered
            .iter()
            .position(|&index| self.entries[index].selected)
            .unwrap_or(0);
        self.cursor.snap(self.selected as f32);
    }
    pub fn filter(&mut self) {
        if !self.remote
            && !self.query.is_empty()
            && self.entries.len() > 100
            && !matches!(self.page, Page::Audio | Page::Clipboard)
        {
            self.filtered.clear();
            self.selected = 0;
            self.cursor.snap(0.0);
            return;
        }
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                if self.provider == "menu/main"
                    && self.query.is_empty()
                    && entry.category == "launcher"
                {
                    return false;
                }
                if self.page == Page::Clipboard && !self.clipboard.accepts(entry) {
                    return false;
                }
                if self.page == Page::Audio
                    && entry.category
                        != [
                            "playback",
                            "recording",
                            "outputs",
                            "inputs",
                            "configuration",
                        ][self.section]
                {
                    return false;
                }
                if self.page == Page::Reminder || self.remote {
                    return true;
                }
                true
            })
            .map(|(i, _)| i)
            .collect();
        if !self.remote && !self.query.is_empty() && self.page != Page::Reminder {
            let order = crate::search::ranked(&self.entries, &self.query);
            self.filtered = order
                .into_iter()
                .filter(|index| self.filtered.contains(index))
                .collect();
        }
        self.selected = 0;
        self.cursor.snap(0.0);
    }
    pub fn navigate(&mut self, amount: i32) {
        self.selected = (self.selected as i32 + amount)
            .clamp(0, self.filtered.len().saturating_sub(1) as i32)
            as usize;
        if self.page.gallery() {
            self.cursor.target = self.selected as f32;
        } else {
            self.cursor.snap(self.selected as f32);
        }
    }
    pub fn half_page(&self) -> i32 {
        match self.page {
            Page::Controls => 8,
            Page::Clipboard => 4,
            page if page.gallery() => 2,
            _ => 3,
        }
    }
    pub fn command(&self) -> Option<Vec<String>> {
        if let Some(request) = &self.picker {
            return if request.options.input_only {
                Some(vec!["picker-choose".into(), self.query.clone()])
            } else {
                self.filtered
                    .get(self.selected)
                    .map(|&index| vec!["picker-choose".into(), self.entries[index].name.clone()])
            };
        }
        if self.page == Page::Reminder {
            let (minutes, message) = self.query.trim().split_once(char::is_whitespace)?;
            let minutes = minutes
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0 && *value <= 525600)?;
            if message.trim().is_empty() {
                return None;
            }
            return Some(vec![
                "reminder".into(),
                minutes.to_string(),
                message.trim().into(),
            ]);
        }
        self.filtered
            .get(self.selected)
            .map(|&i| self.entries[i].command.clone())
    }
    pub fn finish_query_action(&mut self, success: bool) {
        if let Some(submitted) = self.clear_after_action.take()
            && success
            && self.query == submitted
        {
            self.query.clear();
        }
    }
    pub fn visible_images(&self) -> Vec<(String, ImageKind)> {
        let start = if self.page.gallery() {
            self.selected.saturating_sub(3)
        } else if self.page == Page::Clipboard {
            if self.clipboard.preview {
                self.selected
            } else {
                (self.selected / 4).saturating_sub(1) * 4
            }
        } else {
            self.selected.saturating_sub(5)
        };
        let mut images: Vec<_> = self
            .filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(if self.page == Page::Clipboard {
                if self.clipboard.preview { 1 } else { 8 }
            } else {
                9
            })
            .filter_map(|(position, &index)| {
                let path = &self.entries[index].image;
                (!path.is_empty()).then(|| {
                    (
                        position.abs_diff(self.selected),
                        path.clone(),
                        match self.page {
                            Page::Wallpapers => ImageKind::Wallpaper,
                            Page::Clipboard => ImageKind::Clipboard,
                            _ => ImageKind::Icon,
                        },
                    )
                })
            })
            .collect();
        images.sort_by_key(|(distance, _, _)| *distance);
        images
            .into_iter()
            .map(|(_, path, wallpaper)| (path, wallpaper))
            .collect()
    }
    pub fn hit(&self, x: f32, y: f32, width: f32) -> Option<usize> {
        if self.page == Page::Clipboard {
            if self.clipboard.preview {
                return None;
            }
            crate::clipboard::hit(x, y, self.selected).filter(|&index| index < self.filtered.len())
        } else if self.page.gallery() {
            if !(60.0..self.size().1 as f32 - 38.0).contains(&y) {
                return None;
            }
            let step = if self.page == Page::Styles {
                250.0
            } else {
                180.0
            };
            let index = if self.page == Page::Styles {
                ((x - (width - 484.0) / 2.0) / 250.0).floor() as i32
            } else {
                ((x - width / 2.0) / step + self.cursor.value + 0.5).floor() as i32
            };
            (index >= 0 && (index as usize) < self.filtered.len()).then_some(index as usize)
        } else if self.page == Page::Controls {
            if !(20.0..width - 20.0).contains(&x) || !(62.0..266.0).contains(&y) {
                return None;
            }
            let start = (self.selected / 4).saturating_sub(2) * 4;
            let index = start
                + ((y - 62.0) / 68.0) as usize * 4
                + ((x - 20.0) / ((width - 40.0) / 4.0)) as usize;
            (index < self.filtered.len()).then_some(index)
        } else {
            let top = if self.page == Page::Audio { 90.0 } else { 60.0 };
            if !(top..top + 252.0).contains(&y) {
                return None;
            }
            let index = ((y - top) / 42.0) as usize + self.selected.saturating_sub(5);
            (index < self.filtered.len()).then_some(index)
        }
    }
    pub fn tab(x: f32, y: f32, width: f32) -> Option<Page> {
        let left = width - 16.0 - Page::ALL.len() as f32 * 28.0;
        if !(14.0..44.0).contains(&y) || !(left..width - 16.0).contains(&x) {
            return None;
        }
        Page::ALL.get(((x - left) / 28.0).floor() as usize).copied()
    }
    pub fn draw(
        &self,
        p: &mut tiny_skia::Pixmap,
        text: &mut Text,
        data: &Data,
        t: &Theme,
        s: f32,
        opacity: f32,
    ) {
        let (width, height) = self.size();
        let (w, h) = (width as f32, height as f32);
        let fg = t.text().alpha(opacity);
        let dim = t.dim().alpha(opacity);
        let tile_widths: Vec<_> = if self.page == Page::Controls {
            self.entries
                .iter()
                .map(|entry| {
                    let icon = if entry.icon.is_empty() {
                        "󰒓"
                    } else {
                        &entry.icon
                    };
                    let name: String = entry.name.chars().take(16).collect();
                    (
                        text.width(icon, 21.0 * s) / s,
                        text.width(&name, 10.0 * s) / s,
                    )
                })
                .collect()
        } else {
            vec![]
        };
        let clip = Rect::new(20.0, 8.0, w - 40.0, h - 16.0).scale(s);
        let mut line =
            |p: &mut tiny_skia::Pixmap, value: &str, x: f32, y: f32, size: f32, color: Color| {
                text.draw_clipped(p, value, x * s, y * s, size * s, color, clip)
            };
        line(p, "", 26.0, 34.0, 13.0, dim);
        let search = if self.query.is_empty() && self.page == Page::Reminder {
            "Minutes and message…".into()
        } else if self.query.is_empty() && self.page == Page::Picker {
            format!("{}…", self.title)
        } else if self.query.is_empty() && self.page == Page::Controls && !self.search_focused {
            crate::modules::clock::format("%a, %d %b")
        } else if self.query.is_empty() {
            format!("Search {}…", self.title.to_lowercase())
        } else {
            self.query.clone()
        };
        line(
            p,
            &search,
            50.0,
            34.0,
            13.0,
            if self.query.is_empty() { dim } else { fg },
        );
        if self
            .picker
            .as_ref()
            .is_some_and(|request| request.options.input_only)
        {
            return;
        }
        if self.page == Page::Audio {
            for (i, label) in ["Playback", "Recording", "Outputs", "Inputs", "Profiles"]
                .iter()
                .enumerate()
            {
                let x = 24.0 + i as f32 * 110.0;
                if self.section == i {
                    draw::rounded(
                        p,
                        Rect::new(x - 4.0, 53.0, 102.0, 26.0).scale(s),
                        7.0 * s,
                        fg.alpha(0.1),
                    );
                }
                line(
                    p,
                    label,
                    x,
                    70.0,
                    10.0,
                    if self.section == i { fg } else { dim },
                );
            }
        }
        for (i, page) in Page::ALL.iter().enumerate() {
            let x = w - 16.0 - Page::ALL.len() as f32 * 28.0 + i as f32 * 28.0;
            if *page == self.page {
                draw::rounded(
                    p,
                    Rect::new(x, 14.0, 26.0, 28.0).scale(s),
                    9.0 * s,
                    t.accent().alpha(0.16 * opacity),
                );
            }
            let icon = match page {
                Page::Apps => "󰀻",
                Page::Themes => "󰏘",
                Page::Wallpapers => "󰸉",
                Page::Clipboard => "",
                Page::Controls => "󰒓",
                _ => "󰕮",
            };
            line(
                p,
                icon,
                x + 6.0,
                33.0,
                13.0,
                if *page == self.page { fg } else { dim },
            );
        }
        if self.loading {
            line(p, "Loading…", 26.0, 98.0, 13.0, dim);
        } else if self.filtered.is_empty() {
            line(
                p,
                if self.error.is_empty() {
                    "No matches"
                } else {
                    &self.error
                },
                26.0,
                98.0,
                13.0,
                dim,
            );
        }
        if self.page == Page::Clipboard {
            crate::clipboard::draw(
                &self.clipboard,
                &self.entries,
                &self.filtered,
                self.selected,
                &self.images,
                p,
                text,
                t,
                s,
                opacity,
            );
            return;
        }
        if self.page.gallery() {
            let card_width = if self.page == Page::Styles {
                234.0
            } else {
                164.0
            };
            let step = card_width + 16.0;
            let card_height = if self.page == Page::Themes {
                86.0
            } else {
                110.0
            };
            for (position, &index) in self.filtered.iter().enumerate() {
                let x = if self.page == Page::Styles {
                    (w - 484.0) / 2.0 + position as f32 * step
                } else {
                    w / 2.0 + (position as f32 - self.cursor.value) * step - card_width / 2.0
                };
                if x + card_width < 18.0 || x > w - 18.0 {
                    continue;
                }
                let entry = &self.entries[index];
                let selected = position == self.selected;
                let card = Rect::new(x, 60.0, card_width, card_height).scale(s);
                draw::rounded(
                    p,
                    card,
                    13.0 * s,
                    fg.alpha(if selected { 0.14 } else { 0.06 }),
                );
                if selected {
                    draw::outline(p, card, 13.0 * s, 1.5 * s, t.accent().alpha(opacity));
                }
                if self.page == Page::Wallpapers {
                    if let Some(image) = self.images.get(&entry.image) {
                        let transform = tiny_skia::Transform::from_scale(
                            (card_width - 4.0) * s / image.width() as f32,
                            82.0 * s / image.height() as f32,
                        )
                        .post_translate((x + 2.0) * s, 62.0 * s);
                        p.draw_pixmap(
                            0,
                            0,
                            image.as_ref().as_ref(),
                            &tiny_skia::PixmapPaint {
                                opacity,
                                ..Default::default()
                            },
                            transform,
                            None,
                        );
                    } else {
                        line(p, "󰸉", x + card_width / 2.0 - 12.0, 115.0, 24.0, dim);
                    }
                } else if self.page == Page::Themes {
                    for (i, color) in entry.colors.iter().take(6).enumerate() {
                        draw::rounded(
                            p,
                            Rect::new(x + 21.0 + i as f32 * 21.0, 86.0, 13.0, 13.0).scale(s),
                            6.5 * s,
                            Color::parse(color).unwrap_or(t.accent()).alpha(opacity),
                        );
                    }
                } else {
                    let preview = Rect::new(x + 15.0, 76.0, card_width - 30.0, 56.0).scale(s);
                    draw::rounded(p, preview, 7.0 * s, fg.alpha(0.07));
                    if entry.id == "native-motion" {
                        draw::rounded(
                            p,
                            Rect::new(x + 69.0, 76.0, 96.0, 17.0).scale(s),
                            7.0 * s,
                            fg.alpha(0.65),
                        );
                    } else {
                        for (dx, cw) in [(23.0, 45.0), (84.0, 64.0), (164.0, 45.0)] {
                            draw::rounded(
                                p,
                                Rect::new(x + dx, 82.0, cw, 12.0).scale(s),
                                6.0 * s,
                                fg.alpha(0.50),
                            );
                        }
                    }
                }
                let name: String = entry
                    .name
                    .chars()
                    .take(if self.page == Page::Styles { 26 } else { 19 })
                    .collect();
                line(p, &name, x + 14.0, 60.0 + card_height - 13.0, 11.0, fg);
                if entry.selected {
                    line(
                        p,
                        "●",
                        x + card_width - 17.0,
                        82.0,
                        8.0,
                        t.accent().alpha(opacity),
                    );
                }
            }
        } else if self.page == Page::Controls {
            let start = (self.selected / 4).saturating_sub(2) * 4;
            let step = (w - 40.0) / 4.0;
            for (position, &index) in self.filtered.iter().enumerate().skip(start).take(12) {
                let entry = &self.entries[index];
                let x = 20.0 + ((position - start) % 4) as f32 * step;
                let y = 62.0 + ((position - start) / 4) as f32 * 68.0;
                let selected = position == self.selected;
                draw::rounded(
                    p,
                    Rect::new(x, y, step - 8.0, 60.0).scale(s),
                    11.0 * s,
                    if selected {
                        t.accent().alpha(0.19 * opacity)
                    } else {
                        fg.alpha(0.045)
                    },
                );
                let icon = if entry.icon.is_empty() {
                    "󰒓"
                } else {
                    &entry.icon
                };
                let (icon_width, name_width) = tile_widths[index];
                line(
                    p,
                    icon,
                    x + (step - 8.0 - icon_width) / 2.0,
                    y + 28.0,
                    21.0,
                    if selected {
                        t.accent().alpha(opacity)
                    } else {
                        fg
                    },
                );
                let name: String = entry.name.chars().take(16).collect();
                line(
                    p,
                    &name,
                    x + (step - 8.0 - name_width) / 2.0,
                    y + 48.0,
                    10.0,
                    if selected { fg } else { dim },
                );
            }
        } else {
            let start = self.selected.saturating_sub(5);
            for (position, &index) in self.filtered.iter().enumerate().skip(start).take(6) {
                let entry = &self.entries[index];
                let y = (if self.page == Page::Audio { 90.0 } else { 60.0 })
                    + (position - start) as f32 * 42.0;
                if position == self.selected {
                    draw::rounded(
                        p,
                        Rect::new(18.0, y, w - 36.0, 40.0).scale(s),
                        10.0 * s,
                        fg.alpha(0.13),
                    );
                    draw::rounded(
                        p,
                        Rect::new(19.0, y + 11.0, 2.0, 18.0).scale(s),
                        s,
                        t.accent().alpha(opacity),
                    );
                }
                draw::rounded(
                    p,
                    Rect::new(28.0, y + 5.0, 30.0, 30.0).scale(s),
                    8.0 * s,
                    fg.alpha(0.07),
                );
                if let Some(image) = self.images.get(&entry.image) {
                    let transform = tiny_skia::Transform::from_scale(
                        23.0 * s / image.width() as f32,
                        23.0 * s / image.height() as f32,
                    )
                    .post_translate(31.5 * s, (y + 8.5) * s);
                    p.draw_pixmap(
                        0,
                        0,
                        image.as_ref().as_ref(),
                        &tiny_skia::PixmapPaint {
                            opacity,
                            ..Default::default()
                        },
                        transform,
                        None,
                    );
                } else {
                    line(
                        p,
                        if entry.icon.is_empty() {
                            if self.page == Page::Menu {
                                entry
                                    .name
                                    .split_once("  ")
                                    .map(|(icon, _)| icon)
                                    .unwrap_or("󰀻")
                            } else {
                                "󰀻"
                            }
                        } else {
                            &entry.icon
                        },
                        36.0,
                        y + 25.0,
                        13.0,
                        dim,
                    );
                }
                let display = if self.page == Page::Menu {
                    entry
                        .name
                        .split_once("  ")
                        .map(|(_, name)| name)
                        .unwrap_or(&entry.name)
                } else {
                    &entry.name
                };
                let name: String = display.chars().take(48).collect();
                let detail: String = entry.detail.chars().take(58).collect();
                if entry.level.is_none() {
                    line(p, &name, 71.0, y + 17.0, 12.0, fg);
                }
                if let Some(level) = entry.level {
                    line(
                        p,
                        &name.chars().take(30).collect::<String>(),
                        71.0,
                        y + 17.0,
                        12.0,
                        fg,
                    );
                    line(
                        p,
                        if entry.category != "outputs" && entry.category != "inputs" {
                            "Enter mute · ←/→ level"
                        } else if entry.selected {
                            "Default · Ctrl Enter switches"
                        } else {
                            "Enter mute · Ctrl Enter switches"
                        },
                        71.0,
                        y + 32.0,
                        9.0,
                        dim,
                    );
                    let x = w - 178.0;
                    draw::rounded(
                        p,
                        Rect::new(x, y + 23.0, 112.0, 4.0).scale(s),
                        2.0 * s,
                        fg.alpha(0.15),
                    );
                    draw::rounded(
                        p,
                        Rect::new(x, y + 23.0, 112.0 * level.min(100) as f32 / 100.0, 4.0).scale(s),
                        2.0 * s,
                        if entry.muted { dim } else { t.accent() },
                    );
                    line(p, &format!("{level}%"), w - 58.0, y + 28.0, 10.0, dim);
                } else {
                    line(p, &detail, 71.0, y + 32.0, 10.0, dim);
                }
            }
        }
        if self.page == Page::Controls {
            let cpu = data.cpu.first().copied().unwrap_or(0.0);
            let memory = data
                .memory
                .as_ref()
                .map(|m| format!("{:.1}/{:.0}G", m.used, m.total))
                .unwrap_or_default();
            let storage = data
                .disk
                .as_ref()
                .map(|disk| {
                    format!(
                        "{:.0}/{:.0}G",
                        disk.total * disk.used_percent / 100.0,
                        disk.total
                    )
                })
                .unwrap_or_else(|| "—".into());
            line(
                p,
                &format!("CPU {cpu:.0}%   Memory {memory}   Storage {storage}"),
                26.0,
                h - 18.0,
                10.0,
                dim,
            );
        }
        if !self.error.is_empty() && !self.filtered.is_empty() && self.page != Page::Controls {
            let error: String = self
                .error
                .chars()
                .take(((w - 120.0) / 7.0) as usize)
                .collect();
            line(p, &error, 26.0, h - 18.0, 10.0, dim);
        }
        line(
            p,
            &format!(
                "{}/{}",
                if self.filtered.is_empty() {
                    0
                } else {
                    self.selected + 1
                },
                self.filtered.len()
            ),
            w - 72.0,
            h - 18.0,
            10.0,
            dim,
        );
        if let Some(rect) = self.media_rect() {
            crate::now_playing::draw(p, text, &data.media, rect, fg, s);
        }
    }
}

pub enum Reply {
    Catalog(u64, Result<Vec<Entry>, String>),
    Search(u64, u64, Result<Vec<Entry>, String>),
    Filtered(u64, u64, Vec<usize>),
    Images(u64, Vec<String>, HashMap<String, Arc<tiny_skia::Pixmap>>),
    ClipboardAction(u64, Result<Option<Vec<Entry>>, String>),
    NativeAction(u64, Result<(), String>),
}
#[derive(Clone, Copy)]
pub enum ImageKind {
    Icon,
    Wallpaper,
    Clipboard,
}
enum Job {
    Load(u64, String),
    Images(u64, u64, Vec<(String, ImageKind)>),
    Clear(u64),
}
fn run_native_actions(
    receiver: mpsc::Receiver<(u64, Vec<String>)>,
    mut execute: impl FnMut(u64, Vec<String>) -> bool,
) {
    crate::action_queue::run(
        receiver,
        |(id, previous), (next_id, newer)| {
            id == next_id
                && previous.len() == 4
                && newer.len() == 4
                && previous[0] == "audio"
                && previous[1].ends_with("-volume")
                && previous[..3] == newer[..3]
        },
        |(id, arguments)| execute(id, arguments),
    );
}

pub struct Service {
    applications: crate::applications::Catalog,
    search: crate::search::Service,
    jobs: mpsc::Sender<Job>,
    generation: Arc<AtomicU64>,
    image_request: Arc<AtomicU64>,
    actions: mpsc::Sender<(u64, crate::clipboard::Action)>,
    native_actions: mpsc::Sender<(u64, Vec<String>)>,
}
impl Service {
    pub fn native_action(&self, arguments: Vec<String>, id: u64) {
        let _ = self.native_actions.send((id, arguments));
    }
    pub fn refresh(&self, model: &mut Model) {
        if model.page == Page::Search || (model.page == Page::Apps && !model.query.is_empty()) {
            self.query(model);
            return;
        }
        let _ = self
            .jobs
            .send(Job::Load(model.generation, model.provider.clone()));
    }
    pub fn start(sender: Sender<Update>) -> Self {
        let applications = crate::applications::Catalog::start(sender.clone());
        let (jobs, receiver) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let search = crate::search::Service::start(sender.clone(), generation.clone());
        let current = generation.clone();
        let image_request = Arc::new(AtomicU64::new(0));
        let latest_images = image_request.clone();
        let (native_actions, native_jobs) = mpsc::channel::<(u64, Vec<String>)>();
        let native_sender = sender.clone();
        std::thread::Builder::new()
            .name("bar-desktop-actions".into())
            .spawn(move || {
                let mut desktop = crate::desktop::Service::new(native_sender.clone());
                run_native_actions(native_jobs, |id, arguments| {
                    let result = if arguments
                        .first()
                        .is_some_and(|value| value == "search-activate")
                    {
                        crate::search::activate(&arguments[1..])
                    } else {
                        desktop.execute(&arguments)
                    };
                    if arguments == ["idle"] {
                        return true;
                    }
                    if result.is_ok()
                        && arguments
                            .get(1)
                            .is_some_and(|operation| operation.ends_with("-volume"))
                    {
                        return true;
                    }
                    native_sender
                        .send(Update::Shell(Reply::NativeAction(id, result)))
                        .is_ok()
                });
            })
            .expect("desktop action worker");
        let (actions, action_jobs) = mpsc::channel();
        let action_sender = sender.clone();
        std::thread::Builder::new()
            .name("bar-clipboard-actions".into())
            .spawn(move || {
                while let Ok((id, action)) = action_jobs.recv() {
                    let result = crate::clipboard::execute(action);
                    if action_sender
                        .send(Update::Shell(Reply::ClipboardAction(id, result)))
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .expect("clipboard action worker");
        std::thread::Builder::new()
            .name("bar-catalog".into())
            .spawn(move || {
                let mut cached_generation = 0;
                let mut decoded_images: HashMap<String, Arc<tiny_skia::Pixmap>> = HashMap::new();
                while let Ok(job) = receiver.recv() {
                    match job {
                        Job::Load(id, provider) if id == current.load(Ordering::Relaxed) => {
                            let result: Result<Vec<Entry>, String> = if provider == "clipboard" {
                                crate::clipboard::entries()
                            } else if let Some(result) = crate::desktop::load(&provider, || {
                                id != current.load(Ordering::Relaxed)
                            }) {
                                result
                            } else {
                                crate::process::output(
                                    Command::new("desktopctl").args(["bar", "catalog", &provider]),
                                    std::time::Duration::from_secs(8),
                                    8 * 1024 * 1024,
                                    || id != current.load(Ordering::Relaxed),
                                )
                                .map_err(|error| error.to_string())
                                .and_then(|output| {
                                    if !output.status.success() {
                                        return Err("Catalog unavailable".into());
                                    }
                                    serde_json::from_slice(&output.stdout)
                                        .map_err(|error| error.to_string())
                                })
                            };
                            let _ = sender.send(Update::Shell(Reply::Catalog(id, result)));
                        }
                        Job::Images(id, revision, paths)
                            if id == current.load(Ordering::Relaxed)
                                && revision == latest_images.load(Ordering::Relaxed) =>
                        {
                            let requested: Vec<String> =
                                paths.iter().map(|(path, _)| path.clone()).collect();
                            if cached_generation != id {
                                decoded_images.clear();
                                cached_generation = id;
                            }
                            decoded_images.retain(|path, _| requested.contains(path));
                            let mut images = decoded_images.clone();
                            if !images.is_empty() {
                                let _ = sender.send(Update::Shell(Reply::Images(
                                    id,
                                    requested.clone(),
                                    images.clone(),
                                )));
                            }
                            for (path, kind) in paths.into_iter().take(9) {
                                if id != current.load(Ordering::Relaxed)
                                    || revision != latest_images.load(Ordering::Relaxed)
                                {
                                    break;
                                }
                                if images.contains_key(&path) {
                                    continue;
                                }
                                let image = if matches!(
                                    kind,
                                    ImageKind::Wallpaper | ImageKind::Clipboard
                                ) {
                                    match load_thumbnail(
                                        &path,
                                        matches!(kind, ImageKind::Clipboard),
                                        || {
                                            id != current.load(Ordering::Relaxed)
                                                || revision != latest_images.load(Ordering::Relaxed)
                                        },
                                    ) {
                                        Ok(image) => Some(image),
                                        Err(error) => {
                                            if id == current.load(Ordering::Relaxed)
                                                && revision == latest_images.load(Ordering::Relaxed)
                                            {
                                                eprintln!(
                                                    "desktop-bar: preview unavailable: {error}"
                                                );
                                            }
                                            None
                                        }
                                    }
                                } else {
                                    load_icon(&path)
                                };
                                if let Some(image) = image {
                                    let image = Arc::new(image);
                                    decoded_images.insert(path.clone(), image.clone());
                                    images.insert(path, image);
                                    let _ = sender.send(Update::Shell(Reply::Images(
                                        id,
                                        requested.clone(),
                                        images.clone(),
                                    )));
                                }
                            }
                            let _ =
                                sender.send(Update::Shell(Reply::Images(id, requested, images)));
                        }
                        Job::Clear(id) if id == current.load(Ordering::Relaxed) => {
                            decoded_images.clear();
                            crate::desktop::release_connections();
                            cached_generation = id;
                        }
                        _ => {}
                    }
                }
            })
            .expect("catalog worker");
        Self {
            applications,
            search,
            jobs,
            generation,
            image_request,
            actions,
            native_actions,
        }
    }
    pub fn load(&self, page: Page) -> u64 {
        self.provider(page.provider())
    }
    pub fn provider(&self, provider: &str) -> u64 {
        let id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.search.cancel();
        if provider == "apps" {
            self.applications.request(id);
        } else if !provider.is_empty()
            && !provider.starts_with("search/")
            && !provider.starts_with("actions/")
        {
            let _ = self.jobs.send(Job::Load(id, provider.into()));
        }
        id
    }
    pub fn populate(&self, model: &mut Model) {
        if model.page == Page::Apps
            && let Some(entries) = self.applications.cached()
        {
            model.entries = entries;
            model.loading = false;
            model.refreshing = true;
            model.filter();
            model.select_current();
            self.images(model);
        }
    }
    pub fn cancel(&self) {
        self.search.cancel();
        self.native_action(vec!["idle".into()], 0);
        let id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.jobs.send(Job::Clear(id));
    }
    pub fn query(&self, model: &mut Model) {
        if model.page != Page::Apps && model.page != Page::Search {
            if !model.query.is_empty()
                && model.entries.len() > 100
                && !matches!(model.page, Page::Audio | Page::Clipboard)
            {
                model.loading = true;
                model.revision =
                    self.search
                        .filter(model.generation, &model.query, &model.entries, false);
            }
            return;
        }
        model.remote = false;
        model.error.clear();
        if model.page == Page::Apps {
            if let Some(entries) = self.applications.cached() {
                model.entries = entries;
            }
            model.filter();
            if model.query.is_empty() {
                self.search.cancel();
                model.loading = false;
                return;
            }
        } else {
            model.entries.clear();
            model.filter();
        }
        let request = crate::search::Request::launcher(
            model.provider.strip_prefix("search/").unwrap_or("apps"),
            &model.query,
        );
        if model.page == Page::Apps && request.providers == ["desktopapplications", "websearch"] {
            model.loading = model.filtered.is_empty();
            if self.applications.cached().is_some() {
                model.revision =
                    self.search
                        .filter(model.generation, &model.query, &model.entries, true);
            }
            return;
        }
        if model.page == Page::Apps && request.providers != ["desktopapplications", "websearch"] {
            model.entries.clear();
            model.filter();
        }
        model.loading = model.entries.is_empty();
        model.revision = self.search.request(model.generation, request);
    }
    pub fn resume(&self, model: &mut Model) {
        model.generation = self.provider(&model.provider);
        model.loading = model.entries.is_empty() && !model.provider.is_empty();
        model.refreshing = !model.loading;
        if model.page == Page::Search || (model.page == Page::Apps && !model.query.is_empty()) {
            self.query(model);
        }
    }
    pub fn clipboard_action(&self, action: crate::clipboard::Action) {
        let _ = self
            .actions
            .send((self.generation.load(Ordering::Relaxed), action));
    }
    pub fn images(&self, model: &mut Model) {
        let paths = model.visible_images();
        let requested: Vec<_> = paths.iter().map(|(path, _)| path.clone()).collect();
        if requested == model.requested {
            return;
        }
        model.requested = requested;
        let revision = self.image_request.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self
            .jobs
            .send(Job::Images(model.generation, revision, paths));
    }
}
fn load_thumbnail(
    path: &str,
    fit: bool,
    cancelled: impl FnMut() -> bool,
) -> Result<tiny_skia::Pixmap, String> {
    if let Some(image) = crate::thumbnail::cached(path, fit) {
        return Ok(image);
    }
    let mut command = Command::new("desktopctl");
    command.args(["bar", "thumbnail", path]);
    if fit {
        command.arg("fit");
    }
    let output = crate::process::output(
        &mut command,
        std::time::Duration::from_secs(8),
        64 * 1024,
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    crate::assets::png(String::from_utf8_lossy(&output.stdout).trim())
        .ok_or_else(|| "Invalid preview image".into())
}
fn load_icon(path: &str) -> Option<tiny_skia::Pixmap> {
    let bytes = crate::assets::read(path)?;
    if path.ends_with(".svg") {
        let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default()).ok()?;
        let mut pixmap = tiny_skia::Pixmap::new(48, 48)?;
        let size = tree.size();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(48.0 / size.width(), 48.0 / size.height()),
            &mut pixmap.as_mut(),
        );
        Some(pixmap)
    } else {
        let source = crate::assets::decode_png(&bytes)?;
        let (w, h) = (source.width(), source.height());
        let mut icon = tiny_skia::Pixmap::new(48, 48)?;
        let scale = (48.0 / w as f32).min(48.0 / h as f32);
        icon.draw_pixmap(
            0,
            0,
            source.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::from_scale(scale, scale).post_translate(
                (48.0 - w as f32 * scale) / 2.0,
                (48.0 - h as f32 * scale) / 2.0,
            ),
            None,
        );
        Some(icon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_slider_bursts_coalesce_after_barriers_and_preserve_generations() {
        let (sender, receiver) = mpsc::channel();
        let action = |id, operation: &str, target: &str, value: &str| {
            (
                id,
                vec![
                    "audio".into(),
                    operation.into(),
                    target.into(),
                    value.into(),
                ],
            )
        };
        let first = action(1, "outputs-volume", "1:2", "10");
        let latest = action(1, "outputs-volume", "1:2", "20");
        let mute = action(1, "outputs-mute", "1:2", "true");
        let second = action(1, "inputs-volume", "2:1", "10");
        let second_latest = action(1, "inputs-volume", "2:1", "30");
        let next_generation = action(2, "inputs-volume", "2:1", "40");
        let next_latest = action(2, "inputs-volume", "2:1", "70");
        let radio_on = (
            2,
            vec!["wifi".into(), "/test".into(), "power".into(), "true".into()],
        );
        let radio_off = (
            2,
            vec![
                "wifi".into(),
                "/test".into(),
                "power".into(),
                "false".into(),
            ],
        );
        for item in [
            first,
            latest.clone(),
            mute.clone(),
            second,
            second_latest.clone(),
            next_generation,
            next_latest.clone(),
            radio_on.clone(),
            radio_off.clone(),
        ] {
            sender.send(item).unwrap();
        }
        drop(sender);
        let mut applied = vec![];
        run_native_actions(receiver, |id, args| {
            applied.push((id, args));
            true
        });
        assert_eq!(
            applied,
            [
                latest,
                mute,
                second_latest,
                next_latest,
                radio_on,
                radio_off
            ]
        );
    }
    #[test]
    fn action_acknowledgements_preserve_new_input_and_failed_submissions() {
        let mut model = Model::new(Page::Search, 1, &Theme::default());
        model.query = "submitted".into();
        model.clear_after_action = Some(model.query.clone());
        model.query = "new input".into();
        model.finish_query_action(true);
        assert_eq!(model.query, "new input");
        model.clear_after_action = Some(model.query.clone());
        model.finish_query_action(false);
        assert_eq!(model.query, "new input");
        model.clear_after_action = Some(model.query.clone());
        model.finish_query_action(true);
        assert!(model.query.is_empty());
    }
    #[test]
    fn search_and_navigation_keep_actions_attached_to_entries() {
        let mut model = Model::new(Page::Controls, 1, &Theme::default());
        model.query = "wall".into();
        model.filter();
        model.navigate(9);
        assert_eq!(model.filtered.len(), 1);
        assert_eq!(
            model.command(),
            Some(vec!["panel".into(), "wallpapers".into()])
        );
        model.query = "no results".into();
        model.filter();
        model.navigate(-1);
        assert_eq!(model.command(), None);
    }
}
