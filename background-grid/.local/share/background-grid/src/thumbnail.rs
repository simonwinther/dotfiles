use crate::Result;
use gtk::gdk_pixbuf::{InterpType, Pixbuf};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const WIDTH: i32 = 448;
const HEIGHT: i32 = 252;

pub fn thumbnail(path: &Path, cache: &Path) -> Result<PathBuf> {
    let metadata = fs::metadata(path)?;
    let nanoseconds =
        i128::from(metadata.mtime()) * 1_000_000_000 + i128::from(metadata.mtime_nsec());
    // The existing Python cache used this exact key and the same output dimensions.
    let key = format!("v1:{}:{nanoseconds}:{}", path.display(), metadata.len());
    let target = cache.join(format!("{:x}.png", Sha256::digest(key.as_bytes())));
    if target.is_file() {
        return Ok(target);
    }
    fs::create_dir_all(cache)?;
    let source = Pixbuf::from_file_at_scale(path, 896, 896, true)?;
    let oriented = source.apply_embedded_orientation().unwrap_or(source);
    let scale = (f64::from(WIDTH) / f64::from(oriented.width()))
        .max(f64::from(HEIGHT) / f64::from(oriented.height()));
    let width = (f64::from(oriented.width()) * scale).ceil() as i32;
    let height = (f64::from(oriented.height()) * scale).ceil() as i32;
    let scaled = oriented
        .scale_simple(width, height, InterpType::Hyper)
        .ok_or("Could not resize wallpaper preview")?;
    let preview = scaled.new_subpixbuf((width - WIDTH) / 2, (height - HEIGHT) / 2, WIDTH, HEIGHT);
    let temporary = tempfile::Builder::new()
        .prefix("preview-")
        .suffix(".tmp")
        .tempfile_in(cache)?;
    preview.savev(temporary.path(), "png", &[])?;
    temporary.persist(&target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::gdk_pixbuf::Colorspace;

    #[test]
    fn preview_is_cropped_cached_and_never_changes_original() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("portrait.png");
        let pixbuf = Pixbuf::new(Colorspace::Rgb, false, 8, 100, 300).unwrap();
        pixbuf.fill(0x336699ff);
        pixbuf.savev(&source, "png", &[])?;
        let original = fs::read(&source)?;
        let cached = thumbnail(&source, &temp.path().join("cache"))?;
        let preview = Pixbuf::from_file(&cached)?;
        assert_eq!((preview.width(), preview.height()), (WIDTH, HEIGHT));
        assert_eq!(thumbnail(&source, &temp.path().join("cache"))?, cached);
        assert_eq!(fs::read(source)?, original);
        Ok(())
    }

    #[test]
    fn invalid_image_does_not_leave_partial_preview() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("bad.png");
        let cache = temp.path().join("cache");
        fs::write(&source, b"not an image")?;
        assert!(thumbnail(&source, &cache).is_err());
        assert_eq!(fs::read_dir(cache)?.count(), 0);
        Ok(())
    }
}
