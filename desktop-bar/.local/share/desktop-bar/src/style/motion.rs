use crate::{draw, layout::Rect, theme::Theme};
pub fn group(p: &mut tiny_skia::Pixmap, r: Rect, t: &Theme, s: f32) {
    draw::rounded(
        p,
        Rect::new(r.x - s, r.y + s, r.w + 2.0 * s, r.h + 3.0 * s),
        24.0 * s,
        t.accent().alpha(0.14),
    );
    draw::rounded(p, r, 24.0 * s, t.accent().alpha(0.30));
    draw::rounded(p, r.inset(s), 24.0 * s, t.pill());
}
