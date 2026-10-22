use std::f64::consts::{PI, TAU};
pub fn sector_at(
    x: f64,
    y: f64,
    center: (f64, f64),
    inner: f64,
    outer: f64,
    count: usize,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let (x, y) = (x - center.0, y - center.1);
    let distance = x.hypot(y);
    if distance < inner || distance > outer {
        return None;
    }
    let step = TAU / count as f64;
    let angle = (y.atan2(x) + PI / 2.0 + step / 2.0).rem_euclid(TAU);
    Some(((angle / step).floor() as usize).min(count - 1))
}
pub fn center(
    width: f64,
    height: f64,
    radius: f64,
    pointer: Option<(f64, f64)>,
) -> (f64, f64, f64) {
    let r = radius
        .min(width / 2.0 - 16.0)
        .min(height / 2.0 - 56.0)
        .max(80.0);
    let (x, y) = pointer.unwrap_or((width / 2.0, height / 2.0));
    (
        x.clamp(r + 12.0, (width - r - 12.0).max(r + 12.0)),
        y.clamp(r + 12.0, (height - r - 48.0).max(r + 12.0)),
        r,
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn centered(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x: x - width / 2.0,
            y: y - height / 2.0,
            width,
            height,
        }
    }
}

fn fits_sector(rect: Rect, inner: f64, outer: f64, angle: f64, step: f64) -> bool {
    let closest_x = 0.0_f64.clamp(rect.x, rect.x + rect.width);
    let closest_y = 0.0_f64.clamp(rect.y, rect.y + rect.height);
    if closest_x.hypot(closest_y) < inner {
        return false;
    }
    for x in [rect.x, rect.x + rect.width] {
        for y in [rect.y, rect.y + rect.height] {
            if x.hypot(y) > outer {
                return false;
            }
            let delta = (y.atan2(x) - angle + PI).rem_euclid(TAU) - PI;
            if step < TAU - 0.001 && delta.abs() > step / 2.0 - 0.035 {
                return false;
            }
        }
    }
    true
}

pub fn entry_text_bounds(inner: f64, outer: f64, angle: f64, step: f64) -> (Rect, Rect) {
    let unit = (outer / 214.0).min(1.0);
    let safe_inner = inner + 8.0 * unit;
    let safe_outer = outer - 23.0 * unit;
    let mut best = None;
    for index in 0..=24 {
        let radius = inner + (outer - inner) * (0.30 + index as f64 / 60.0);
        let (x, y) = (angle.cos() * radius, angle.sin() * radius);
        let icon = Rect::centered(x, y - 20.0 * unit, 36.0 * unit, 30.0 * unit);
        if !fits_sector(icon, safe_inner, safe_outer, angle, step) {
            continue;
        }
        let mut low = 0.0;
        let mut high = 130.0 * unit;
        for _ in 0..16 {
            let width = (low + high) / 2.0;
            let rect = Rect::centered(x, y + 19.0 * unit, width, 36.0 * unit);
            if fits_sector(rect, safe_inner, safe_outer, angle, step) {
                low = width;
            } else {
                high = width;
            }
        }
        let label = Rect::centered(x, y + 19.0 * unit, low, 36.0 * unit);
        if low > 0.0 && best.is_none_or(|(_, b): (Rect, Rect)| label.width > b.width) {
            best = Some((icon, label));
        }
    }
    best.unwrap_or_else(|| {
        let radius = (inner + outer) / 2.0;
        let p = Rect::centered(angle.cos() * radius, angle.sin() * radius, 0.0, 0.0);
        (p, p)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_label_and_icon_fits_its_sector_including_diagonals() {
        for outer in [80.0, 120.0, 214.0] {
            let unit = (outer / 214.0_f64).min(1.0);
            let inner = outer * 0.39;
            for count in 1..=8 {
                let step = TAU / count as f64;
                for index in 0..count {
                    let angle = -PI / 2.0 + index as f64 * step;
                    let (icon, label) = entry_text_bounds(inner, outer, angle, step);
                    assert!(
                        label.width > 15.0 * unit,
                        "No room for {count} entries at {index}: {label:?}"
                    );
                    assert!(fits_sector(
                        icon,
                        inner + 8.0 * unit,
                        outer - 23.0 * unit,
                        angle,
                        step
                    ));
                    assert!(fits_sector(
                        label,
                        inner + 8.0 * unit,
                        outer - 23.0 * unit,
                        angle,
                        step
                    ));
                }
            }
        }
    }

    #[test]
    fn hit_testing_matches_top_clockwise_sectors() {
        assert_eq!(sector_at(0.0, -100.0, (0.0, 0.0), 50.0, 200.0, 4), Some(0));
        assert_eq!(sector_at(100.0, 0.0, (0.0, 0.0), 50.0, 200.0, 4), Some(1));
        assert_eq!(sector_at(0.0, 0.0, (0.0, 0.0), 50.0, 200.0, 4), None);
        assert_eq!(sector_at(400.0, 0.0, (0.0, 0.0), 50.0, 200.0, 4), None);
    }
    #[test]
    fn ring_stays_inside_scaled_display_edges() {
        let (x, y, r) = center(800.0, 600.0, 235.0, Some((0.0, 599.0)));
        assert!(x - r >= 0.0 && x + r <= 800.0 && y - r >= 0.0 && y + r <= 600.0);
    }
}
