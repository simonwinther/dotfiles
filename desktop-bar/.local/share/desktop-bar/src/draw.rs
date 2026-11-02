use crate::theme::Color;
use tiny_skia::{Paint, PathBuilder, Pixmap, Rect, Transform};

pub fn notch_path(r: crate::layout::Rect, shoulder: f32, radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x, r.y, r.w, r.h);
    let a = shoulder.min(w / 4.0).min(h / 2.0);
    let b = radius.min(h / 2.0).min(w / 4.0);
    let mut path = PathBuilder::new();
    path.move_to(x - a, y);
    path.line_to(x + w + a, y);
    path.cubic_to(x + w + a * 0.45, y, x + w, y + a * 0.45, x + w, y + a);
    path.line_to(x + w, y + h - b);
    path.cubic_to(
        x + w,
        y + h - b * 0.45,
        x + w - b * 0.45,
        y + h,
        x + w - b,
        y + h,
    );
    path.line_to(x + b, y + h);
    path.cubic_to(x + b * 0.45, y + h, x, y + h - b * 0.45, x, y + h - b);
    path.line_to(x, y + a);
    path.cubic_to(x, y + a * 0.45, x - a * 0.45, y, x - a, y);
    path.close();
    path.finish()
}

pub fn notch(p: &mut Pixmap, r: crate::layout::Rect, s: f32, color: Color) {
    // Only the outer ring is visible beneath the opaque body. Stroking it
    // avoids blending the entire panel four times on every animation frame.
    for (spread, opacity) in [(10.0, 0.02), (7.0, 0.035), (4.0, 0.06), (2.0, 0.10)] {
        outline(
            p,
            crate::layout::Rect::new(
                r.x - spread * s,
                r.y + 5.0 * s,
                r.w + 2.0 * spread * s,
                r.h + spread * s,
            ),
            (20.0 + spread) * s,
            (spread + 8.0) * s,
            Color::hex("#000000").alpha(opacity),
        );
    }
    if let Some(path) = notch_path(r, 10.0 * s, 20.0 * s) {
        let mut paint = Paint::default();
        paint.set_color(color.skia());
        p.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

pub fn rounded(p: &mut Pixmap, r: crate::layout::Rect, radius: f32, color: Color) {
    if let Some(path) = rounded_path(r, radius) {
        let mut paint = Paint::default();
        paint.set_color(color.skia());
        p.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

pub fn outline(p: &mut Pixmap, r: crate::layout::Rect, radius: f32, width: f32, color: Color) {
    if let Some(path) = rounded_path(r.inset(width / 2.0), radius - width / 2.0) {
        let mut paint = Paint::default();
        paint.set_color(color.skia());
        p.stroke_path(
            &path,
            &paint,
            &tiny_skia::Stroke {
                width,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
}

pub fn gradient(p: &mut Pixmap, r: crate::layout::Rect, radius: f32, from: Color, to: Color) {
    if let Some(path) = rounded_path(r, radius)
        && let Some(shader) = tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(r.x, r.y),
            tiny_skia::Point::from_xy(r.x + r.w, r.y + r.h),
            vec![
                tiny_skia::GradientStop::new(0.0, from.skia()),
                tiny_skia::GradientStop::new(1.0, to.skia()),
            ],
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        )
    {
        let paint = Paint {
            shader,
            ..Default::default()
        };
        p.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

pub fn rounded_path(r: crate::layout::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    if r.w <= 0.0 || r.h <= 0.0 {
        return None;
    }
    let (x, y, w, h) = (r.x, r.y, r.w, r.h);
    let k = radius * 0.5522848;
    let mut b = PathBuilder::new();
    b.move_to(x + radius, y);
    b.line_to(x + w - radius, y);
    b.cubic_to(
        x + w - radius + k,
        y,
        x + w,
        y + radius - k,
        x + w,
        y + radius,
    );
    b.line_to(x + w, y + h - radius);
    b.cubic_to(
        x + w,
        y + h - radius + k,
        x + w - radius + k,
        y + h,
        x + w - radius,
        y + h,
    );
    b.line_to(x + radius, y + h);
    b.cubic_to(
        x + radius - k,
        y + h,
        x,
        y + h - radius + k,
        x,
        y + h - radius,
    );
    b.line_to(x, y + radius);
    b.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    b.close();
    b.finish()
}

pub fn clear(p: &mut Pixmap, r: crate::layout::Rect) {
    if let Some(r) = Rect::from_xywh(r.x, r.y, r.w, r.h) {
        let paint = Paint {
            blend_mode: tiny_skia::BlendMode::Clear,
            ..Default::default()
        };
        p.fill_rect(r, &paint, Transform::identity(), None);
    }
}

pub fn blend(p: &mut Pixmap, x: i32, y: i32, coverage: f32, c: Color) {
    if x < 0 || y < 0 || x >= p.width() as i32 || y >= p.height() as i32 {
        return;
    }
    let i = (y as usize * p.width() as usize + x as usize) * 4;
    let a = (coverage * c.3).clamp(0.0, 1.0);
    let out = &mut p.data_mut()[i..i + 4];
    for (dst, v) in out.iter_mut().zip([c.0, c.1, c.2, 1.0]) {
        *dst = round_byte(v * a * 255.0 + f32::from(*dst) * (1.0 - a));
    }
}

/// Equals `value.round().clamp(0.0, 255.0) as u8` for every input, without
/// the `roundf` library call that baseline x86-64 makes for each channel.
fn round_byte(value: f32) -> u8 {
    if !(value > 0.0) {
        return 0;
    }
    if value >= 255.0 {
        return 255;
    }
    let whole = value as u32;
    (whole + u32::from(value - whole as f32 >= 0.5)).min(255) as u8
}

#[cfg(test)]
mod tests {
    #[test]
    fn byte_rounding_matches_the_standard_library() {
        let mut values = vec![
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            0.5,
            -0.5,
            0.49999997,
            254.5,
            254.49998,
            255.49998,
            1e9,
        ];
        values.extend((-4096..=300 * 4096).map(|n| n as f32 / 4096.0));
        let mut bits = 0x3f00_0000_u32;
        while bits < 0x4380_0000 {
            values.push(f32::from_bits(bits));
            bits += 997;
        }
        for value in values {
            assert_eq!(
                super::round_byte(value),
                value.round().clamp(0.0, 255.0) as u8,
                "{value}"
            );
        }
    }
}
