use gtk::{gdk, gdk_pixbuf::Pixbuf, glib};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

pub struct Pixels {
    width: i32,
    height: i32,
    stride: usize,
    alpha: bool,
    bytes: Vec<u8>,
}

impl Pixels {
    pub fn load(path: &str, width: i32, height: i32, cache_key: Option<&str>) -> Option<Self> {
        let cache = cache_key
            .filter(|key| key.len() <= 128 && key.bytes().all(|b| b.is_ascii_alphanumeric()))
            .and_then(|key| {
                let base = std::env::var_os("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .or_else(|| {
                        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))
                    })?;
                Some(
                    base.join("clipboard-picker/thumbnails")
                        .join(format!("{key}.png")),
                )
            });
        let cached = cache.as_ref().filter(|p| p.is_file());
        let pixbuf = cached
            .and_then(|p| Pixbuf::from_file_at_scale(p, width, height, true).ok())
            .or_else(|| Pixbuf::from_file_at_scale(path, width, height, true).ok())?;
        if cached.is_none()
            && let Some(cache) = cache
            && let Some(parent) = cache.parent()
            && std::fs::create_dir_all(parent).is_ok()
        {
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            if pixbuf.savev(&cache, "png", &[]).is_ok() {
                let _ = std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o600));
            }
        }
        Some(Self {
            width: pixbuf.width(),
            height: pixbuf.height(),
            stride: pixbuf.rowstride() as usize,
            alpha: pixbuf.has_alpha(),
            bytes: pixbuf.read_pixel_bytes().as_ref().to_vec(),
        })
    }

    pub fn texture(self) -> gdk::MemoryTexture {
        gdk::MemoryTexture::new(
            self.width,
            self.height,
            if self.alpha {
                gdk::MemoryFormat::R8g8b8a8
            } else {
                gdk::MemoryFormat::R8g8b8
            },
            &glib::Bytes::from_owned(self.bytes),
            self.stride,
        )
    }
}
