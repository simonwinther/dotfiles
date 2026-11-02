use crate::{config::Paths, protocol::Item};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

#[derive(Default)]
pub struct Catalog {
    entries: Vec<Item>,
    scanned: Option<Instant>,
    directories: Vec<(PathBuf, Option<SystemTime>)>,
}

pub fn data_directories(paths: &Paths) -> Vec<PathBuf> {
    let mut directories = vec![paths.data.clone()];
    for path in std::env::split_paths(
        &std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ) {
        if !directories.contains(&path) {
            directories.push(path);
        }
    }
    directories
}

pub fn executable(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let candidate = Path::new(name);
    let executable = |path: &Path| {
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if candidate.components().count() > 1 {
        return executable(candidate).then(|| candidate.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join(name))
        .find(|path| executable(path))
}

impl Catalog {
    pub fn get(&mut self, paths: &Paths) -> &[Item] {
        let directories: Vec<_> = data_directories(paths)
            .into_iter()
            .map(|root| {
                let path = root.join("applications");
                let modified = path.metadata().and_then(|m| m.modified()).ok();
                (path, modified)
            })
            .collect();
        if self
            .scanned
            .is_none_or(|time| time.elapsed() >= Duration::from_secs(60))
            || self.directories != directories
        {
            self.entries = scan(
                &directories
                    .iter()
                    .map(|(path, _)| path.clone())
                    .collect::<Vec<_>>(),
                &data_directories(paths),
            );
            self.directories = directories;
            self.scanned = Some(Instant::now());
        }
        &self.entries
    }
}

fn ini(path: &Path) -> Option<HashMap<String, String>> {
    if path.metadata().ok()?.len() > 1024 * 1024 {
        return None;
    }
    let source = std::fs::read_to_string(path).ok()?;
    let mut values = HashMap::new();
    let mut active = false;
    for raw in source.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            active = line == "[Desktop Entry]";
            continue;
        }
        if !active || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.insert(
                key.trim().into(),
                value
                    .trim()
                    .replace("\\s", " ")
                    .replace("\\n", "\n")
                    .replace("\\t", "\t")
                    .replace("\\\\", "\\"),
            );
        }
    }
    Some(values)
}

fn collect(root: &Path, directory: &Path, depth: usize, entries: &mut Vec<PathBuf>) {
    if depth > 8 || entries.len() >= 20_000 {
        return;
    }
    let Ok(reader) = std::fs::read_dir(directory) else {
        return;
    };
    let mut paths: Vec<_> = reader
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    for path in paths {
        if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
            && path.is_file()
        {
            entries.push(path);
        } else if path.symlink_metadata().is_ok_and(|m| m.is_dir()) && path.starts_with(root) {
            collect(root, &path, depth + 1, entries);
        }
    }
}

fn scan(roots: &[PathBuf], data: &[PathBuf]) -> Vec<Item> {
    let locale = std::env::var("LC_ALL")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("LC_MESSAGES").ok())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_else(|| "en".into());
    let language = locale.split('.').next().unwrap_or("en");
    let short = language.split('_').next().unwrap_or(language);
    let desktops = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "Hyprland".into());
    let desktops: HashSet<_> = desktops.split(':').collect();
    let mut seen = HashSet::new();
    let mut entries = vec![];
    let mut icons = Icons::new(data);
    for root in roots {
        let mut files = vec![];
        collect(root, root, 0, &mut files);
        for path in files {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let identity = relative.to_string_lossy().replace('/', "-");
            if !seen.insert(identity) {
                continue;
            }
            let Some(values) = ini(&path) else {
                continue;
            };
            let value = |key: &str| values.get(key).map(String::as_str).unwrap_or("");
            if (!value("Type").is_empty() && value("Type") != "Application")
                || ["Hidden", "NoDisplay"]
                    .iter()
                    .any(|key| value(key).eq_ignore_ascii_case("true"))
            {
                continue;
            }
            if !value("TryExec").is_empty() && executable(value("TryExec")).is_none() {
                continue;
            }
            if !value("OnlyShowIn").is_empty()
                && !value("OnlyShowIn")
                    .split(';')
                    .any(|name| desktops.contains(name))
            {
                continue;
            }
            if value("NotShowIn")
                .split(';')
                .filter(|name| !name.is_empty())
                .any(|name| desktops.contains(name))
            {
                continue;
            }
            let translated = |key: &str| {
                values
                    .get(&format!("{key}[{language}]"))
                    .or_else(|| values.get(&format!("{key}[{short}]")))
                    .or_else(|| values.get(key))
                    .map(String::as_str)
                    .unwrap_or("")
            };
            let name = translated("Name");
            if name.is_empty()
                || (value("Exec").is_empty()
                    && !value("DBusActivatable").eq_ignore_ascii_case("true"))
            {
                continue;
            }
            let detail = if translated("GenericName").is_empty() {
                translated("Comment")
            } else {
                translated("GenericName")
            };
            let canonical = path.canonicalize().unwrap_or(path);
            entries.push(Item {
                identifier: canonical.to_string_lossy().into_owned(),
                text: name.into(),
                subtext: detail.into(),
                icon: icons.resolve(value("Icon")),
                provider: "desktopapplications".into(),
                actions: vec!["start".into(), "new_instance".into()],
                preview: translated("Keywords").into(),
                ..Default::default()
            });
        }
    }
    entries.sort_by_cached_key(|item| item.text.to_lowercase());
    entries
}

struct Icons {
    directories: Vec<PathBuf>,
    cache: HashMap<String, String>,
}
impl Icons {
    fn new(data: &[PathBuf]) -> Self {
        let mut directories = vec![];
        for root in data {
            for theme in ["hicolor", "Adwaita", "Yaru"] {
                for size in ["scalable", "48x48", "32x32", "64x64", "256x256", "symbolic"] {
                    let path = root.join("icons").join(theme).join(size).join("apps");
                    if path.is_dir() {
                        directories.push(path);
                    }
                }
            }
            let path = root.join("pixmaps");
            if path.is_dir() {
                directories.push(path);
            }
        }
        Self {
            directories,
            cache: HashMap::new(),
        }
    }
    fn resolve(&mut self, name: &str) -> String {
        if name.is_empty() {
            return String::new();
        }
        if let Some(value) = self.cache.get(name) {
            return value.clone();
        }
        let path = Path::new(name);
        let found = if path.is_absolute() && path.is_file() {
            Some(path.to_owned())
        } else {
            self.directories.iter().find_map(|directory| {
                ["svg", "png", "xpm"]
                    .iter()
                    .map(|extension| directory.join(format!("{name}.{extension}")))
                    .find(|path| path.is_file())
            })
        };
        let result = found
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.into());
        self.cache.insert(name.into(), result.clone());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_masks_nested_entries_and_invalid_desktop_files_are_omitted() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("user");
        let system = root.path().join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(system.join("nested")).unwrap();
        let file = |path: &Path, body: &str| {
            std::fs::write(path, format!("[Desktop Entry]\n{body}")).unwrap()
        };
        file(
            &system.join("hidden.desktop"),
            "Name=System\nExec=fixture\n",
        );
        file(&user.join("hidden.desktop"), "Hidden=true\n");
        file(
            &system.join("nested/app.desktop"),
            "Name=Literal $HOME; name\nExec=fixture --quoted\nKeywords=notes;writing;\n",
        );
        file(
            &system.join("wrong.desktop"),
            "Name=Wrong\nExec=fixture\nOnlyShowIn=MissingDesktop;\n",
        );
        file(
            &system.join("missing.desktop"),
            "Name=Missing\nTryExec=desktop-search-nonexistent-fixture\nExec=fixture\n",
        );
        let entries = scan(&[user, system], &[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Literal $HOME; name");
        assert_eq!(entries[0].preview, "notes;writing;");
    }
}
