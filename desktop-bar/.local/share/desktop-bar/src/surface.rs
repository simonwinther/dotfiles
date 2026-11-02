use crate::{App, layout::Rect};
use smithay_client_toolkit::{
    compositor::FrameCallbackData,
    reexports::client::{
        QueueHandle,
        protocol::{wl_shm, wl_surface},
    },
    shm::{
        Shm,
        slot::{Buffer, SlotPool},
    },
};
use std::{cell::Cell, rc::Rc};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::WpFractionalScaleV1,
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};

pub struct Canvas {
    pub pixmap: tiny_skia::Pixmap,
    pool: SlotPool,
    pub viewport: Option<WpViewport>,
    fractional: Option<WpFractionalScaleV1>,
    pub scale: f32,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
    frame_pending: Rc<Cell<bool>>,
    pub dirty: bool,
    owns_viewport: bool,
    format: wl_shm::Format,
    rows: Option<u32>,
    buffers: Vec<Retained>,
}
/// A buffer kept across frames, with the image rows it has not received yet.
struct Retained {
    buffer: Buffer,
    stale: std::ops::Range<usize>,
}
/// More buffers than this means the compositor holds them unusually long.
const RETAINED_BUFFERS: usize = 3;
impl Canvas {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        surface: &wl_surface::WlSurface,
        qh: &QueueHandle<App>,
        shm: &Shm,
        viewporter: Option<&WpViewporter>,
        fractional: Option<&WpFractionalScaleManagerV1>,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<Self, String> {
        let viewport = viewporter.map(|v| v.get_viewport(surface, qh, ()));
        let fractional = if viewport.is_some() {
            fractional.map(|f| f.get_fractional_scale(surface, qh, surface.clone()))
        } else {
            None
        };
        let scale = if viewport.is_some() {
            scale
        } else {
            scale.ceil()
        };
        let (w, h) = pixels(width, height, scale);
        let pool = SlotPool::new(w as usize * h as usize * 8, shm).map_err(|e| e.to_string())?;
        let pixmap = tiny_skia::Pixmap::new(w, h).ok_or("Invalid surface dimensions")?;
        if let Some(viewport) = &viewport {
            surface.set_buffer_scale(1);
            viewport.set_destination(width as i32, height as i32);
        } else {
            surface.set_buffer_scale(scale as i32);
        }
        Ok(Self {
            pixmap,
            pool,
            viewport,
            fractional,
            scale,
            width,
            height,
            configured: false,
            frame_pending: Rc::new(Cell::new(false)),
            dirty: true,
            owns_viewport: true,
            format: format(shm),
            rows: None,
            buffers: Vec::new(),
        })
    }
    pub fn shared(source: &Self, shm: &Shm) -> Result<Self, String> {
        let (w, h) = pixels(source.width, source.height, source.scale);
        Ok(Self {
            pixmap: tiny_skia::Pixmap::new(w, h).ok_or("Invalid surface dimensions")?,
            pool: SlotPool::new(w as usize * h as usize * 8, shm).map_err(|e| e.to_string())?,
            viewport: source.viewport.clone(),
            fractional: None,
            scale: source.scale,
            width: source.width,
            height: source.height,
            configured: true,
            frame_pending: source.frame_pending.clone(),
            dirty: true,
            owns_viewport: false,
            format: source.format,
            rows: None,
            buffers: Vec::new(),
        })
    }
    pub fn resize(
        &mut self,
        surface: &wl_surface::WlSurface,
        width: u32,
        height: u32,
        scale: f32,
        shm: &Shm,
    ) -> bool {
        let scale = if self.viewport.is_some() {
            scale
        } else {
            scale.ceil()
        };
        if width == self.width && height == self.height && scale == self.scale {
            return false;
        }
        let (w, h) = pixels(width, height, scale);
        let Some(pixmap) = tiny_skia::Pixmap::new(w, limit(self.rows, h)) else {
            return false;
        };
        // wl_shm pools can only grow. Replace the mapping when dimensions
        // change so output scaling and panel morphs cannot retain a high-water
        // allocation. Existing compositor buffers survive pool destruction.
        if (w, h) != self.buffer_size() {
            let Ok(pool) = SlotPool::new(w as usize * h as usize * 8, shm) else {
                return false;
            };
            self.pool = pool;
        }
        self.pixmap = pixmap;
        self.buffers.clear();
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.dirty = true;
        if let Some(v) = &self.viewport {
            v.set_destination(width as i32, height as i32);
            surface.set_buffer_scale(1);
        } else {
            surface.set_buffer_scale(scale as i32);
        }
        true
    }
    /// Physical size of the buffers attached to the surface.
    pub fn buffer_size(&self) -> (u32, u32) {
        pixels(self.width, self.height, self.scale)
    }
    /// Retains only the top `rows` of the image. Lower rows of every
    /// presented buffer are transparent. Returns whether the image was
    /// reallocated, which requires a full redraw.
    pub fn limit_rows(&mut self, rows: Option<u32>) -> bool {
        self.rows = rows;
        let (w, h) = self.buffer_size();
        let h = limit(rows, h);
        if (self.pixmap.width(), self.pixmap.height()) == (w, h) {
            return false;
        }
        let Some(pixmap) = tiny_skia::Pixmap::new(w, h) else {
            return false;
        };
        self.pixmap = pixmap;
        self.buffers.clear();
        true
    }
    pub fn request(&mut self, surface: &wl_surface::WlSurface, qh: &QueueHandle<App>) {
        self.dirty = true;
        self.animate(surface, qh);
    }
    pub fn pending(&self) -> bool {
        self.frame_pending.get()
    }
    pub fn frame_done(&self) {
        self.frame_pending.set(false);
    }
    pub fn animate(&mut self, surface: &wl_surface::WlSurface, qh: &QueueHandle<App>) {
        if !self.configured || self.pending() {
            return;
        }
        self.frame_pending.set(true);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        surface.commit();
    }
    pub fn present(
        &mut self,
        surface: &wl_surface::WlSurface,
        qh: &QueueHandle<App>,
        damage: &[Rect],
        moving: bool,
    ) {
        self.dirty = false;
        if damage.is_empty() {
            // Nothing changed, but an animation still needs the next frame.
            if moving {
                self.animate(surface, qh);
            }
            return;
        }
        let (w, h) = self.buffer_size();
        let (w, h) = (w as i32, h as i32);
        let stride = w as usize * 4;
        let image_rows = self.pixmap.height() as usize;
        // Rows below the retained image are transparent in every buffer.
        let changed = damage
            .iter()
            .map(|r| r.rows(image_rows as u32))
            .reduce(|a, b| a.start.min(b.start)..a.end.max(b.end))
            .unwrap_or(0..0);
        for retained in &mut self.buffers {
            retained.stale = if retained.stale.is_empty() {
                changed.clone()
            } else if changed.is_empty() {
                retained.stale.clone()
            } else {
                retained.stale.start.min(changed.start)..retained.stale.end.max(changed.end)
            };
        }
        // Reuse a buffer the compositor has released and copy only the rows
        // it has not seen. A new buffer receives the whole image once.
        let reusable = self
            .buffers
            .iter()
            .position(|retained| retained.buffer.canvas(&mut self.pool).is_some());
        let index = match reusable {
            Some(index) => index,
            None => {
                let Ok((buffer, canvas)) = self.pool.create_buffer(w, h, w * 4, self.format) else {
                    self.request(surface, qh);
                    return;
                };
                canvas[image_rows * stride..h as usize * stride].fill(0);
                let retained = Retained {
                    buffer,
                    stale: 0..image_rows,
                };
                if self.buffers.len() < RETAINED_BUFFERS {
                    self.buffers.push(retained);
                    self.buffers.len() - 1
                } else {
                    // Every retained buffer is busy; use this one for a single frame.
                    copy_rows(
                        self.format,
                        retained.buffer.canvas(&mut self.pool).unwrap(),
                        self.pixmap.data(),
                        0..image_rows,
                        stride,
                    );
                    attach(
                        &self.frame_pending,
                        surface,
                        qh,
                        &retained.buffer,
                        damage,
                        moving,
                    );
                    return;
                }
            }
        };
        let retained = &mut self.buffers[index];
        let rows = std::mem::replace(&mut retained.stale, 0..0);
        copy_rows(
            self.format,
            retained.buffer.canvas(&mut self.pool).unwrap(),
            self.pixmap.data(),
            rows,
            stride,
        );
        attach(
            &self.frame_pending,
            surface,
            qh,
            &retained.buffer,
            damage,
            moving,
        );
    }
}
fn attach(
    frame_pending: &Cell<bool>,
    surface: &wl_surface::WlSurface,
    qh: &QueueHandle<App>,
    buffer: &Buffer,
    damage: &[Rect],
    moving: bool,
) {
    let (w, h) = (buffer.stride() / 4, buffer.height());
    for r in damage {
        let x = r.x.floor().max(0.0) as i32;
        let y = r.y.floor().max(0.0) as i32;
        surface.damage_buffer(
            x,
            y,
            ((r.x + r.w).ceil() as i32 - x).min(w - x).max(0),
            ((r.y + r.h).ceil() as i32 - y).min(h - y).max(0),
        );
    }
    if moving && !frame_pending.get() {
        frame_pending.set(true);
        surface.frame(qh, FrameCallbackData(surface.clone()));
    }
    if buffer.attach_to(surface).is_ok() {
        surface.commit();
    }
}
/// Copies image rows into a shm buffer, swizzling when the format needs it.
fn copy_rows(
    format: wl_shm::Format,
    canvas: &mut [u8],
    image: &[u8],
    rows: std::ops::Range<usize>,
    stride: usize,
) {
    let range = rows.start * stride..rows.end * stride;
    if format == wl_shm::Format::Abgr8888 {
        canvas[range.clone()].copy_from_slice(&image[range]);
    } else {
        for (out, pixel) in canvas[range.clone()]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(image[range].as_chunks::<4>().0.iter())
        {
            out.copy_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take()
            && self.owns_viewport
        {
            v.destroy();
        }
    }
}
/// ABGR8888 stores bytes in tiny-skia's premultiplied RGBA order, so frames
/// can be copied without swizzling. ARGB8888 is the protocol's guaranteed fallback.
fn format(shm: &Shm) -> wl_shm::Format {
    if shm.formats().contains(&wl_shm::Format::Abgr8888) {
        wl_shm::Format::Abgr8888
    } else {
        wl_shm::Format::Argb8888
    }
}
fn limit(rows: Option<u32>, height: u32) -> u32 {
    rows.map_or(height, |rows| rows.clamp(1, height))
}
fn pixels(w: u32, h: u32, s: f32) -> (u32, u32) {
    (
        (w as f32 * s).round().clamp(1.0, 32768.0) as u32,
        (h as f32 * s).round().clamp(1.0, 16384.0) as u32,
    )
}
