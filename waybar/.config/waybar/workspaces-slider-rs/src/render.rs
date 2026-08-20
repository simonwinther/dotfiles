//! Software rendering of the pill, matching the Cairo/Pango original.

use fontdue::{Font, FontSettings};
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, Point,
    PremultipliedColorU8, SpreadMode, Stroke, Transform,
};

pub const WORKSPACE_COUNT: i32 = 10;
pub const PANEL_WIDTH: f32 = 292.0;
pub const PANEL_HEIGHT: f32 = 34.0;
pub const PANEL_TOP: i32 = 14;
pub const SLOT_WIDTH: f32 = 28.0;
pub const FIRST_CENTER: f32 = 20.0;
pub const INDICATOR_RADIUS: f32 = 13.0;
const FONT_PX: f32 = 14.0;

const BASE: [f32; 3] = [0x12 as f32 / 255.0, 0x12 as f32 / 255.0, 0x14 as f32 / 255.0];
const TEXT: [f32; 3] = [0xF4 as f32 / 255.0, 0xF4 as f32 / 255.0, 0xF5 as f32 / 255.0];
const BLUE: [f32; 3] = [0x60 as f32 / 255.0, 0xA5 as f32 / 255.0, 0xFA as f32 / 255.0];
const MAUVE: [f32; 3] = [0xA7 as f32 / 255.0, 0x78 as f32 / 255.0, 0xFA as f32 / 255.0];
const SURFACE2: [f32; 3] = [0x52 as f32 / 255.0, 0x52 as f32 / 255.0, 0x5B as f32 / 255.0];

const FONT_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Bold.ttf",
    "/usr/share/fonts/truetype/JetBrainsMonoNerdFont-Bold.ttf",
    "/usr/local/share/fonts/JetBrainsMonoNerdFont-Bold.ttf",
];

pub fn center_for(workspace: i32) -> f32 {
    FIRST_CENTER + (workspace - 1) as f32 * SLOT_WIDTH
}

/// One digit, already rasterized and placed. The set never changes, so this is
/// built once at startup instead of shaping text on every frame.
struct Glyph {
    coverage: Vec<u8>,
    width: usize,
    height: usize,
    left: i32,
    top: i32,
}

pub struct Renderer {
    glyphs: Vec<Glyph>,
    scale: f32,
    width: u32,
    height: u32,
}

fn load_font() -> Option<Font> {
    let mut path = FONT_CANDIDATES.iter().map(std::path::PathBuf::from).find(|p| p.exists());
    if path.is_none() {
        // Fall back to any bold JetBrainsMono in the usual font trees.
        for dir in ["/usr/share/fonts", "/usr/local/share/fonts"] {
            if let Some(found) = find_font(std::path::Path::new(dir), 0) {
                path = Some(found);
                break;
            }
        }
    }
    let bytes = std::fs::read(path?).ok()?;
    Font::from_bytes(bytes, FontSettings::default()).ok()
}

fn find_font(dir: &std::path::Path, depth: usize) -> Option<std::path::PathBuf> {
    if depth > 3 {
        return None;
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_font(&path, depth + 1) {
                return Some(found);
            }
        } else {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.contains("JetBrainsMono") && name.contains("Bold") && name.ends_with(".ttf") {
                return Some(path);
            }
        }
    }
    None
}

impl Renderer {
    pub fn new(scale: f32) -> Option<Self> {
        let font = load_font()?;
        let px = FONT_PX * scale;
        let line = font.horizontal_line_metrics(px)?;
        // Pango centred the *logical* box (ascent+descent), not the ink box, so
        // reproduce that or the digits sit a pixel or two off.
        let baseline = PANEL_HEIGHT * scale / 2.0 + (line.ascent + line.descent) / 2.0;

        let mut glyphs = Vec::with_capacity(WORKSPACE_COUNT as usize);
        for workspace in 1..=WORKSPACE_COUNT {
            let character = if workspace == 10 { '0' } else { (b'0' + workspace as u8) as char };
            let (metrics, coverage) = font.rasterize(character, px);
            let center = center_for(workspace) * scale;
            let left = center - metrics.advance_width / 2.0 + metrics.xmin as f32;
            let top = baseline - metrics.ymin as f32 - metrics.height as f32;
            glyphs.push(Glyph {
                coverage,
                width: metrics.width,
                height: metrics.height,
                left: left.round() as i32,
                top: top.round() as i32,
            });
        }

        // fontdue parses every glyph in the file up front, and a Nerd Font has
        // over twelve thousand. Dropping it frees that back to the allocator,
        // but glibc keeps the arena, so hand it back to the kernel explicitly.
        // Only the ten bitmaps above outlive this.
        drop(font);
        unsafe { libc::malloc_trim(0) };

        Some(Renderer {
            glyphs,
            scale,
            width: (PANEL_WIDTH * scale).round() as u32,
            height: (PANEL_HEIGHT * scale).round() as u32,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Which labels can fall under the indicator at this position.
    fn labels_near(&self, position: f32) -> std::ops::RangeInclusive<i32> {
        let reach = INDICATOR_RADIUS + SLOT_WIDTH / 2.0;
        let low = (((position - reach - FIRST_CENTER) / SLOT_WIDTH).ceil() as i32 + 1).max(1);
        let high =
            (((position + reach - FIRST_CENTER) / SLOT_WIDTH).floor() as i32 + 1).min(WORKSPACE_COUNT);
        low..=high
    }

    fn draw_labels(
        &self,
        pixmap: &mut Pixmap,
        occupied: &std::collections::HashSet<i32>,
        deferred: Option<i32>,
        range: std::ops::RangeInclusive<i32>,
        override_color: Option<[f32; 3]>,
        mask: Option<&Mask>,
    ) {
        for workspace in range {
            let glyph = &self.glyphs[(workspace - 1) as usize];
            let (color, alpha) = match override_color {
                Some(color) => (color, 1.0),
                None if occupied.contains(&workspace) && Some(workspace) != deferred => (TEXT, 1.0),
                None => (TEXT, 0.45),
            };
            blend_glyph(pixmap, glyph, color, alpha, mask);
        }
    }

    pub fn draw(
        &self,
        pixmap: &mut Pixmap,
        position: f32,
        occupied: &std::collections::HashSet<i32>,
        deferred: Option<i32>,
    ) {
        let s = self.scale;
        let transform = Transform::from_scale(s, s);
        let mid = PANEL_HEIGHT / 2.0;

        pixmap.fill(Color::TRANSPARENT);

        let mut paint = Paint::default();
        paint.anti_alias = true;

        if let Some(path) = rounded_rect(0.0, 0.0, PANEL_WIDTH, PANEL_HEIGHT, PANEL_HEIGHT / 2.0) {
            paint.set_color(rgba(BASE, 0.96));
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }

        if let Some(path) =
            rounded_rect(0.5, 0.5, PANEL_WIDTH - 1.0, PANEL_HEIGHT - 1.0, 16.5)
        {
            paint.set_color(rgba(SURFACE2, 0.22));
            let stroke = Stroke { width: 1.0, ..Default::default() };
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        }

        self.draw_labels(pixmap, occupied, deferred, 1..=WORKSPACE_COUNT, None, None);

        for (radius, alpha) in [(17.0_f32, 0.035_f32), (15.0, 0.07)] {
            if let Some(path) = PathBuilder::from_circle(position, mid + 2.0, radius) {
                paint.set_color(rgba(BLUE, alpha));
                pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
            }
        }

        if let Some(path) = PathBuilder::from_circle(position, mid, INDICATOR_RADIUS) {
            // Logical coordinates, like the path: fill_path applies `transform`
            // to the shader as well, so pre-scaling here would stretch the ramp
            // by the scale factor twice over and flatten it to its first stop.
            let start = Point::from_xy(position - INDICATOR_RADIUS, mid - INDICATOR_RADIUS);
            let end = Point::from_xy(position + INDICATOR_RADIUS, mid + INDICATOR_RADIUS);
            if let Some(shader) = LinearGradient::new(
                start,
                end,
                vec![
                    GradientStop::new(0.0, rgba(BLUE, 1.0)),
                    GradientStop::new(1.0, rgba(MAUVE, 1.0)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                let mut fill = Paint::default();
                fill.anti_alias = true;
                fill.shader = shader;
                pixmap.fill_path(&path, &fill, FillRule::Winding, transform, None);
            }
        }

        if let Some(path) = PathBuilder::from_circle(position, mid, INDICATOR_RADIUS - 0.5) {
            paint.set_color(rgba(TEXT, 0.20));
            let stroke = Stroke { width: 1.0, ..Default::default() };
            pixmap.stroke_path(&path, &paint, &stroke, transform, None);
        }

        // Labels showing through the indicator, punched out in the panel colour.
        if let Some(path) = PathBuilder::from_circle(position, mid, INDICATOR_RADIUS - 0.5) {
            if let Some(mut mask) = Mask::new(self.width, self.height) {
                mask.fill_path(&path, FillRule::Winding, true, transform);
                let range = self.labels_near(position);
                self.draw_labels(pixmap, occupied, deferred, range, Some(BASE), Some(&mask));
            }
        }
    }
}

fn rgba(color: [f32; 3], alpha: f32) -> Color {
    Color::from_rgba(color[0], color[1], color[2], alpha).unwrap_or(Color::TRANSPARENT)
}

fn rounded_rect(x: f32, y: f32, width: f32, height: f32, radius: f32) -> Option<Path> {
    const K: f32 = 0.552_284_75;
    let control = radius * K;
    let (right, bottom) = (x + width, y + height);
    let mut builder = PathBuilder::new();
    builder.move_to(x + radius, y);
    builder.line_to(right - radius, y);
    builder.cubic_to(right - radius + control, y, right, y + radius - control, right, y + radius);
    builder.line_to(right, bottom - radius);
    builder.cubic_to(
        right,
        bottom - radius + control,
        right - radius + control,
        bottom,
        right - radius,
        bottom,
    );
    builder.line_to(x + radius, bottom);
    builder.cubic_to(x + radius - control, bottom, x, bottom - radius + control, x, bottom - radius);
    builder.line_to(x, y + radius);
    builder.cubic_to(x, y + radius - control, x + radius - control, y, x + radius, y);
    builder.close();
    builder.finish()
}

/// Source-over blend of a coverage bitmap in a flat colour, honouring a clip.
fn blend_glyph(
    pixmap: &mut Pixmap,
    glyph: &Glyph,
    color: [f32; 3],
    alpha: f32,
    mask: Option<&Mask>,
) {
    let canvas_width = pixmap.width() as i32;
    let canvas_height = pixmap.height() as i32;
    let mask_data = mask.map(|m| m.data());
    let pixels = pixmap.pixels_mut();

    for row in 0..glyph.height {
        let y = glyph.top + row as i32;
        if y < 0 || y >= canvas_height {
            continue;
        }
        for column in 0..glyph.width {
            let x = glyph.left + column as i32;
            if x < 0 || x >= canvas_width {
                continue;
            }
            let mut coverage = glyph.coverage[row * glyph.width + column] as f32 / 255.0;
            if coverage <= 0.0 {
                continue;
            }
            let index = y as usize * canvas_width as usize + x as usize;
            if let Some(data) = mask_data {
                coverage *= data[index] as f32 / 255.0;
                if coverage <= 0.0 {
                    continue;
                }
            }
            let source_alpha = coverage * alpha;
            let inverse = 1.0 - source_alpha;
            let destination = pixels[index];
            let out_a = source_alpha * 255.0 + destination.alpha() as f32 * inverse;
            let out_r = color[0] * source_alpha * 255.0 + destination.red() as f32 * inverse;
            let out_g = color[1] * source_alpha * 255.0 + destination.green() as f32 * inverse;
            let out_b = color[2] * source_alpha * 255.0 + destination.blue() as f32 * inverse;
            let a = out_a.round().clamp(0.0, 255.0) as u8;
            // Premultiplied invariant: no channel may exceed alpha.
            let r = (out_r.round().clamp(0.0, 255.0) as u8).min(a);
            let g = (out_g.round().clamp(0.0, 255.0) as u8).min(a);
            let b = (out_b.round().clamp(0.0, 255.0) as u8).min(a);
            if let Some(value) = PremultipliedColorU8::from_rgba(r, g, b, a) {
                pixels[index] = value;
            }
        }
    }
}
