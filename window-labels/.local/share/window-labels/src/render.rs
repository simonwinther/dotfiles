//! Draws one label pill into a premultiplied RGBA pixmap.
use crate::placement::LABEL_HEIGHT;
use ab_glyph::{Font, FontVec, GlyphId, PxScale, ScaleFont, point};
use std::{path::PathBuf, process::Command};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

const FONT_PX: f32 = 11.0;
const PADDING: f32 = 7.0;
const RADIUS: f32 = 5.0;
const ELLIPSIS: char = '…';

struct Palette {
    background: [u8; 4],
    border: [u8; 4],
    text: [u8; 4],
}

const ACTIVE: Palette = Palette {
    background: [14, 18, 22, 240],
    border: [51, 204, 255, 230],
    text: [255, 255, 255, 255],
};
const INACTIVE: Palette = Palette {
    background: [14, 14, 14, 215],
    border: [89, 89, 89, 170],
    text: [255, 255, 255, 175],
};

pub struct Painter {
    /// Primary font first; later fonts supply glyphs the primary lacks.
    fonts: Vec<FontVec>,
    /// Outline colours for active and inactive labels.
    borders: ([u8; 4], [u8; 4]),
}

fn font_file(pattern: &str, env: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(env).filter(|v| !v.is_empty()) {
        return Some(path.into());
    }
    let output = Command::new("fc-match")
        .args(["--format", "%{file}", pattern])
        .output()
        .ok()?;
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    (output.status.success() && path.is_file()).then_some(path)
}

impl Painter {
    pub fn new() -> Result<Self, String> {
        let fonts: Vec<FontVec> = [
            ("monospace:weight=bold", "WINDOW_LABELS_FONT"),
            ("Symbols Nerd Font Mono", "WINDOW_LABELS_SYMBOL_FONT"),
            ("sans-serif", "WINDOW_LABELS_FALLBACK_FONT"),
        ]
        .into_iter()
        .filter_map(|(pattern, env)| font_file(pattern, env))
        .filter_map(|path| FontVec::try_from_vec(std::fs::read(path).ok()?).ok())
        .collect();
        if fonts.is_empty() {
            return Err("No usable font found".into());
        }
        Ok(Self {
            fonts,
            borders: (ACTIVE.border, INACTIVE.border),
        })
    }

    /// Outlines labels in the compositor's window border colours.
    pub fn with_borders(mut self, borders: Option<([u8; 4], [u8; 4])>) -> Self {
        if let Some(borders) = borders {
            self.borders = borders;
        }
        self
    }

    fn glyph(&self, c: char) -> (&FontVec, GlyphId) {
        self.fonts
            .iter()
            .map(|font| (font, font.glyph_id(c)))
            .find(|(_, id)| id.0 != 0)
            .unwrap_or_else(|| (&self.fonts[0], self.fonts[0].glyph_id(c)))
    }

    /// ab_glyph scales by line height; this sizes the em square instead.
    fn scale(font: &FontVec, px: f32) -> PxScale {
        let em = font.units_per_em().unwrap_or(1000.0);
        PxScale::from(px * font.height_unscaled() / em)
    }

    fn advance(&self, c: char, px: f32) -> f32 {
        let (font, id) = self.glyph(c);
        font.as_scaled(Self::scale(font, px)).h_advance(id)
    }

    fn text_width(&self, text: &str, px: f32) -> f32 {
        text.chars().map(|c| self.advance(c, px)).sum()
    }

    fn covered(&self, c: char) -> bool {
        c.is_whitespace() || self.fonts.iter().any(|font| font.glyph_id(c).0 != 0)
    }

    /// Shortens `title` with an ellipsis so the pill fits `max_width`.
    /// Characters no font can draw are dropped rather than shown as boxes.
    pub fn fit(&self, title: &str, max_width: i32) -> String {
        let title: String = title.chars().filter(|&c| self.covered(c)).collect();
        let title = title.trim();
        let available = max_width as f32 - 2.0 * PADDING;
        if self.text_width(title, FONT_PX) <= available {
            return title.to_owned();
        }
        let mut width = self.advance(ELLIPSIS, FONT_PX);
        let mut fitted: String = title
            .chars()
            .take_while(|&c| {
                width += self.advance(c, FONT_PX);
                width <= available
            })
            .collect();
        fitted.truncate(fitted.trim_end().len());
        fitted.push(ELLIPSIS);
        fitted
    }

    /// Logical width of the pill holding `text`.
    pub fn width(&self, text: &str) -> i32 {
        (self.text_width(text, FONT_PX) + 2.0 * PADDING).ceil() as i32
    }

    pub fn paint(&self, text: &str, active: bool, width: i32, scale: f64) -> Option<Pixmap> {
        let scale = scale as f32;
        let pixel_width = (width as f32 * scale).ceil() as u32;
        let pixel_height = (LABEL_HEIGHT as f32 * scale).ceil() as u32;
        let mut pixmap = Pixmap::new(pixel_width, pixel_height)?;
        let palette = if active { &ACTIVE } else { &INACTIVE };

        let inset = 0.5 * scale;
        let pill = rounded_rect(
            inset,
            inset,
            pixel_width as f32 - 2.0 * inset,
            pixel_height as f32 - 2.0 * inset,
            RADIUS * scale,
        )?;
        let mut paint = Paint {
            anti_alias: true,
            ..Paint::default()
        };
        paint.set_color_rgba8(
            palette.background[0],
            palette.background[1],
            palette.background[2],
            palette.background[3],
        );
        pixmap.fill_path(
            &pill,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
        let border = if active {
            self.borders.0
        } else {
            self.borders.1
        };
        paint.set_color_rgba8(border[0], border[1], border[2], border[3]);
        let stroke = Stroke {
            width: scale,
            ..Stroke::default()
        };
        pixmap.stroke_path(&pill, &paint, &stroke, Transform::identity(), None);

        self.draw_text(&mut pixmap, text, palette.text, scale);
        Some(pixmap)
    }

    fn draw_text(&self, pixmap: &mut Pixmap, text: &str, color: [u8; 4], scale: f32) {
        let px = FONT_PX * scale;
        let primary = self.fonts[0].as_scaled(Self::scale(&self.fonts[0], px));
        let baseline = ((pixmap.height() as f32 - (primary.ascent() - primary.descent())) / 2.0
            + primary.ascent())
        .round();
        let (width, height) = (pixmap.width() as i32, pixmap.height() as i32);
        let pixels = pixmap.data_mut();
        let mut x = PADDING * scale;
        for c in text.chars() {
            let (font, id) = self.glyph(c);
            let scaled = font.as_scaled(Self::scale(font, px));
            let glyph = id.with_scale_and_position(scaled.scale(), point(x, baseline));
            x += scaled.h_advance(id);
            let Some(outline) = font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if (0..width).contains(&px_x) && (0..height).contains(&px_y) {
                    let index = ((px_y * width + px_x) * 4) as usize;
                    blend(&mut pixels[index..index + 4], color, coverage);
                }
            });
        }
    }
}

/// Source-over blend of a straight-alpha colour into a premultiplied pixel.
fn blend(pixel: &mut [u8], color: [u8; 4], coverage: f32) {
    let alpha = coverage.clamp(0.0, 1.0) * f32::from(color[3]) / 255.0;
    for channel in 0..3 {
        let source = f32::from(color[channel]) * alpha;
        pixel[channel] = (source + f32::from(pixel[channel]) * (1.0 - alpha)).round() as u8;
    }
    pixel[3] = (alpha * 255.0 + f32::from(pixel[3]) * (1.0 - alpha)).round() as u8;
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<tiny_skia::Path> {
    let r = radius.min(w / 2.0).min(h / 2.0);
    // Cubic approximation of a quarter circle.
    let k = r * 0.552_284_8;
    let mut path = PathBuilder::new();
    path.move_to(x + r, y);
    path.line_to(x + w - r, y);
    path.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    path.line_to(x + w, y + h - r);
    path.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    path.line_to(x + r, y + h);
    path.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    path.line_to(x, y + r);
    path.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    path.close();
    path.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_truncates_with_ellipsis_within_width() {
        let Ok(painter) = Painter::new() else { return };
        let title = "OC | Colab notebook editing in Neovim with a very long suffix";
        let fitted = painter.fit(title, 120);
        assert!(fitted.ends_with(ELLIPSIS));
        assert!(painter.width(&fitted) <= 120);
        assert_eq!(painter.fit("Claude", 400), "Claude");
    }

    #[test]
    fn paint_scales_buffer() {
        let Ok(painter) = Painter::new() else { return };
        let pixmap = painter.paint("Claude", true, 60, 1.2).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (72, 20));
    }
}
