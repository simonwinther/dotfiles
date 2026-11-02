use crate::{draw, layout::Rect, panel::Kind, shell::Page, text::Text, theme::Theme};

#[derive(Clone, Debug)]
pub struct Model {
    title: String,
    rows: Vec<(&'static str, &'static str)>,
}

impl Model {
    pub fn for_panel(kind: &Kind) -> Option<Self> {
        let (title, rows): (_, Vec<_>) = match kind {
            Kind::Shell(model) => {
                let rows = match model.page {
                    Page::Controls => vec![
                        ("h j k l", "Move between tiles"),
                        ("Ctrl D / U", "Half a page down / up"),
                        ("Enter", "Open selection"),
                        ("/ · Ctrl L", "Search"),
                        ("Tab / Shift Tab", "Change page"),
                        ("Super Ctrl D", "Toggle dashboard"),
                    ],
                    Page::Clipboard if model.clipboard.preview => vec![
                        ("hjkl / Ctrl hjkl", "Move through history"),
                        ("p · Ctrl Space", "Return to grid"),
                        ("Space", "Select item"),
                        ("Enter", "Paste selection"),
                        ("Ctrl Enter", "Copy selection"),
                        ("Esc", "Return to grid"),
                    ],
                    Page::Clipboard => vec![
                        ("hjkl / Ctrl hjkl", "Move between items"),
                        ("Ctrl D / U", "Half a page down / up"),
                        ("p · Ctrl Space", "Preview item"),
                        ("Space", "Select item"),
                        ("Enter / Ctrl Enter", "Paste / copy selection"),
                        ("Ctrl Tab", "Change category"),
                        ("/ · i", "Search"),
                        ("Ctrl P / Delete", "Pin / remove item"),
                    ],
                    Page::Audio => vec![
                        ("Ctrl J / K", "Move down / up"),
                        ("Left / Right", "Adjust volume"),
                        ("Tab / Shift Tab", "Change mixer section"),
                        ("Enter", "Mute or choose profile"),
                        ("Ctrl Enter", "Set default device"),
                        ("Type", "Search this section"),
                    ],
                    Page::Reminder => {
                        vec![("Type", "Minutes and message"), ("Enter", "Set reminder")]
                    }
                    Page::Search | Page::Apps => vec![
                        ("Ctrl J / K", "Move down / up"),
                        ("Ctrl D / U", "Half a page down / up"),
                        ("Type", "Fuzzy search"),
                        ("Enter", "Open selection"),
                        ("Ctrl O", "Item and provider actions"),
                        ("Ctrl Y", "Activate and keep open"),
                    ],
                    page if page.gallery() => vec![
                        ("Left / Right", "Browse cards"),
                        ("Ctrl D / U", "Half a page down / up"),
                        ("Type", "Search"),
                        ("Enter", "Apply selection"),
                        ("Tab / Shift Tab", "Change page"),
                    ],
                    _ => vec![
                        ("Ctrl J / K", "Move down / up"),
                        ("Ctrl D / U", "Half a page down / up"),
                        ("Type", "Search"),
                        ("Enter", "Open selection"),
                        ("Tab / Shift Tab", "Change page"),
                    ],
                };
                let title = if model.page == Page::Controls {
                    "Dashboard".into()
                } else if model.page == Page::Clipboard && model.clipboard.preview {
                    "Clipboard preview".into()
                } else {
                    model.title.clone()
                };
                (title, rows)
            }
            Kind::Volume => (
                "Quick sound".into(),
                vec![
                    ("Drag slider", "Set output or mic volume"),
                    ("Scroll slider", "Adjust volume"),
                    ("Audio devices", "Open the full mixer"),
                ],
            ),
            Kind::Calendar => ("Calendar".into(), vec![("Click arrows", "Change month")]),
            Kind::Network => (
                "Network".into(),
                vec![("Wi-Fi settings", "Open network controls")],
            ),
            Kind::Menu { .. } => (
                "Tray menu".into(),
                vec![
                    ("Ctrl J / K", "Move down / up"),
                    ("Enter", "Open selection"),
                ],
            ),
            Kind::Battery => ("Battery".into(), vec![]),
            Kind::System => ("System".into(), vec![]),
            Kind::Overview(_) => ("Overview".into(), vec![]),
            Kind::Tooltip(_) => ("Details".into(), vec![]),
            Kind::Prompt(_) | Kind::Help(_) => return None,
        };
        Some(Self { title, rows })
    }

    pub fn size(&self) -> (u32, u32) {
        (420, 80 + self.rows.len() as u32 * 26)
    }

    pub fn draw(&self, p: &mut tiny_skia::Pixmap, text: &mut Text, theme: &Theme, s: f32) {
        let fg = theme.text();
        let dim = fg.alpha(0.55);
        text.draw(p, &self.title, 24.0 * s, 30.0 * s, 14.0 * s, fg);
        for (index, (key, action)) in self.rows.iter().enumerate() {
            let y = 48.0 + index as f32 * 26.0;
            draw::rounded(
                p,
                Rect::new(24.0, y, 136.0, 22.0).scale(s),
                5.0 * s,
                fg.alpha(0.065),
            );
            text.draw(p, key, 32.0 * s, (y + 15.0) * s, 10.0 * s, fg);
            text.draw(p, action, 174.0 * s, (y + 15.0) * s, 11.0 * s, dim);
        }
        text.draw(
            p,
            "Esc / ?   Back",
            24.0 * s,
            (self.size().1 as f32 - 16.0) * s,
            10.0 * s,
            dim,
        );
    }
}
