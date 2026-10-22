use crate::model::{Plot, SAMPLES, Viewport};
use anyhow::{Context as _, Result, ensure};
use gtk::cairo::{Context, FontSlant, FontWeight, Format, ImageSurface};
use std::io::Write;
use std::path::Path;

type Point = (f64, f64);
type Color = (f64, f64, f64);
const INK: Color = (0.14, 0.18, 0.23);
pub const COLORS: [Color; 10] = [
    (0.12, 0.47, 0.71),
    (1.0, 0.50, 0.05),
    (0.17, 0.63, 0.17),
    (0.84, 0.15, 0.16),
    (0.58, 0.40, 0.74),
    (0.55, 0.34, 0.29),
    (0.89, 0.47, 0.76),
    (0.50, 0.50, 0.50),
    (0.74, 0.74, 0.13),
    (0.09, 0.75, 0.81),
];

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
    pub fn coordinates(self, view: Viewport, x: f64, y: f64) -> Point {
        (
            view.xmin + (x - self.x) / self.width * (view.xmax - view.xmin),
            view.ymax - (y - self.y) / self.height * (view.ymax - view.ymin),
        )
    }
}
#[derive(Clone, Copy)]
enum Anchor {
    Left,
    Middle,
    Right,
}
enum Primitive {
    Line {
        points: Vec<Point>,
        color: Color,
        width: f64,
        dashed: bool,
    },
    Text {
        point: Point,
        text: String,
        color: Color,
        size: f64,
        anchor: Anchor,
    },
}
pub struct Scene {
    pub rect: Rect,
    width: f64,
    height: f64,
    items: Vec<Primitive>,
}

impl Scene {
    fn line(&mut self, points: Vec<Point>, color: Color, width: f64, dashed: bool) {
        if points.len() > 1 {
            self.items.push(Primitive::Line {
                points,
                color,
                width,
                dashed,
            });
        }
    }
    fn text(&mut self, point: Point, text: impl Into<String>, size: f64, anchor: Anchor) {
        self.items.push(Primitive::Text {
            point,
            text: text.into(),
            color: INK,
            size,
            anchor,
        });
    }
    pub fn draw(&self, context: &Context) -> Result<()> {
        context.set_source_rgb(1.0, 1.0, 1.0);
        context.paint()?;
        context.select_font_face("sans-serif", FontSlant::Normal, FontWeight::Normal);
        context.set_line_join(gtk::cairo::LineJoin::Round);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        for item in &self.items {
            match item {
                Primitive::Line {
                    points,
                    color,
                    width,
                    dashed,
                } => {
                    context.set_source_rgb(color.0, color.1, color.2);
                    context.set_line_width(*width);
                    context.set_dash(if *dashed { &[3.0, 4.0] } else { &[] }, 0.0);
                    context.move_to(points[0].0, points[0].1);
                    for point in &points[1..] {
                        context.line_to(point.0, point.1);
                    }
                    context.stroke()?;
                }
                Primitive::Text {
                    point,
                    text,
                    color,
                    size,
                    anchor,
                } => {
                    context.set_source_rgb(color.0, color.1, color.2);
                    context.set_font_size(*size);
                    let extents = context.text_extents(text)?;
                    let shift = match anchor {
                        Anchor::Left => 0.0,
                        Anchor::Middle => extents.width() / 2.0 + extents.x_bearing(),
                        Anchor::Right => extents.width() + extents.x_bearing(),
                    };
                    context.move_to(point.0 - shift, point.1);
                    context.show_text(text)?;
                }
            }
        }
        Ok(())
    }
    pub fn svg(&self) -> String {
        use std::fmt::Write as _;
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}"><rect width="100%" height="100%" fill="white"/><g font-family="sans-serif" stroke-linejoin="round" stroke-linecap="round">"#,
            self.width, self.height, self.width, self.height
        );
        for item in &self.items {
            match item {
                Primitive::Line {
                    points,
                    color,
                    width,
                    dashed,
                } => {
                    let mut data = String::new();
                    for (i, (x, y)) in points.iter().enumerate() {
                        write!(data, "{} {x:.3} {y:.3} ", if i == 0 { 'M' } else { 'L' }).unwrap();
                    }
                    write!(
                        svg,
                        r#"<path d="{data}" fill="none" stroke="{}" stroke-width="{width}"{}/>"#,
                        hex(*color),
                        if *dashed {
                            r#" stroke-dasharray="3 4""#
                        } else {
                            ""
                        }
                    )
                    .unwrap();
                }
                Primitive::Text {
                    point,
                    text,
                    color,
                    size,
                    anchor,
                } => {
                    write!(svg,r#"<text x="{}" y="{}" fill="{}" font-size="{size}" text-anchor="{}">{}</text>"#,point.0,point.1,hex(*color),match anchor {Anchor::Left=>"start",Anchor::Middle=>"middle",Anchor::Right=>"end"},escape(text)).unwrap();
                }
            }
        }
        svg.push_str("</g></svg>\n");
        svg
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn hex(color: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.0 * 255.0).round() as u8,
        (color.1 * 255.0).round() as u8,
        (color.2 * 255.0).round() as u8
    )
}
fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_owned()
    } else {
        format!(
            "{}…",
            text.chars()
                .take(limit.saturating_sub(1))
                .collect::<String>()
        )
    }
}

pub fn layout(plot: &Plot, width: f64, height: f64) -> Rect {
    let columns = ((width - 100.0) / 260.0).floor().max(2.0) as usize;
    let rows = plot.curves.len().div_ceil(columns);
    Rect {
        x: 76.0,
        y: 40.0,
        width: (width - 106.0).max(20.0),
        height: (height - 104.0 - rows as f64 * 20.0).max(30.0),
    }
}

pub fn scene(plot: &Plot, view: Viewport, width: f64, height: f64) -> Scene {
    let rect = layout(plot, width, height);
    let mut scene = Scene {
        rect,
        width,
        height,
        items: Vec::new(),
    };
    scene.text((rect.x, 23.0), "Functions of x", 15.0, Anchor::Left);
    for (value, label) in ticks(view.xmin, view.xmax, (rect.width / 95.0) as usize) {
        let x = rect.x + (value - view.xmin) / (view.xmax - view.xmin) * rect.width;
        scene.line(
            vec![(x, rect.y), (x, rect.y + rect.height)],
            (0.84, 0.87, 0.90),
            0.7,
            true,
        );
        scene.text(
            (x, rect.y + rect.height + 22.0),
            label,
            11.0,
            Anchor::Middle,
        );
    }
    for (value, label) in ticks(view.ymin, view.ymax, (rect.height / 70.0) as usize) {
        let y = rect.y + (view.ymax - value) / (view.ymax - view.ymin) * rect.height;
        scene.line(
            vec![(rect.x, y), (rect.x + rect.width, y)],
            (0.84, 0.87, 0.90),
            0.7,
            true,
        );
        scene.text((rect.x - 10.0, y + 4.0), label, 11.0, Anchor::Right);
    }
    if view.xmin <= 0.0 && view.xmax >= 0.0 {
        let x = rect.x - view.xmin / (view.xmax - view.xmin) * rect.width;
        scene.line(
            vec![(x, rect.y), (x, rect.y + rect.height)],
            (0.42, 0.46, 0.51),
            1.0,
            false,
        );
    }
    if view.ymin <= 0.0 && view.ymax >= 0.0 {
        let y = rect.y + view.ymax / (view.ymax - view.ymin) * rect.height;
        scene.line(
            vec![(rect.x, y), (rect.x + rect.width, y)],
            (0.42, 0.46, 0.51),
            1.0,
            false,
        );
    }
    scene.line(
        vec![
            (rect.x, rect.y),
            (rect.x + rect.width, rect.y),
            (rect.x + rect.width, rect.y + rect.height),
            (rect.x, rect.y + rect.height),
            (rect.x, rect.y),
        ],
        (0.57, 0.61, 0.66),
        0.8,
        false,
    );
    for (i, curve) in plot.curves.iter().enumerate() {
        for points in curve_segments(curve, view, rect) {
            scene.line(points, COLORS[i % COLORS.len()], 1.8, false);
        }
    }
    scene.text(
        (rect.x + rect.width, rect.y + rect.height + 43.0),
        "x",
        12.0,
        Anchor::Middle,
    );
    scene.text((rect.x - 46.0, rect.y + 8.0), "y", 12.0, Anchor::Middle);
    let columns = ((width - 100.0) / 260.0).floor().max(2.0) as usize;
    let column_width = rect.width / columns as f64;
    for (i, curve) in plot.curves.iter().enumerate() {
        let x = rect.x + (i % columns) as f64 * column_width;
        let y = rect.y + rect.height + 65.0 + (i / columns) as f64 * 20.0;
        scene.line(
            vec![(x, y - 4.0), (x + 22.0, y - 4.0)],
            COLORS[i % COLORS.len()],
            2.2,
            false,
        );
        scene.text(
            (x + 30.0, y),
            truncate(
                &curve.label,
                ((column_width - 40.0) / 6.5).max(4.0) as usize,
            ),
            11.0,
            Anchor::Left,
        );
    }
    scene
}

fn ticks(min: f64, max: f64, count: usize) -> Vec<(f64, String)> {
    let raw = (max - min) / count.clamp(2, 12) as f64;
    let scale = 10_f64.powf(raw.log10().floor());
    if !scale.is_finite() || scale <= 0.0 {
        return Vec::new();
    }
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .into_iter()
        .find(|m| m * scale >= raw)
        .unwrap_or(10.0)
        * scale;
    let first = (min / step).ceil() * step;
    (0..32)
        .map(|i| first + i as f64 * step)
        .take_while(|v| *v <= max + step * 1e-8)
        .filter(|v| v.is_finite())
        .map(|v| {
            let value = if v.abs() < step * 1e-8 { 0.0 } else { v };
            let label = if value == 0.0 {
                "0".to_owned()
            } else if value.abs() >= 100_000.0 || value.abs() < 0.001 {
                format!("{value:.2e}")
            } else {
                let mut decimals = (-step.log10().floor()).max(0.0) as usize;
                if (step / 10_f64.powi(-(decimals as i32))).fract().abs() > 1e-8 {
                    decimals += 1;
                }
                format!("{value:.decimals$}")
            };
            (value, label)
        })
        .collect()
}

fn clip_segment(a: Point, b: Point, view: Viewport) -> Option<(Point, Point)> {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    if !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    let mut lower: f64 = 0.0;
    let mut upper: f64 = 1.0;
    for (p, q) in [
        (-dx, a.0 - view.xmin),
        (dx, view.xmax - a.0),
        (-dy, a.1 - view.ymin),
        (dy, view.ymax - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let ratio = q / p;
            if p < 0.0 {
                lower = lower.max(ratio);
            } else {
                upper = upper.min(ratio);
            }
            if lower > upper {
                return None;
            }
        }
    }
    Some((
        (a.0 + lower * dx, a.1 + lower * dy),
        (a.0 + upper * dx, a.1 + upper * dy),
    ))
}

fn curve_segments(curve: &crate::model::Curve, view: Viewport, rect: Rect) -> Vec<Vec<Point>> {
    let mut segments = Vec::new();
    let mut points = Vec::new();
    let mut previous: Option<Point> = None;
    let mut stack = Vec::new();
    let pixel = |p: Point| {
        (
            rect.x + (p.0 - view.xmin) / (view.xmax - view.xmin) * rect.width,
            rect.y + (view.ymax - p.1) / (view.ymax - view.ymin) * rect.height,
        )
    };
    for i in 0..SAMPLES {
        let x = view.xmin + (view.xmax - view.xmin) * (i as f64 / (SAMPLES - 1) as f64);
        let y = curve.expression.evaluate_with_stack(x, &mut stack);
        if !y.is_finite() {
            if points.len() > 1 {
                segments.push(std::mem::take(&mut points));
            }
            points.clear();
            previous = None;
            continue;
        }
        let current = (x, y);
        if let Some(before) = previous {
            let crosses =
                (before.1 < view.ymin && y > view.ymax) || (before.1 > view.ymax && y < view.ymin);
            let split = if crosses {
                let middle = curve.expression.evaluate((before.0 + x) * 0.5);
                !middle.is_finite()
                    || (middle - (before.1 + y) * 0.5).abs() > (view.ymax - view.ymin) * 0.5
            } else {
                false
            };
            if let Some((a, b)) = (!split)
                .then(|| clip_segment(before, current, view))
                .flatten()
            {
                let a = pixel(a);
                let b = pixel(b);
                if points
                    .last()
                    .is_none_or(|last: &Point| (last.0 - a.0).abs() + (last.1 - a.1).abs() > 0.01)
                {
                    if points.len() > 1 {
                        segments.push(std::mem::take(&mut points));
                    }
                    points.clear();
                    points.push(a);
                }
                points.push(b);
            } else {
                if points.len() > 1 {
                    segments.push(std::mem::take(&mut points));
                }
                points.clear();
            }
        }
        previous = Some(current);
    }
    if points.len() > 1 {
        segments.push(points);
    }
    segments
}

pub fn export(plot: &Plot, view: Viewport, path: &Path, width: u32, height: u32) -> Result<()> {
    ensure!(
        (400..=8192).contains(&width)
            && (300..=8192).contains(&height)
            && u64::from(width) * u64::from(height) <= 32_000_000,
        "Image dimensions must be 400–8192 × 300–8192, with at most 32 million pixels"
    );
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        extension == "png" || extension == "svg" || extension == "pdf",
        "Choose a .png, .svg, or .pdf output file"
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent).context("Open export destination")?;
    let scene = scene(plot, view, width as f64, height as f64);
    if extension == "svg" {
        file.write_all(scene.svg().as_bytes())?;
    } else if extension == "pdf" {
        crate::pdf::write(&scene, file.path(), width as f64, height as f64)?;
    } else {
        let mut surface = ImageSurface::create(Format::ARgb32, width as i32, height as i32)?;
        {
            let context = Context::new(&surface)?;
            scene.draw(&context)?;
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data()?;
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for row in data.chunks(stride).take(height as usize) {
            for bytes in row[..width as usize * 4].as_chunks::<4>().0 {
                let pixel = u32::from_ne_bytes(*bytes);
                rgb.extend([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
            }
        }
        let mut encoder = png::Encoder::new(file.as_file_mut(), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgb)?;
        writer.finish()?;
    }
    file.as_file().sync_all()?;
    file.persist(path).context("Save exported plot")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_tick_labels_keep_their_precision() {
        assert!(ticks(-5.0, 5.0, 4).iter().any(|(_, label)| label == "2.5"));
    }
    #[test]
    fn pdf_exports_without_a_display() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("plot.pdf");
        export(
            &Plot::parse("sin(x), x^2").unwrap(),
            Viewport::new(-3.0, 3.0).unwrap(),
            &path,
            1000,
            600,
        )
        .unwrap();
        assert!(std::fs::read(path).unwrap().starts_with(b"%PDF-"));
    }
    #[test]
    fn poles_do_not_join_across_the_graph() {
        let plot = Plot::parse("1/x").unwrap();
        let view = Viewport::new(-1.0, 1.0).unwrap();
        let rect = layout(&plot, 1000.0, 600.0);
        let segments = curve_segments(&plot.curves[0], view, rect);
        assert!(segments.len() >= 2);
        let zero = rect.x + rect.width / 2.0;
        assert!(
            segments
                .iter()
                .all(|line| !(line.iter().any(|p| p.0 < zero) && line.iter().any(|p| p.0 > zero)))
        );
    }
    #[test]
    fn steep_linear_curves_are_clipped_instead_of_treated_as_poles() {
        let plot = Plot::parse("10000*x").unwrap();
        let view = Viewport::new(-1.0, 1.0).unwrap();
        let rect = layout(&plot, 1000.0, 600.0);
        let lines = curve_segments(&plot.curves[0], view, rect);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].first().unwrap().1 > rect.y + rect.height - 0.01);
        assert!(lines[0].last().unwrap().1 < rect.y + 0.01);
    }
    #[test]
    fn undefined_domains_have_no_nonfinite_coordinates() {
        let plot = Plot::parse("sqrt(x)").unwrap();
        let view = Viewport::new(-1.0, 1.0).unwrap();
        assert!(
            curve_segments(&plot.curves[0], view, layout(&plot, 1000.0, 600.0))
                .iter()
                .flatten()
                .all(|(x, y)| x.is_finite() && y.is_finite())
        );
    }
    #[test]
    fn svg_escapes_formulas_and_matches_canvas_dimensions() {
        let plot = Plot::parse("where(x<0,-1,1)").unwrap();
        let svg = scene(&plot, Viewport::new(-2.0, 2.0).unwrap(), 1000.0, 600.0).svg();
        assert!(svg.contains("width=\"1000\""));
        assert!(svg.contains("x&lt;0"));
        assert!(!svg.contains("x<0"));
    }
    #[test]
    fn png_exports_without_a_display_and_has_expected_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("plot.png");
        export(
            &Plot::parse("sin(x),3").unwrap(),
            Viewport::new(-10.0, 10.0).unwrap(),
            &path,
            1000,
            600,
        )
        .unwrap();
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 1000);
        assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 600);
    }
    #[test]
    fn bad_export_does_not_replace_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("plot.jpg");
        std::fs::write(&path, "original").unwrap();
        assert!(
            export(
                &Plot::parse("x").unwrap(),
                Viewport::new(-1.0, 1.0).unwrap(),
                &path,
                1000,
                600
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }
}
