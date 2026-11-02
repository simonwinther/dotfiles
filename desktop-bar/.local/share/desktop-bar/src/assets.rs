//! Limit file reads and decoded dimensions before allocating preview images.
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path};

const MAX_BYTES: usize = 4 * 1024 * 1024;

pub fn read(path: impl AsRef<Path>) -> Option<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= MAX_BYTES).then_some(bytes)
}

pub fn decode_png(bytes: &[u8]) -> Option<tiny_skia::Pixmap> {
    if bytes.len() > MAX_BYTES || bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 1_048_576 {
        return None;
    }
    tiny_skia::Pixmap::decode_png(bytes).ok()
}

pub fn png(path: impl AsRef<Path>) -> Option<tiny_skia::Pixmap> {
    decode_png(&read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previews_reject_oversized_files_and_dimensions_before_decoding() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preview.png");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES as u64 + 1)
            .unwrap();
        assert!(read(&path).is_none());
        let mut bytes = tiny_skia::Pixmap::new(4, 4).unwrap().encode_png().unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(png(&path).unwrap().width(), 4);
        bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(decode_png(&bytes).is_none());
        assert!(read(directory.path()).is_none());
        let pipe = directory.path().join("pipe");
        let name = std::ffi::CString::new(pipe.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(read(pipe).is_none());
    }
}
