use crate::{
    backend::Backend,
    launch,
    protocol::{Activation, FuzzyInfo, Item, Query, State},
    todo,
};
use anyhow::{Result, bail, ensure};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rusqlite::params;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub const NAMES: &[&str] = &[
    "bluetooth",
    "calc",
    "clipboard",
    "desktopapplications",
    "files",
    "menus",
    "providerlist",
    "runner",
    "symbols",
    "todo",
    "unicode",
    "websearch",
];
const URI_PATH: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC.remove(b'/');

pub fn query(
    backend: &Backend,
    request: &Query,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<Vec<Item>> {
    ensure!(
        request.query.len() <= 4096 && request.providers.len() <= 32,
        "Query exceeds its limit"
    );
    let limit = if request.maxresults <= 0 {
        256
    } else {
        request.maxresults.min(100_000) as usize
    };
    let names: Vec<_> = if request.providers.is_empty() {
        vec!["desktopapplications".into(), "websearch".into()]
    } else {
        request.providers.clone()
    };
    let mut items = vec![];
    for provider in &names {
        if cancelled() {
            return Ok(vec![]);
        }
        let mut entries = match provider.as_str() {
            "clipboard" => {
                let store = backend.store.lock().unwrap();
                let mode = if request.mode.is_empty() {
                    store.setting("clipboard_mode")?
                } else {
                    request.mode.clone()
                };
                store.list(&request.query, limit, &mode)?
            }
            "desktopapplications" => backend
                .applications
                .lock()
                .unwrap()
                .get(&backend.paths)
                .to_vec(),
            "files" => files(backend, &request.query, limit, cancelled)?,
            "runner" => runner(&request.query, limit, cancelled),
            "symbols" => symbols(&request.query, limit, cancelled),
            "unicode" => unicode(&request.query, limit, cancelled),
            "calc" => calculator(backend, &request.query, cancelled)?,
            "websearch" => websearch(backend, &request.query),
            "todo" => todo::query(backend, &request.query, limit)?,
            "bluetooth" => native("bluetooth", cancelled)?,
            "providerlist" => providerlist(backend)?,
            "menus" => {
                let (menu, query) = request
                    .query
                    .split_once(':')
                    .unwrap_or(("main", &request.query));
                menus(backend, menu, query, cancelled)?
            }
            value if value.starts_with("menus:") => {
                menus(backend, &value[6..], &request.query, cancelled)?
            }
            _ => vec![],
        };
        if matches!(
            provider.as_str(),
            "desktopapplications" | "providerlist" | "bluetooth"
        ) || (provider.starts_with("menus") && !provider.ends_with(":repositories"))
        {
            rank(backend, &mut entries, &request.query, request.exactsearch)?;
        }
        items.extend(entries);
    }
    if names.len() > 1 && items.iter().any(|item| item.provider != "websearch") {
        items.retain(|item| item.provider != "websearch");
    }
    // Clipboard and tasks already have deliberate recency and urgency order.
    if names.len() > 1 || !matches!(names[0].as_str(), "clipboard" | "todo") {
        items.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.text.to_lowercase().cmp(&b.text.to_lowercase()))
        });
    }
    items.truncate(limit);
    Ok(items)
}

fn match_score(query: &str, text: &str, exact: bool) -> Option<(i32, Vec<i32>)> {
    crate::fuzzy::indices(query, text, exact)
}

fn rank(backend: &Backend, items: &mut Vec<Item>, query: &str, exact: bool) -> Result<()> {
    let usage: HashMap<(String, String), i64> = {
        let store = backend.store.lock().unwrap();
        let mut statement = store
            .db
            .prepare("SELECT provider,id,count FROM usage ORDER BY last_used DESC LIMIT 10000")?;
        statement
            .query_map([], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    items.retain_mut(|item| {
        let main = match_score(query, &item.text, exact);
        let detail = match_score(query, &format!("{} {}", item.subtext, item.preview), exact);
        let Some((score, positions)) = main.or(detail.map(|(score, _)| (score - 100, vec![])))
        else {
            return false;
        };
        item.score += score
            + usage
                .get(&(item.provider.clone(), item.identifier.clone()))
                .copied()
                .unwrap_or(0)
                .min(100) as i32;
        item.fuzzyinfo = Some(FuzzyInfo {
            start: positions.first().copied().unwrap_or(0),
            field: "text".into(),
            positions,
        });
        true
    });
    Ok(())
}

fn files(
    backend: &Backend,
    query: &str,
    limit: usize,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<Item>> {
    let pattern = file_pattern(query);
    let output = crate::process::output(
        Command::new("fd")
            .args([
                "--hidden",
                "--ignore-case",
                "--regex",
                "--full-path",
                "--color",
                "never",
                "--print0",
                "--max-results",
                &limit.min(1000).to_string(),
                "--",
                &pattern,
            ])
            .arg(&backend.paths.home),
        Duration::from_secs(3),
        2 * 1024 * 1024,
        cancelled,
    )?;
    ensure!(output.status.success(), "File search failed");
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .filter_map(|bytes| std::str::from_utf8(bytes).ok())
        .map(|path| {
            let path = Path::new(path);
            let directory = path.is_dir();
            Item {
                identifier: path.to_string_lossy().into_owned(),
                text: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                subtext: path.parent().unwrap_or(path).to_string_lossy().into_owned(),
                provider: "files".into(),
                score: crate::fuzzy::score(query, &path.to_string_lossy(), false).unwrap_or(0)
                    as i32,
                icon: if directory {
                    "folder"
                } else {
                    "text-x-generic"
                }
                .into(),
                r#type: 1,
                actions: ["open", "opendir", "copypath", "copyfile", "refresh_index"]
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                ..Default::default()
            }
        })
        .collect())
}

fn file_pattern(query: &str) -> String {
    let mut pattern = String::new();
    for character in query.chars().filter(|character| !character.is_whitespace()) {
        if !pattern.is_empty() {
            pattern.push_str(".*");
        }
        if ".^$*+?()[]{}|\\".contains(character) {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern
}

fn runner(query: &str, limit: usize, cancelled: &impl Fn() -> bool) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut items = vec![];
    let first = query.split_whitespace().next().unwrap_or("");
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if cancelled() {
            return vec![];
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !seen.insert(name.clone()) {
                continue;
            }
            let Some((score, _)) = match_score(first, &name, false) else {
                continue;
            };
            if crate::applications::executable(entry.path().to_string_lossy().as_ref()).is_none() {
                continue;
            }
            items.push(Item {
                identifier: name.clone(),
                text: name,
                provider: "runner".into(),
                score,
                icon: "utilities-terminal".into(),
                actions: vec!["run".into(), "runterminal".into()],
                ..Default::default()
            });
            if items.len() >= limit * 4 {
                break;
            }
        }
        if items.len() >= limit * 4 {
            break;
        }
    }
    if !query.trim().is_empty() {
        items.push(Item {
            identifier: "generic".into(),
            text: query.into(),
            provider: "runner".into(),
            score: 1200,
            icon: "utilities-terminal".into(),
            actions: vec!["run".into(), "runterminal".into()],
            ..Default::default()
        });
    }
    items
}

fn symbols(query: &str, limit: usize, cancelled: &impl Fn() -> bool) -> Vec<Item> {
    let mut items = vec![];
    for emoji in emojis::iter() {
        if cancelled() {
            break;
        }
        let short = emoji.shortcode().unwrap_or("");
        let name = format!("{} {}", emoji.name(), short);
        let Some((score, _)) = match_score(query, &name, false) else {
            continue;
        };
        items.push(Item {
            identifier: emoji.as_str().into(),
            text: format!("{}  {}", emoji.as_str(), emoji.name()),
            subtext: short.into(),
            provider: "symbols".into(),
            icon: emoji.as_str().into(),
            score,
            actions: vec!["run_cmd".into()],
            ..Default::default()
        });
        if items.len() >= limit * 4 {
            break;
        }
    }
    items
}

fn unicode(query: &str, limit: usize, cancelled: &impl Fn() -> bool) -> Vec<Item> {
    if let Some(character) = unicode_names2::character(&query.trim().to_ascii_uppercase())
        && let Some(name) = unicode_names2::name(character)
    {
        return vec![Item {
            identifier: character.to_string(),
            text: format!("{character}  {name}"),
            subtext: format!("U+{:04X}", character as u32),
            provider: "unicode".into(),
            icon: character.to_string(),
            score: 10000,
            actions: vec!["run_cmd".into()],
            ..Default::default()
        }];
    }
    let mut items = vec![];
    let requested_code = query
        .trim()
        .strip_prefix("U+")
        .or_else(|| query.trim().strip_prefix("u+"))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok());
    let deadline = Instant::now() + Duration::from_millis(500);
    let range = if let Some(code) = requested_code {
        code..=code
    } else {
        32..=0x10ffff
    };
    for code in range {
        if code % 256 == 0 && (cancelled() || Instant::now() >= deadline) {
            break;
        }
        let Some(character) = char::from_u32(code) else {
            continue;
        };
        let Some(name) = unicode_names2::name(character) else {
            continue;
        };
        let name = name.to_string();
        let Some((score, _)) = (if requested_code == Some(code) {
            Some((1000, vec![]))
        } else {
            match_score(query, &name, false)
        }) else {
            continue;
        };
        items.push(Item {
            identifier: character.to_string(),
            text: format!("{character}  {name}"),
            subtext: format!("U+{code:04X}"),
            provider: "unicode".into(),
            icon: character.to_string(),
            score,
            actions: vec!["run_cmd".into()],
            ..Default::default()
        });
        if items.len() >= limit * 4 {
            break;
        }
    }
    items
}

fn calculator(
    backend: &Backend,
    query: &str,
    cancelled: &(impl Fn() -> bool + Sync),
) -> Result<Vec<Item>> {
    if query.trim().is_empty() {
        let store = backend.store.lock().unwrap();
        let mut statement = store.db.prepare(
            "SELECT key,value FROM settings WHERE key LIKE 'calc:%' ORDER BY rowid DESC LIMIT 1000",
        )?;
        return Ok(statement
            .query_map([], |row| {
                Ok(Item {
                    identifier: row.get::<_, String>(0)?,
                    text: row.get(1)?,
                    provider: "calc".into(),
                    actions: vec!["copy".into(), "delete".into()],
                    ..Default::default()
                })
            })?
            .collect::<rusqlite::Result<_>>()?);
    }
    struct Interrupt<'a, F> {
        deadline: Instant,
        cancelled: &'a F,
    }
    impl<F: Fn() -> bool + Sync> fend_core::Interrupt for Interrupt<'_, F> {
        fn should_interrupt(&self) -> bool {
            (self.cancelled)() || Instant::now() >= self.deadline
        }
    }
    let interrupt = Interrupt {
        deadline: Instant::now() + Duration::from_millis(150),
        cancelled,
    };
    let mut context = fend_core::Context::new();
    match fend_core::evaluate_with_interrupt(query, &mut context, &interrupt) {
        Ok(value) => {
            let text = value.get_main_result().to_owned();
            if text.len() > 20_000 {
                return Ok(vec![]);
            }
            Ok(vec![Item {
                identifier: text.clone(),
                text,
                subtext: query.into(),
                provider: "calc".into(),
                score: 10000,
                actions: vec!["copy".into(), "save".into()],
                ..Default::default()
            }])
        }
        Err(_) => Ok(vec![]),
    }
}

fn websearch(backend: &Backend, query: &str) -> Vec<Item> {
    if query.trim().is_empty() {
        return vec![];
    }
    let term = utf8_percent_encode(query, NON_ALPHANUMERIC).to_string();
    let mut items: Vec<_> = backend
        .config
        .websearch
        .iter()
        .enumerate()
        .map(|(index, engine)| Item {
            identifier: index.to_string(),
            text: format!("{}: {}", engine.name, query),
            subtext: engine.url.replace("%TERM%", &term),
            provider: "websearch".into(),
            icon: "web-browser".into(),
            actions: vec!["search".into()],
            ..Default::default()
        })
        .collect();
    if query.starts_with("https://") || query.starts_with("http://") {
        items.insert(
            0,
            Item {
                identifier: query.into(),
                text: query.into(),
                provider: "websearch".into(),
                icon: "web-browser".into(),
                score: 100,
                actions: vec!["open_url".into()],
                ..Default::default()
            },
        );
    }
    items
}

#[derive(Deserialize)]
#[serde(default)]
struct Native {
    id: String,
    name: String,
    detail: String,
    icon: String,
    command: Vec<String>,
}
impl Default for Native {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            detail: String::new(),
            icon: String::new(),
            command: vec![],
        }
    }
}
fn native(provider: &str, cancelled: &impl Fn() -> bool) -> Result<Vec<Item>> {
    let output = crate::process::output(
        Command::new("desktop-bar").args(["--catalog", provider]),
        Duration::from_secs(4),
        2 * 1024 * 1024,
        cancelled,
    )?;
    ensure!(output.status.success(), "Desktop catalog is unavailable");
    let entries: Vec<Native> = serde_json::from_slice(&output.stdout)?;
    entries
        .into_iter()
        .map(|entry| {
            Ok(Item {
                identifier: serde_json::to_string(&entry.command)?,
                text: entry.name,
                subtext: entry.detail,
                icon: entry.icon,
                provider: provider.into(),
                actions: vec!["activate".into()],
                ..Default::default()
            })
        })
        .collect()
}

fn menu_config(backend: &Backend) -> Result<serde_json::Value> {
    let path = std::env::var_os("DESKTOP_MENUS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| backend.paths.config.join("desktop/menus.json"));
    ensure!(
        path.metadata()?.len() <= 8 * 1024 * 1024,
        "Menu configuration is too large"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn providerlist(backend: &Backend) -> Result<Vec<Item>> {
    let mut items: Vec<_> = NAMES
        .iter()
        .filter(|name| **name != "providerlist")
        .map(|name| Item {
            identifier: (*name).into(),
            text: (*name).into(),
            provider: "providerlist".into(),
            actions: vec!["activate".into()],
            ..Default::default()
        })
        .collect();
    let mut menus: Vec<_> = menu_config(backend)
        .ok()
        .and_then(|value| value.as_object().map(|map| map.keys().cloned().collect()))
        .unwrap_or_default();
    menus.extend(["repositories".into(), "notifications".into()]);
    for name in menus {
        items.push(Item {
            identifier: format!("menus:{name}"),
            text: name,
            provider: "providerlist".into(),
            actions: vec!["activate".into()],
            ..Default::default()
        });
    }
    Ok(items)
}
fn menus(
    backend: &Backend,
    name: &str,
    query: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Vec<Item>> {
    if name == "repositories" {
        let parts: Vec<_> = query.split_whitespace().collect();
        let valid = |text: &str| {
            !text.is_empty()
                && text != "."
                && text != ".."
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        };
        let owner = if parts.len() == 2 {
            parts[0]
        } else {
            backend.config.repository_owner.as_str()
        };
        if !valid(owner) || parts.len() > 2 {
            return Ok(vec![]);
        }
        let (text, url) = if parts.is_empty() {
            (
                "My repositories".into(),
                format!("https://github.com/{owner}?tab=repositories"),
            )
        } else {
            let repository = parts[parts.len() - 1];
            if !valid(repository) {
                return Ok(vec![]);
            }
            (
                format!("{owner}/{repository}"),
                format!("https://github.com/{owner}/{repository}"),
            )
        };
        return Ok(vec![Item {
            identifier: url.clone(),
            text,
            subtext: url,
            provider: "menus:repositories".into(),
            icon: "github".into(),
            actions: vec!["activate".into()],
            ..Default::default()
        }]);
    }
    if name == "notifications" {
        let mut items = native("notifications", cancelled)?;
        for item in &mut items {
            item.identifier = format!("{}\n{}", item.text, item.subtext);
            item.provider = "menus:notifications".into();
            item.actions = vec!["copy".into()];
        }
        return Ok(items);
    }
    let value = menu_config(backend)?;
    let Some(entries) = value
        .get(name)
        .and_then(|group| group.get("items"))
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(vec![]);
    };
    entries
        .iter()
        .map(|(label, command)| {
            Ok(Item {
                identifier: serde_json::to_string(command)?,
                text: label.clone(),
                provider: format!("menus:{name}"),
                actions: vec!["activate".into()],
                ..Default::default()
            })
        })
        .collect()
}

pub fn state(backend: &Backend, provider: &str) -> Result<State> {
    if provider == "todo" {
        return todo::state(backend);
    }
    let mut state = State {
        provider: provider.into(),
        ..Default::default()
    };
    if provider == "desktop-search" {
        let store = backend.store.lock().unwrap();
        let (items, pins): (i64, i64) = store.db.query_row(
            "SELECT COUNT(*),COALESCE(SUM(pinned),0) FROM clips",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        state.states = vec![
            "ready".into(),
            format!("clipboard_items={items}"),
            format!("pinned_items={pins}"),
        ];
    }
    if provider == "clipboard" {
        let store = backend.store.lock().unwrap();
        let mode = store.setting("clipboard_mode")?;
        state.states.push(if mode.is_empty() {
            "show_combined".into()
        } else {
            mode
        });
        let paused = backend.paused.load(Ordering::Acquire);
        state
            .states
            .push(if paused { "paused" } else { "unpaused" }.into());
        state.actions = [
            "show_images_only",
            "show_text_only",
            "show_pinned_only",
            "show_combined",
            "remove_all",
            if paused { "unpause" } else { "pause" },
        ]
        .iter()
        .map(|value| (*value).into())
        .collect();
    } else if provider == "files" {
        state.actions.push("refresh_index".into());
    } else if provider == "calc" {
        state.actions.push("delete_all".into());
    }
    Ok(state)
}

pub fn activate(backend: &Backend, action: &Activation) -> Result<()> {
    ensure!(
        action.identifier.len() <= 1024 * 1024
            && action.query.len() <= 4096
            && action.arguments.len() <= 4096,
        "Activation exceeds its limit"
    );
    match action.provider.as_str() {
        "clipboard" => clipboard_action(backend, action)?,
        "todo" => todo::activate(backend, action)?,
        "desktopapplications" => {
            ensure!(
                ["start", "new_instance", ""].contains(&action.action.as_str()),
                "Unsupported application action"
            );
            ensure!(
                backend
                    .applications
                    .lock()
                    .unwrap()
                    .get(&backend.paths)
                    .iter()
                    .any(|item| item.identifier == action.identifier),
                "Application no longer exists"
            );
            launch::start(
                &[
                    "desktopctl".into(),
                    "bar".into(),
                    "launch".into(),
                    action.identifier.clone(),
                ],
                &action.context,
            )?;
        }
        "runner" => {
            ensure!(
                ["run", "runterminal"].contains(&action.action.as_str()),
                "Unsupported runner action"
            );
            // Shell syntax is intentional only in the explicit command runner.
            let command = if action.identifier == "generic" {
                action.query.clone()
            } else if action.query.split_whitespace().next() == Some(action.identifier.as_str()) {
                action.query.clone()
            } else {
                format!("{} {}", action.identifier, action.arguments)
            };
            ensure!(!command.trim().is_empty(), "Command is empty");
            let mut args = if action.action == "runterminal" {
                backend.config.terminal.clone()
            } else {
                vec![]
            };
            args.extend(["sh".into(), "-c".into(), command]);
            launch::start(&args, &action.context)?;
        }
        "files" => {
            if action.action == "refresh_index" {
                backend.events.notify("files");
                return Ok(());
            }
            let path = Path::new(&action.identifier);
            ensure!(path.is_absolute() && path.exists(), "File no longer exists");
            match action.action.as_str() {
                "open" | "" => launch::start(
                    &["xdg-open".into(), action.identifier.clone()],
                    &action.context,
                )?,
                "opendir" => launch::start(
                    &[
                        "xdg-open".into(),
                        path.parent().unwrap_or(path).to_string_lossy().into_owned(),
                    ],
                    &action.context,
                )?,
                "copypath" => {
                    launch::copy_text(backend, &action.identifier, "text/plain;charset=utf-8")?
                }
                "copyfile" => launch::copy_text(
                    backend,
                    &format!(
                        "file://{}\r\n",
                        utf8_percent_encode(&action.identifier, URI_PATH)
                    ),
                    "text/uri-list",
                )?,
                _ => bail!("Unsupported file action"),
            }
        }
        "symbols" | "unicode" => {
            ensure!(
                ["run_cmd", "copy"].contains(&action.action.as_str()),
                "Unsupported symbol action"
            );
            launch::copy_text(backend, &action.identifier, "text/plain;charset=utf-8")?;
        }
        "calc" => {
            let store = backend.store.lock().unwrap();
            match action.action.as_str() {
                "save" => store.set_setting(
                    &format!("calc:{}", chrono::Local::now().timestamp_micros()),
                    &action.identifier,
                )?,
                "delete" => {
                    store.db.execute(
                        "DELETE FROM settings WHERE key=?1 AND key LIKE 'calc:%'",
                        [&action.identifier],
                    )?;
                }
                "delete_all" => {
                    store
                        .db
                        .execute("DELETE FROM settings WHERE key LIKE 'calc:%'", [])?;
                }
                "copy" => {
                    let value = if action.identifier.starts_with("calc:") {
                        store.setting(&action.identifier)?
                    } else {
                        action.identifier.clone()
                    };
                    drop(store);
                    launch::copy_text(backend, &value, "text/plain;charset=utf-8")?;
                }
                _ => bail!("Unsupported calculator action"),
            }
            backend.events.notify("calc");
        }
        "websearch" => {
            let url = if action.action == "open_url" {
                action.identifier.clone()
            } else {
                let index = action.identifier.parse::<usize>()?;
                backend
                    .config
                    .websearch
                    .get(index)
                    .ok_or_else(|| anyhow::anyhow!("Search engine no longer exists"))?
                    .url
                    .replace(
                        "%TERM%",
                        &utf8_percent_encode(&action.query, NON_ALPHANUMERIC).to_string(),
                    )
            };
            ensure!(
                url.starts_with("https://") || url.starts_with("http://"),
                "Search requires an HTTP URL"
            );
            launch::start(&["xdg-open".into(), url], &action.context)?;
        }
        "bluetooth" => {
            let args: Vec<String> = serde_json::from_str(&action.identifier)?;
            ensure!(
                args.first().is_some_and(|value| value == "native")
                    && args.get(1).is_some_and(|value| value == "bluetooth"),
                "Unsupported Bluetooth action"
            );
            launch::start(
                &[
                    vec!["desktop-bar".into(), "--action".into()],
                    args[1..].to_vec(),
                ]
                .concat(),
                "",
            )?;
        }
        "providerlist" => {}
        "menus:repositories" => {
            ensure!(
                action.identifier.starts_with("https://github.com/"),
                "Invalid repository URL"
            );
            launch::start(
                &["xdg-open".into(), action.identifier.clone()],
                &action.context,
            )?;
        }
        "menus:notifications" => {
            launch::copy_text(backend, &action.identifier, "text/plain;charset=utf-8")?
        }
        value if value.starts_with("menus") => {
            let args: Vec<String> = serde_json::from_str(&action.identifier)?;
            ensure!(!args.is_empty(), "Menu command is empty");
            launch::start(&[vec!["desktopctl".into()], args].concat(), &action.context)?;
        }
        _ => bail!("Unsupported provider"),
    }
    if !matches!(action.provider.as_str(), "clipboard" | "todo") {
        backend.store.lock().unwrap().db.execute("INSERT INTO usage(provider,id,count,last_used) VALUES(?1,?2,1,?3) ON CONFLICT(provider,id) DO UPDATE SET count=MIN(count+1,1000000),last_used=excluded.last_used",params![action.provider,action.identifier,chrono::Local::now().timestamp()])?;
    }
    Ok(())
}

fn clipboard_action(backend: &Backend, action: &Activation) -> Result<()> {
    match action.action.as_str() {
        "pause" => return backend.pause(true),
        "unpause" => return backend.pause(false),
        "show_images_only" | "show_text_only" | "show_pinned_only" | "show_combined" => backend
            .store
            .lock()
            .unwrap()
            .set_setting("clipboard_mode", &action.action)?,
        "pin" => backend
            .store
            .lock()
            .unwrap()
            .pin(&action.identifier, true)?,
        "unpin" => backend
            .store
            .lock()
            .unwrap()
            .pin(&action.identifier, false)?,
        "remove" => backend.store.lock().unwrap().remove(&action.identifier)?,
        "remove_all" => backend.store.lock().unwrap().clear_unpinned()?,
        "copy" | "paste" | "edit" => {
            let (mime, path) = backend.store.lock().unwrap().payload(&action.identifier)?;
            if action.action == "edit" {
                // Editors receive a private copy; immutable history blobs are never edited in place.
                let directory = backend.paths.state.join("edits");
                crate::config::private_directory(&directory)?;
                let target = directory.join(format!(
                    "{}-{}.{}",
                    chrono::Local::now().timestamp_micros(),
                    std::process::id(),
                    path.extension().unwrap_or_default().to_string_lossy()
                ));
                std::fs::copy(&path, &target)?;
                let args = if mime.starts_with("image/") {
                    vec![
                        "satty".into(),
                        "--filename".into(),
                        target.to_string_lossy().into_owned(),
                    ]
                } else {
                    vec![
                        "desktopctl".into(),
                        "launch".into(),
                        "editor".into(),
                        target.to_string_lossy().into_owned(),
                    ]
                };
                launch::start(&args, &action.context)?;
            } else {
                launch::copy_file(&path, &mime)?;
                if action.action == "paste" {
                    launch::start(&["desktopctl".into(), "clipboard-paste".into()], "")?;
                }
            }
        }
        _ => bail!("Unsupported clipboard action"),
    }
    backend.events.notify("clipboard");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_queries_match_subsequences_without_treating_characters_as_regex() {
        assert_eq!(file_pattern("nxcfg"), "n.*x.*c.*f.*g");
        assert_eq!(file_pattern("a.b"), "a.*\\..*b");
        assert_eq!(file_pattern("$HOME;"), "\\$.*H.*O.*M.*E.*;");
    }
    #[test]
    fn fuzzy_search_retains_literal_names_and_query_order() {
        assert!(match_score("fb", "Firefox Browser", false).is_some());
        assert!(match_score("bf", "Firefox Browser", false).is_none());
        assert!(match_score("fb", "Firefox Browser", true).is_none());
        assert_eq!(
            match_score("$HOME;", "Literal $HOME; name", true)
                .unwrap()
                .1,
            vec![8, 9, 10, 11, 12, 13]
        );
    }
}
