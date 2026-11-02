use crate::{
    draw,
    layout::Rect,
    modules::Data,
    text::Text,
    theme::{Color, Theme},
};

#[derive(Clone, Debug, Default)]
pub struct Overview {
    pub workspaces: Vec<i32>,
    pub active: i32,
}
#[derive(Clone, Debug)]
pub enum Action {
    Panel(&'static str),
    Media(&'static str),
    Workspace(i32),
    Tray(usize),
}
impl Overview {
    pub fn hit(&self, x: f32, y: f32, data: &Data) -> Option<Action> {
        if (88.0..118.0).contains(&y) {
            let panels = [
                "apps",
                "clipboard",
                "notifications",
                "volume",
                "bluetooth",
                "controls",
            ];
            if (214.0..426.0).contains(&x) {
                return panels
                    .get(((x - 214.0) / 36.0) as usize)
                    .copied()
                    .map(Action::Panel);
            }
            for (slot, (index, _)) in data
                .tray
                .iter()
                .enumerate()
                .filter(|(_, item)| !item.passive)
                .enumerate()
            {
                if Rect::new(474.0 + slot as f32 * 28.0, 88.0, 28.0, 30.0).contains(x, y) {
                    return Some(Action::Tray(index));
                }
            }
        }
        if y < 36.0 {
            return Some(Action::Panel("calendar"));
        }
        if (46.0..83.0).contains(&y) {
            if x < 180.0
                && let Some(n) = self.workspaces.get(((x - 28.0).max(0.0) / 18.0) as usize)
            {
                return Some(Action::Workspace(*n));
            }
            if x >= 574.0 {
                return Some(Action::Panel("battery"));
            }
            if x >= 530.0 {
                return Some(Action::Panel("network"));
            }
            if x >= 482.0 {
                return Some(Action::Panel("notifications"));
            }
            return crate::now_playing::hit(&data.media, Rect::new(202.0, 44.0, 259.0, 40.0), x, y)
                .map(Action::Media);
        }
        None
    }
    pub fn draw(&self, p: &mut tiny_skia::Pixmap, text: &mut Text, data: &Data, t: &Theme, s: f32) {
        let fg = t.text();
        let dim = fg.alpha(0.48);
        let time = crate::modules::clock::format("%H:%M");
        let date = crate::modules::clock::format("%a, %b %d");
        let tw = text.width(&time, 18.0 * s);
        let dw = text.width(&date, 11.0 * s);
        text.draw(
            p,
            &time,
            (320.0 * s - (tw + dw + 9.0 * s) / 2.0).round(),
            29.0 * s,
            18.0 * s,
            fg,
        );
        text.draw(
            p,
            &date,
            320.0 * s + (tw - dw + 9.0 * s) / 2.0,
            28.0 * s,
            11.0 * s,
            dim,
        );
        text.draw(p, "Workspaces", 28.0 * s, 51.0 * s, 10.0 * s, dim);
        for (i, &n) in self.workspaces.iter().enumerate() {
            let active = n == self.active;
            draw::rounded(
                p,
                Rect::new(
                    28.0 + i as f32 * 18.0,
                    if active { 61.0 } else { 64.0 },
                    7.0,
                    if active { 17.0 } else { 11.0 },
                )
                .scale(s),
                3.5 * s,
                if active { t.accent() } else { fg.alpha(0.25) },
            );
        }
        crate::now_playing::draw(
            p,
            text,
            &data.media,
            Rect::new(202.0, 44.0, 259.0, 40.0),
            fg,
            s,
        );
        text.draw(p, "󰂚", 490.0 * s, 71.0 * s, 15.0 * s, dim);
        signal(p, Rect::new(540.0, 60.0, 19.0, 12.0).scale(s), data, fg);
        battery(p, Rect::new(582.0, 60.0, 26.0, 13.0).scale(s), data, fg);
        let icons = ["󰀻", "", "󰂚", "", "󰂯", "󰒓"];
        for (i, icon) in icons.iter().enumerate() {
            text.draw(
                p,
                icon,
                (224.0 + i as f32 * 36.0) * s,
                108.0 * s,
                13.0 * s,
                dim,
            );
        }
        for (slot, (_, item)) in data
            .tray
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.passive)
            .take(5)
            .enumerate()
        {
            if let Some(icon) = &item.icon {
                let size = 16.0 * s;
                p.draw_pixmap(
                    0,
                    0,
                    icon.as_ref().as_ref(),
                    &tiny_skia::PixmapPaint {
                        opacity: 0.65,
                        ..Default::default()
                    },
                    tiny_skia::Transform::from_scale(
                        size / icon.width() as f32,
                        size / icon.height() as f32,
                    )
                    .post_translate((480.0 + slot as f32 * 28.0) * s, 94.0 * s),
                    None,
                );
            }
        }
    }
}
pub fn signal(p: &mut tiny_skia::Pixmap, r: Rect, data: &Data, fg: Color) {
    for i in 0..4 {
        let h = r.h * (i as f32 + 1.0) / 4.0;
        draw::rounded(
            p,
            Rect::new(r.x + i as f32 * r.w / 4.0, r.y + r.h - h, r.w / 5.5, h),
            r.w / 15.0,
            fg.alpha(if data.network.connected() && i < data.network.bars() {
                0.85
            } else {
                0.22
            }),
        );
    }
}
pub fn battery(p: &mut tiny_skia::Pixmap, r: Rect, data: &Data, fg: Color) {
    let s = r.h / 13.0;
    draw::rounded(
        p,
        Rect::new(r.x + r.w - 2.0 * s, r.y + 4.0 * s, 2.0 * s, r.h - 8.0 * s),
        s,
        fg.alpha(0.45),
    );
    let shell = Rect::new(r.x, r.y, r.w - 3.0 * s, r.h);
    draw::rounded(p, shell, 3.0 * s, fg.alpha(0.22));
    if let Some(battery) = &data.battery {
        let color = if battery.percent < 15.0 && !battery.charging {
            Color::hex("#e06c75")
        } else {
            fg.alpha(0.85)
        };
        draw::rounded(
            p,
            Rect::new(
                shell.x,
                shell.y,
                (shell.w * battery.percent.clamp(0.0, 100.0) / 100.0).max(2.0 * s),
                shell.h,
            ),
            3.0 * s,
            color,
        );
    }
}
