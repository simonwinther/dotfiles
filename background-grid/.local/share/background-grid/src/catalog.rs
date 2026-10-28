use crate::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Folder {
    pub name: String,
    pub images: Vec<PathBuf>,
    pub children: Vec<Folder>,
}

impl Folder {
    pub fn all_images(&self) -> Vec<PathBuf> {
        let mut paths = self.images.clone();
        for child in &self.children {
            paths.extend(child.all_images());
        }
        paths.sort_by_cached_key(|path| sort_key(path));
        paths.dedup();
        paths
    }
}

fn sort_key(path: &Path) -> (String, PathBuf) {
    (
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase(),
        path.to_path_buf(),
    )
}

fn supported(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        matches!(
            extension.to_string_lossy().to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tif" | "tiff" | "avif"
        )
    })
}

fn scan(
    path: &Path,
    folder: &mut Folder,
    visited: &mut HashSet<PathBuf>,
    cancelled: &impl Fn() -> bool,
) -> Result<()> {
    if cancelled() {
        return Err("Wallpaper scan cancelled".into());
    }
    if !path.is_dir() {
        return Ok(());
    }
    let real = fs::canonicalize(path)?;
    if !visited.insert(real) {
        return Ok(());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        if cancelled() {
            return Err("Wallpaper scan cancelled".into());
        }
        entries.push(entry?);
    }
    entries.sort_by_cached_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
    for entry in entries {
        if cancelled() {
            return Err("Wallpaper scan cancelled".into());
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if visited.contains(&fs::canonicalize(&path)?) {
                continue;
            }
            let index = folder
                .children
                .iter()
                .position(|child| child.name == name)
                .unwrap_or_else(|| {
                    folder.children.push(Folder {
                        name,
                        ..Folder::default()
                    });
                    folder.children.len() - 1
                });
            scan(&path, &mut folder.children[index], visited, cancelled)?;
        } else if path.is_file() && supported(&path) {
            let real = fs::canonicalize(path)?;
            if !folder.images.contains(&real) {
                folder.images.push(real);
            }
        }
    }
    folder.images.sort_by_cached_key(|path| sort_key(path));
    folder
        .children
        .sort_by_cached_key(|child| child.name.to_lowercase());
    Ok(())
}

pub fn catalog(roots: &[PathBuf]) -> Result<Folder> {
    catalog_with_cancel(roots, || false)
}

pub fn catalog_with_cancel(roots: &[PathBuf], cancelled: impl Fn() -> bool) -> Result<Folder> {
    let mut folder = Folder::default();
    let mut visited = HashSet::new();
    for root in roots {
        scan(root, &mut folder, &mut visited, &cancelled)?;
    }
    Ok(folder)
}

pub fn next_image(choices: &[PathBuf], current: &Path) -> Option<PathBuf> {
    if choices.is_empty() {
        None
    } else {
        let next = choices
            .iter()
            .position(|path| path == current)
            .map_or(0, |index| (index + 1) % choices.len());
        Some(choices[next].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::fs::symlink;

    #[test]
    fn catalog_keeps_nested_and_empty_categories_and_stops_symlink_cycles() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir_all(root.join("art/nested"))?;
        fs::create_dir(root.join("empty"))?;
        fs::create_dir(root.join(".hidden"))?;
        for name in [
            "Z.JPG",
            "a.png",
            "art/nested/test.webp",
            ".hidden/no.png",
            ".secret.png",
            "notes.txt",
        ] {
            fs::write(root.join(name), b"fixture")?;
        }
        symlink(root, root.join("art/cycle"))?;
        symlink(root.join("a.png"), root.join("alias.png"))?;
        let collection = catalog(&[root.to_path_buf(), root.join("art")])?;
        assert_eq!(
            collection.images,
            vec![root.join("a.png"), root.join("Z.JPG")]
        );
        assert_eq!(collection.children.len(), 2);
        assert_eq!(collection.children[0].name, "art");
        assert_eq!(
            collection.children[0].children[0].images,
            vec![root.join("art/nested/test.webp")]
        );
        assert!(collection.children[1].all_images().is_empty());
        assert_eq!(collection.all_images().len(), 3);
        Ok(())
    }

    #[test]
    fn next_cycles_and_handles_unknown_or_empty_selection() {
        let choices = vec![PathBuf::from("a.png"), PathBuf::from("b.png")];
        assert_eq!(
            next_image(&choices, Path::new("a.png")),
            Some(choices[1].clone())
        );
        assert_eq!(
            next_image(&choices, Path::new("b.png")),
            Some(choices[0].clone())
        );
        assert_eq!(
            next_image(&choices, Path::new("unknown")),
            Some(choices[0].clone())
        );
        assert_eq!(next_image(&[], Path::new("unknown")), None);
    }

    #[test]
    fn catalog_cancellation_interrupts_directory_enumeration() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for id in 0..100 {
            fs::write(temp.path().join(format!("{id}.png")), b"fixture")?;
        }
        let checks = Cell::new(0);
        let error = catalog_with_cancel(&[temp.path().to_path_buf()], || {
            checks.set(checks.get() + 1);
            checks.get() == 5
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "Wallpaper scan cancelled");
        assert_eq!(checks.get(), 5);
        Ok(())
    }
}
