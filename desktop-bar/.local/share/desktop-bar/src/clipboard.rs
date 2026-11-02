use crate::{draw, layout::Rect, shell::Entry, text::Text, theme::Theme};
use clipboard_history::{Item, Payload};
use smithay_client_toolkit::seat::keyboard::Keysym;
use std::{collections::HashMap, sync::Arc, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Category {
    #[default]
    All,
    Images,
    Text,
}
#[derive(Clone, Debug)]
pub struct State {
    pub category: Category,
    pub searching: bool,
    pub preview: bool,
    pub selected: Vec<String>,
    pub target: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            category: Category::All,
            searching: false,
            preview: false,
            selected: vec![],
            target: String::new(),
        }
    }
}
impl State {
    pub fn cycle(&mut self, backwards: bool) {
        self.category = match (self.category, backwards) {
            (Category::All, false) | (Category::Text, true) => Category::Images,
            (Category::Images, false) | (Category::All, true) => Category::Text,
            _ => Category::All,
        };
    }
    pub fn accepts(&self, entry: &Entry) -> bool {
        match self.category {
            Category::All => true,
            Category::Images => !entry.image.is_empty(),
            Category::Text => entry.image.is_empty(),
        }
    }
    pub fn toggle(&mut self, id: &str) {
        if let Some(index) = self.selected.iter().position(|selected| selected == id) {
            self.selected.remove(index);
        } else {
            self.selected.push(id.into());
        }
    }
    pub fn chosen(&self, entries: &[Entry], focused: Option<&Entry>) -> Vec<Item> {
        if self.selected.is_empty() {
            return focused
                .and_then(|entry| entry.clipboard.clone())
                .into_iter()
                .collect();
        }
        self.selected
            .iter()
            .filter_map(|id| {
                entries
                    .iter()
                    .find(|entry| &entry.id == id)?
                    .clipboard
                    .clone()
            })
            .collect()
    }
}

/// Preview characters that clipboard search matches; ranking reads them from
/// the item instead of storing a copy in the entry.
pub const SEARCH_CHARACTERS: usize = 20000;

pub fn entries() -> Result<Vec<Entry>, String> {
    // The history daemon normally runs; start it only when it cannot answer.
    clipboard_history::history()
        .or_else(|_| {
            let _ = crate::process::output(
                std::process::Command::new("systemctl").args([
                    "--user",
                    "start",
                    "desktop-search.service",
                ]),
                Duration::from_secs(3),
                4096,
                || false,
            );
            clipboard_history::history()
        })
        .map(|items| {
            items
                .into_iter()
                .map(|item| {
                    let image = if item.image() {
                        item.preview.clone()
                    } else {
                        String::new()
                    };
                    let name = if image.is_empty() {
                        item.preview
                            .lines()
                            .next()
                            .filter(|line| !line.is_empty())
                            .unwrap_or(&item.text)
                            .chars()
                            .take(90)
                            .collect()
                    } else {
                        item.text.chars().take(90).collect()
                    };
                    Entry {
                        id: item.identifier.clone(),
                        name,
                        detail: item.subtext.clone(),
                        image,
                        clipboard: Some(item),
                        ..Default::default()
                    }
                })
                .collect()
        })
        .map_err(|_| "Clipboard history unavailable".into())
}

pub enum Action {
    Paste { items: Vec<Item>, target: String },
    Copy(Vec<Item>),
    Files(Vec<Item>),
    Pin(Item),
    Remove(Item),
}

pub enum Input {
    Unhandled,
    Changed,
    Close,
    Reload,
    Action(Action),
}

pub fn key(model: &mut crate::shell::Model, symbol: Keysym, ctrl: bool, shift: bool) -> Input {
    let focused = model
        .filtered
        .get(model.selected)
        .and_then(|&index| model.entries.get(index));
    let item = focused.and_then(|entry| entry.clipboard.clone());
    match symbol {
        Keysym::Escape if model.clipboard.preview => model.clipboard.preview = false,
        Keysym::Escape if model.clipboard.searching => model.clipboard.searching = false,
        Keysym::Escape => return Input::Close,
        Keysym::Return | Keysym::KP_Enter => {
            let items = model.clipboard.chosen(&model.entries, focused);
            if !items.is_empty() {
                return Input::Action(if ctrl && shift {
                    Action::Files(items)
                } else if ctrl {
                    Action::Copy(items)
                } else {
                    Action::Paste {
                        items,
                        target: model.clipboard.target.clone(),
                    }
                });
            }
        }
        Keysym::Tab | Keysym::ISO_Left_Tab if ctrl => {
            model
                .clipboard
                .cycle(shift || symbol == Keysym::ISO_Left_Tab);
            model.filter();
        }
        Keysym::Tab | Keysym::ISO_Left_Tab => return Input::Unhandled,
        Keysym::Down => model.navigate(4),
        Keysym::Up => model.navigate(-4),
        Keysym::Right => model.navigate(1),
        Keysym::Left => model.navigate(-1),
        Keysym::j if ctrl || !model.clipboard.searching => {
            model.clipboard.searching = false;
            model.navigate(4);
        }
        Keysym::k if ctrl || !model.clipboard.searching => {
            model.clipboard.searching = false;
            model.navigate(-4);
        }
        Keysym::h if ctrl || !model.clipboard.searching => {
            model.clipboard.searching = false;
            model.navigate(-1);
        }
        Keysym::l if ctrl || !model.clipboard.searching => {
            model.clipboard.searching = false;
            model.navigate(1);
        }
        Keysym::d | Keysym::D if ctrl => model.navigate(model.half_page()),
        Keysym::u | Keysym::U if ctrl => model.navigate(-model.half_page()),
        Keysym::space if ctrl => {
            model.clipboard.preview = !model.clipboard.preview;
            model.clipboard.searching = false;
        }
        Keysym::space if !model.clipboard.searching => {
            if let Some(item) = item {
                model.clipboard.toggle(&item.identifier);
            }
        }
        Keysym::a | Keysym::A if ctrl => {
            model.clipboard.searching = false;
            if shift {
                model.clipboard.selected.clear();
            } else {
                model.clipboard.selected = model
                    .filtered
                    .iter()
                    .map(|&index| model.entries[index].id.clone())
                    .collect();
            }
        }
        Keysym::p if ctrl => {
            if let Some(item) = item {
                return Input::Action(Action::Pin(item));
            }
        }
        Keysym::p if !model.clipboard.searching => {
            model.clipboard.preview = !model.clipboard.preview;
        }
        Keysym::Delete if !model.clipboard.searching => {
            if let Some(item) = item {
                return Input::Action(Action::Remove(item));
            }
        }
        Keysym::i if ctrl => {
            model.clipboard.category = Category::Images;
            model.filter();
        }
        Keysym::t if ctrl => {
            model.clipboard.category = Category::Text;
            model.filter();
        }
        Keysym::r if ctrl => return Input::Reload,
        Keysym::slash | Keysym::i if !model.clipboard.searching => model.clipboard.searching = true,
        _ if !model.clipboard.searching && !matches!(symbol, Keysym::Home | Keysym::End) => {
            return Input::Changed;
        }
        _ => return Input::Unhandled,
    }
    Input::Changed
}
pub fn execute(action: Action) -> Result<Option<Vec<Entry>>, String> {
    let run = || -> anyhow::Result<Option<Vec<Entry>>> {
        match action {
            Action::Paste { items, target } => {
                let payloads = clipboard_history::prepare_batch(&items)?;
                clipboard_history::paste_batch(
                    &mut clipboard_history::Hyprland,
                    &target,
                    &payloads,
                    Duration::from_millis(800),
                )?;
            }
            Action::Copy(items) => {
                let payload = if items.len() == 1 {
                    items[0].payload()?
                } else if items.iter().all(Item::image) {
                    clipboard_history::export_files(&items)?
                } else {
                    let payloads = clipboard_history::prepare_batch(&items)?;
                    anyhow::ensure!(
                        payloads
                            .iter()
                            .all(|payload| payload.mime.starts_with("text/plain")),
                        "Copy images separately or use Paste for a mixed selection"
                    );
                    Payload {
                        mime: "text/plain;charset=utf-8".into(),
                        bytes: payloads
                            .iter()
                            .map(|p| String::from_utf8_lossy(&p.bytes))
                            .collect::<Vec<_>>()
                            .join("\n")
                            .into_bytes(),
                    }
                };
                clipboard_history::copy(&payload)?;
            }
            Action::Files(items) => {
                clipboard_history::copy(&clipboard_history::export_files(&items)?)?
            }
            Action::Pin(item) => {
                clipboard_history::activate(&item, if item.pinned() { "unpin" } else { "pin" })?;
                std::thread::sleep(Duration::from_millis(40));
                return entries().map(Some).map_err(anyhow::Error::msg);
            }
            Action::Remove(item) => {
                anyhow::ensure!(!item.pinned(), "Unpin this item before removing it");
                clipboard_history::activate(&item, "remove")?;
                std::thread::sleep(Duration::from_millis(40));
                return entries().map(Some).map_err(anyhow::Error::msg);
            }
        }
        Ok(None)
    };
    run().map_err(|error| error.to_string())
}

pub fn hit(x: f32, y: f32, selected: usize) -> Option<usize> {
    if !(20.0..740.0).contains(&x) || !(88.0..400.0).contains(&y) {
        return None;
    }
    Some(
        (selected / 4).saturating_sub(1) * 4
            + ((y - 88.0) / 156.0) as usize * 4
            + ((x - 20.0) / 180.0) as usize,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    state: &State,
    entries: &[Entry],
    filtered: &[usize],
    selected: usize,
    images: &HashMap<String, Arc<tiny_skia::Pixmap>>,
    p: &mut tiny_skia::Pixmap,
    text: &mut Text,
    theme: &Theme,
    scale: f32,
    opacity: f32,
) {
    let fg = theme.text().alpha(opacity);
    let dim = theme.dim().alpha(opacity);
    let mut line = |p: &mut tiny_skia::Pixmap, value: &str, rect: Rect, size, color| {
        let clip = Rect::new(rect.x, rect.y - size - 3.0, rect.w, size + 7.0).scale(scale);
        text.draw_clipped(
            p,
            value,
            rect.x * scale,
            rect.y * scale,
            size * scale,
            color,
            clip,
        );
    };
    for (index, (category, label)) in [
        (Category::All, "All"),
        (Category::Images, "Images"),
        (Category::Text, "Text"),
    ]
    .iter()
    .enumerate()
    {
        let x = 24.0 + index as f32 * 86.0;
        if state.category == *category {
            draw::rounded(
                p,
                Rect::new(x - 5.0, 50.0, 78.0, 26.0).scale(scale),
                8.0 * scale,
                theme.accent().alpha(0.17),
            );
        }
        line(
            p,
            label,
            Rect::new(x + 4.0, 67.0, 70.0, 20.0),
            11.0,
            if state.category == *category { fg } else { dim },
        );
    }
    let focused = filtered.get(selected).and_then(|&index| entries.get(index));
    if state.preview {
        if let Some(entry) = focused {
            let rect = Rect::new(24.0, 88.0, 712.0, 310.0);
            draw::rounded(p, rect.scale(scale), 12.0 * scale, fg.alpha(0.045));
            if let Some(image) = images.get(&entry.image) {
                image_at(p, image, rect.inset(10.0), scale, opacity);
            } else if let Some(item) = &entry.clipboard {
                let mut row = 0;
                for source in item.preview.lines() {
                    let chars: Vec<_> = source.chars().take(20000).collect();
                    for chunk in chars.chunks(88) {
                        if row >= 14 {
                            break;
                        }
                        let value: String = chunk.iter().collect();
                        line(
                            p,
                            &value,
                            Rect::new(38.0, 110.0 + row as f32 * 20.0, 684.0, 20.0),
                            12.0,
                            fg,
                        );
                        row += 1;
                    }
                    if row >= 14 {
                        break;
                    }
                }
            }
        }
    } else {
        let start = (selected / 4).saturating_sub(1) * 4;
        for (position, &index) in filtered.iter().enumerate().skip(start).take(8) {
            let entry = &entries[index];
            let rect = Rect::new(
                20.0 + ((position - start) % 4) as f32 * 180.0,
                88.0 + ((position - start) / 4) as f32 * 156.0,
                172.0,
                148.0,
            );
            draw::rounded(
                p,
                rect.scale(scale),
                11.0 * scale,
                fg.alpha(if position == selected { 0.11 } else { 0.045 }),
            );
            if position == selected {
                draw::outline(
                    p,
                    rect.scale(scale),
                    11.0 * scale,
                    1.3 * scale,
                    theme.accent(),
                );
            }
            if let Some(image) = images.get(&entry.image) {
                image_at(
                    p,
                    image,
                    Rect::new(rect.x + 6.0, rect.y + 6.0, 160.0, 96.0),
                    scale,
                    opacity,
                );
            } else {
                let value = entry
                    .clipboard
                    .as_ref()
                    .map(|item| item.preview.as_str())
                    .unwrap_or(&entry.name);
                let chars: Vec<_> = value
                    .chars()
                    .take(110)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect();
                for (row, chunk) in chars.chunks(22).take(4).enumerate() {
                    line(
                        p,
                        &chunk.iter().collect::<String>(),
                        Rect::new(
                            rect.x + 10.0,
                            rect.y + 24.0 + row as f32 * 19.0,
                            154.0,
                            20.0,
                        ),
                        11.0,
                        fg,
                    );
                }
            }
            line(
                p,
                &entry.detail.chars().take(24).collect::<String>(),
                Rect::new(rect.x + 9.0, rect.y + 127.0, 154.0, 19.0),
                9.0,
                dim,
            );
            if let Some(order) = state.selected.iter().position(|id| id == &entry.id) {
                draw::rounded(
                    p,
                    Rect::new(rect.x + 144.0, rect.y + 6.0, 22.0, 22.0).scale(scale),
                    8.0 * scale,
                    theme.accent(),
                );
                line(
                    p,
                    &(order + 1).to_string(),
                    Rect::new(rect.x + 150.0, rect.y + 22.0, 18.0, 20.0),
                    11.0,
                    theme.text(),
                );
            } else if entry.clipboard.as_ref().is_some_and(Item::pinned) {
                line(
                    p,
                    "󰐃",
                    Rect::new(rect.x + 148.0, rect.y + 24.0, 18.0, 20.0),
                    13.0,
                    theme.accent(),
                );
            }
        }
    }
    if !state.selected.is_empty() {
        line(
            p,
            &format!("{} selected", state.selected.len()),
            Rect::new(24.0, 429.0, 450.0, 20.0),
            10.0,
            dim,
        );
    }
}

fn image_at(
    p: &mut tiny_skia::Pixmap,
    image: &tiny_skia::Pixmap,
    rect: Rect,
    scale: f32,
    opacity: f32,
) {
    let ratio = (rect.w / image.width() as f32).min(rect.h / image.height() as f32) * scale;
    let transform = tiny_skia::Transform::from_scale(ratio, ratio).post_translate(
        (rect.x * scale + (rect.w * scale - image.width() as f32 * ratio) / 2.0).round(),
        (rect.y * scale + (rect.h * scale - image.height() as f32 * ratio) / 2.0).round(),
    );
    p.draw_pixmap(
        0,
        0,
        image.as_ref(),
        &tiny_skia::PixmapPaint {
            opacity,
            ..Default::default()
        },
        transform,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        shell::{Model, Page},
        theme::Theme,
    };

    fn model() -> Model {
        let mut model = Model::new(Page::Clipboard, 1, &Theme::default());
        model.entries = (0..12)
            .map(|index| Entry {
                id: index.to_string(),
                name: format!("Text {index}"),
                clipboard: Some(Item {
                    identifier: index.to_string(),
                    preview: format!("Text {index}"),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .collect();
        model.clipboard.target = "0x123".into();
        model.filter();
        model
    }

    #[test]
    fn grid_navigation_preserves_ordered_selection_and_original_paste_target() {
        let mut model = model();
        key(&mut model, Keysym::Escape, false, false);
        key(&mut model, Keysym::Down, false, false);
        key(&mut model, Keysym::space, false, false);
        key(&mut model, Keysym::Left, false, false);
        key(&mut model, Keysym::space, false, false);
        assert_eq!(model.clipboard.selected, ["4", "3"]);
        let Input::Action(Action::Paste { items, target }) =
            key(&mut model, Keysym::Return, false, false)
        else {
            panic!("Expected batch paste")
        };
        assert_eq!(
            items
                .iter()
                .map(|item| item.identifier.as_str())
                .collect::<Vec<_>>(),
            ["4", "3"]
        );
        assert_eq!(target, "0x123");
        assert!(matches!(
            key(&mut model, Keysym::Return, true, false),
            Input::Action(Action::Copy(_))
        ));
        assert!(matches!(
            key(&mut model, Keysym::Return, true, true),
            Input::Action(Action::Files(_))
        ));
    }

    #[test]
    fn control_navigation_leaves_search_and_moves_by_rows_and_columns() {
        let mut model = model();
        model.selected = 5;
        for (symbol, selected) in [
            (Keysym::j, 9),
            (Keysym::k, 5),
            (Keysym::h, 4),
            (Keysym::l, 5),
        ] {
            model.clipboard.searching = true;
            assert!(matches!(
                key(&mut model, symbol, false, false),
                Input::Unhandled
            ));
            assert!(model.clipboard.searching);
            key(&mut model, symbol, true, false);
            assert_eq!(model.selected, selected);
            assert!(!model.clipboard.searching);
        }
        assert_eq!(model.clipboard.target, "0x123");
    }

    #[test]
    fn categories_and_preview_stay_inside_the_clipboard_panel() {
        let mut model = model();
        for searching in [true, false] {
            model.clipboard.searching = searching;
            for (symbol, shift) in [(Keysym::Tab, false), (Keysym::ISO_Left_Tab, true)] {
                assert!(matches!(
                    key(&mut model, symbol, false, shift),
                    Input::Unhandled
                ));
                assert_eq!(model.clipboard.category, Category::All);
            }
            key(&mut model, Keysym::Tab, true, false);
            assert_eq!(model.clipboard.category, Category::Images);
            key(&mut model, Keysym::Tab, true, true);
            assert_eq!(model.clipboard.category, Category::All);
            key(&mut model, Keysym::ISO_Left_Tab, true, true);
            assert_eq!(model.clipboard.category, Category::Text);
            key(&mut model, Keysym::Tab, true, false);
            assert_eq!(model.clipboard.category, Category::All);
            assert_eq!(model.clipboard.searching, searching);
        }
        key(&mut model, Keysym::i, true, false);
        assert_eq!(model.clipboard.category, Category::Images);
        assert!(model.filtered.is_empty());
        key(&mut model, Keysym::Tab, true, false);
        assert_eq!(model.filtered.len(), 12);
        key(&mut model, Keysym::space, true, false);
        assert!(model.clipboard.preview && !model.clipboard.searching);
        assert!(matches!(
            key(&mut model, Keysym::Escape, false, false),
            Input::Changed
        ));
        assert!(!model.clipboard.preview);
        assert!(matches!(
            key(&mut model, Keysym::Escape, false, false),
            Input::Close
        ));
        assert_eq!(hit(25.0, 95.0, 8), Some(4));
        assert_eq!(hit(25.0, 251.0, 8), Some(8));
    }
}
