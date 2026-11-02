use crate::{draw, layout::Rect, modules::media::Media, text::Text, theme::Color};

pub fn visible(media: &Media) -> bool {
    media.available() && (media.playing || !media.title.trim().is_empty())
}

pub fn hit(media: &Media, rect: Rect, x: f32, y: f32) -> Option<&'static str> {
    if !visible(media) || !rect.contains(x, y) {
        return None;
    }
    let buttons = rect.x + rect.w - 98.0;
    let (method, enabled) = if x < buttons {
        ("PlayPause", media.can_toggle)
    } else {
        match ((x - buttons) / 30.0) as usize {
            0 => ("Previous", media.can_previous),
            1 => ("PlayPause", media.can_toggle),
            _ => ("Next", media.can_next),
        }
    };
    enabled.then_some(method)
}

pub fn draw(
    p: &mut tiny_skia::Pixmap,
    text: &mut Text,
    media: &Media,
    rect: Rect,
    fg: Color,
    s: f32,
) {
    if !visible(media) {
        return;
    }
    draw::rounded(p, rect.scale(s), 11.0 * s, fg.alpha(0.045));
    let size = (rect.h - 12.0) * s;
    if let Some(art) = &media.art {
        p.draw_pixmap(
            0,
            0,
            art.as_ref().as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::from_scale(size / art.width() as f32, size / art.height() as f32)
                .post_translate((rect.x + 6.0) * s, (rect.y + 6.0) * s),
            None,
        );
    } else {
        text.draw(
            p,
            "󰝚",
            (rect.x + 15.0) * s,
            (rect.y + rect.h / 2.0 + 7.0) * s,
            20.0 * s,
            fg.alpha(0.48),
        );
    }
    let x = rect.x + rect.h + 6.0;
    let buttons = rect.x + rect.w - 98.0;
    let clip = Rect::new(x, rect.y, (buttons - x - 12.0).max(0.0), rect.h).scale(s);
    let title = if media.title.is_empty() {
        &media.identity
    } else {
        &media.title
    };
    let artist = if media.artist.is_empty() {
        &media.identity
    } else {
        &media.artist
    };
    text.draw_clipped(
        p,
        title,
        x * s,
        (rect.y + rect.h / 2.0 - 2.0) * s,
        12.0 * s,
        fg,
        clip,
    );
    text.draw_clipped(
        p,
        artist,
        x * s,
        (rect.y + rect.h / 2.0 + 14.0) * s,
        10.0 * s,
        fg.alpha(0.48),
        clip,
    );
    for (i, (icon, enabled)) in [
        ("󰒮", media.can_previous),
        (if media.playing { "󰏤" } else { "󰐊" }, media.can_toggle),
        ("󰒭", media.can_next),
    ]
    .into_iter()
    .enumerate()
    {
        let tw = text.width(icon, 16.0 * s);
        text.draw(
            p,
            icon,
            (buttons + i as f32 * 30.0 + 15.0) * s - tw / 2.0,
            (rect.y + rect.h / 2.0 + 6.0) * s,
            16.0 * s,
            fg.alpha(if enabled { 0.8 } else { 0.2 }),
        );
    }
}
