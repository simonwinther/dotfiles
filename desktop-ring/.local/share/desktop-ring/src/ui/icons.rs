use super::geometry::Rect;
use gtk::cairo::{Context, LineCap, LineJoin};

// Use consistent outline symbols for the main controls, with font fallback for
// other menu entries. Coordinates share a 32-pixel canvas.
pub fn draw(cr: &Context, symbol: &str, bounds: Rect) -> bool {
    if !matches!(symbol, "󰍹" | "󰕾" | "󰍬" | "󰂯" | "󰒲") {
        return false;
    }
    let _ = cr.save();
    let size = bounds.width.min(bounds.height);
    cr.translate(
        bounds.x + (bounds.width - size) / 2.0,
        bounds.y + (bounds.height - size) / 2.0,
    );
    cr.scale(size / 32.0, size / 32.0);
    cr.set_line_width(1.65);
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);
    cr.new_path();
    match symbol {
        "󰍹" => {
            cr.move_to(4.0, 4.0);
            cr.line_to(28.0, 4.0);
            cr.curve_to(29.1, 4.0, 30.0, 4.9, 30.0, 6.0);
            cr.line_to(30.0, 21.0);
            cr.curve_to(30.0, 22.1, 29.1, 23.0, 28.0, 23.0);
            cr.line_to(4.0, 23.0);
            cr.curve_to(2.9, 23.0, 2.0, 22.1, 2.0, 21.0);
            cr.line_to(2.0, 6.0);
            cr.curve_to(2.0, 4.9, 2.9, 4.0, 4.0, 4.0);
            cr.close_path();
            line(cr, &[(14.0, 23.0), (14.0, 29.0)]);
            line(cr, &[(18.0, 23.0), (18.0, 29.0)]);
            line(cr, &[(10.0, 29.0), (22.0, 29.0)]);
        }
        "󰕾" => {
            line(
                cr,
                &[
                    (3.0, 11.0),
                    (8.0, 11.0),
                    (16.0, 4.0),
                    (16.0, 28.0),
                    (8.0, 21.0),
                    (3.0, 21.0),
                    (3.0, 11.0),
                ],
            );
            cr.move_to(21.0, 11.0);
            cr.curve_to(24.0, 14.0, 24.0, 18.0, 21.0, 21.0);
            cr.move_to(25.0, 7.0);
            cr.curve_to(30.0, 12.0, 30.0, 20.0, 25.0, 25.0);
        }
        "󰍬" => {
            cr.move_to(11.0, 8.0);
            cr.curve_to(11.0, 1.0, 21.0, 1.0, 21.0, 8.0);
            cr.line_to(21.0, 16.0);
            cr.curve_to(21.0, 23.0, 11.0, 23.0, 11.0, 16.0);
            cr.close_path();
            cr.move_to(6.0, 16.0);
            cr.curve_to(6.0, 29.0, 26.0, 29.0, 26.0, 16.0);
            line(cr, &[(16.0, 26.0), (16.0, 31.0)]);
            line(cr, &[(13.0, 31.0), (19.0, 31.0)]);
        }
        "󰂯" => {
            line(
                cr,
                &[
                    (8.0, 9.0),
                    (24.0, 23.0),
                    (16.0, 30.0),
                    (16.0, 2.0),
                    (24.0, 9.0),
                    (8.0, 23.0),
                ],
            );
        }
        "󰒲" => {
            for (x, y, size) in [(2.0, 21.0, 7.0), (13.0, 13.0, 6.0), (23.0, 4.0, 7.0)] {
                line(
                    cr,
                    &[
                        (x, y),
                        (x + size, y),
                        (x, y + size + 2.0),
                        (x + size, y + size + 2.0),
                    ],
                );
            }
        }
        _ => unreachable!(),
    }
    let _ = cr.stroke();
    let _ = cr.restore();
    true
}

fn line(cr: &Context, points: &[(f64, f64)]) {
    cr.move_to(points[0].0, points[0].1);
    for &(x, y) in &points[1..] {
        cr.line_to(x, y);
    }
}
