use crate::{
    draw,
    layout::Rect,
    theme::{Color, Theme},
};
pub fn group(p: &mut tiny_skia::Pixmap, r: Rect, t: &Theme, s: f32) {
    draw::rounded(
        p,
        Rect::new(r.x, r.y + 2.0 * s, r.w, r.h),
        24.0 * s,
        if t.light {
            Color::hex("#4c4f69").alpha(0.06)
        } else {
            Color::hex("#000000").alpha(0.16)
        },
    );
    draw::rounded(p, r, 24.0 * s, t.pill());
    draw::outline(p, r, 24.0 * s, s, t.outline());
}
