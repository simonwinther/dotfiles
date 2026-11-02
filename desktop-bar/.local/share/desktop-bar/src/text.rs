use crate::{draw, theme::Color};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use memmap2::Mmap;
use std::os::unix::process::CommandExt;
use std::{collections::HashMap, fs::File, path::PathBuf, process::Command};

struct Glyph {
    width: u32,
    height: u32,
    left: i32,
    top: i32,
    advance: f32,
    coverage: Vec<u8>,
}
/// Multiplicative hashing for packed glyph keys, which need no SipHash.
#[derive(Default)]
struct GlyphHasher(u64);
impl std::hash::Hasher for GlyphHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(*byte)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
    }
    fn write_u64(&mut self, value: u64) {
        self.0 = value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

pub struct Text {
    fonts: [Mmap; 2],
    cache: HashMap<u64, Glyph, std::hash::BuildHasherDefault<GlyphHasher>>,
    family: String,
    loaded_tables: bool,
}

fn font_path(family: &str, env: &str) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(env).filter(|v| !v.is_empty()) {
        return Ok(path.into());
    }
    let mut command = Command::new("fc-match");
    unsafe {
        command.pre_exec(|| {
            crate::reset_child_signals();
            Ok(())
        });
    }
    let result = command
        .args(["--format", "%{file}", family])
        .output()
        .map_err(|e| e.to_string())?;
    let path = PathBuf::from(String::from_utf8_lossy(&result.stdout).trim());
    if !result.status.success() || !path.is_file() {
        return Err(format!("Font unavailable: {family}"));
    }
    Ok(path)
}

impl Text {
    pub fn new(family: &str) -> Result<Self, String> {
        let load = |family, env| -> Result<Mmap, String> {
            let file = File::open(font_path(family, env)?).map_err(|e| e.to_string())?;
            let map = unsafe { Mmap::map(&file) }.map_err(|e| e.to_string())?;
            FontRef::try_from_slice(&map).map_err(|e| e.to_string())?;
            Ok(map)
        };
        Ok(Self {
            fonts: [
                load(&format!("{family}:weight=demibold"), "DESKTOP_BAR_FONT")?,
                load("Symbols Nerd Font Mono", "DESKTOP_BAR_ICON_FONT")?,
            ],
            cache: HashMap::default(),
            family: family.into(),
            loaded_tables: false,
        })
    }
    pub fn reload(&mut self, family: &str) {
        if self.family != family
            && let Ok(new) = Self::new(family)
        {
            *self = new;
        }
    }
    pub fn release_font_pages(&mut self) {
        if !self.loaded_tables {
            return;
        }
        // Only cached glyph coverage survives a frame. The read-only font
        // tables can be faulted back in for a new character or size, so large
        // icon fonts need not remain resident for the lifetime of the bar.
        for map in &self.fonts {
            unsafe {
                libc::madvise(
                    map.as_ptr().cast_mut().cast(),
                    map.len(),
                    libc::MADV_DONTNEED,
                );
            }
        }
        self.loaded_tables = false;
    }
    fn glyph(&mut self, c: char, px: f32) -> &Glyph {
        let icon = usize::from((c as u32 >= 0xe000 && c as u32 <= 0xf8ff) || c as u32 >= 0xf0000);
        // Font, character and size packed into one word; lookups happen for
        // every character measured or drawn.
        let key = (u64::from((px * 64.0).round() as u32) << 22) | (u64::from(c) << 1) | icon as u64;
        if !self.cache.contains_key(&key) {
            self.loaded_tables = true;
            if self.cache.len() >= 2048 {
                self.cache.clear();
            }
            // FontRef borrows the mmap and parses tables lazily, without a raster font atlas.
            let font = FontRef::try_from_slice(&self.fonts[icon]).unwrap();
            // CSS sizes describe the em square; ab_glyph's PxScale describes
            // ascent minus descent. Converting keeps labels equal to Pango's
            // pixel sizes, including fonts with tall icon metrics.
            let px =
                px * font.height_unscaled() / font.units_per_em().unwrap_or(font.height_unscaled());
            let scaled = font.as_scaled(PxScale::from(px));
            let id = scaled.glyph_id(c);
            let mut glyph = Glyph {
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                advance: scaled.h_advance(id),
                coverage: vec![],
            };
            if let Some(outline) = scaled.outline_glyph(id.with_scale(px)) {
                let bounds = outline.px_bounds();
                glyph.width = bounds.width() as u32;
                glyph.height = bounds.height() as u32;
                glyph.left = bounds.min.x as i32;
                glyph.top = bounds.min.y as i32;
                glyph.coverage = vec![0; (glyph.width * glyph.height) as usize];
                outline.draw(|x, y, a| {
                    glyph.coverage[(y * glyph.width + x) as usize] = (a * 255.0).round() as u8;
                });
            }
            self.cache.insert(key, glyph);
        }
        &self.cache[&key]
    }
    pub fn width(&mut self, text: &str, px: f32) -> f32 {
        text.chars().map(|c| self.glyph(c, px).advance).sum()
    }
    pub fn draw(
        &mut self,
        p: &mut tiny_skia::Pixmap,
        text: &str,
        x: f32,
        baseline: f32,
        px: f32,
        color: Color,
    ) {
        let clip = crate::layout::Rect::new(0.0, 0.0, p.width() as f32, p.height() as f32);
        self.draw_clipped(p, text, x, baseline, px, color, clip);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw_clipped(
        &mut self,
        p: &mut tiny_skia::Pixmap,
        text: &str,
        x: f32,
        baseline: f32,
        px: f32,
        color: Color,
        clip: crate::layout::Rect,
    ) {
        let mut x = x;
        for c in text.chars() {
            let g = self.glyph(c, px);
            let (left, top) = (x.round() as i32 + g.left, baseline.round() as i32 + g.top);
            for y in 0..g.height {
                for gx in 0..g.width {
                    let coverage = g.coverage[(y * g.width + gx) as usize];
                    // Zero coverage leaves the destination bytes unchanged.
                    if coverage == 0 {
                        continue;
                    }
                    let dx = left + gx as i32;
                    let dy = top + y as i32;
                    if !clip.contains(dx as f32, dy as f32) {
                        continue;
                    }
                    draw::blend(p, dx, dy, f32::from(coverage) / 255.0, color);
                }
            }
            x += g.advance;
        }
    }
}
