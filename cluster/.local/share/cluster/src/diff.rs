use crate::process::{repo_root, with_input};
use crate::{remote, snapshot, sync};
use anyhow::{Context, Result, ensure};
use crossterm::style::{Color, Stylize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::IsTerminal;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

struct Entry {
    code: String,
    name: String,
    path: PathBuf,
}

// Rsync escapes control characters, invalid UTF-8 and ambiguous backslashes as
// \#ooo. Decode for Git/filesystem lookups; retain the safe spelling for display.
fn decode(name: &str) -> PathBuf {
    let bytes = name.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\\#")
            && i + 5 <= bytes.len()
            && bytes[i + 2..i + 5]
                .iter()
                .all(|b| (b'0'..=b'7').contains(b))
        {
            decoded.push(
                ((bytes[i + 2] - b'0') << 6) | ((bytes[i + 3] - b'0') << 3) | (bytes[i + 4] - b'0'),
            );
            i += 5;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    PathBuf::from(OsString::from_vec(decoded))
        .components()
        .collect()
}

fn scan(
    host: &str,
    directory: &str,
    source: &Path,
    checksum: bool,
    filters: &[u8],
) -> Result<Vec<Entry>> {
    let directory = if directory == "~" {
        "."
    } else {
        directory.strip_prefix("~/").unwrap_or(directory)
    };
    ensure!(
        !directory.starts_with('~'),
        "Use ~/PATH, an absolute path, or a path relative to the host's home."
    );
    let path = Path::new(directory);
    let from = if host == "local" {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            remote::home()?.join(path)
        };
        format!("{}/", path.display())
    } else {
        remote::validate_host(host)?;
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(".").join(path)
        };
        format!("{host}:{}/", path.display())
    };
    let mut command = Command::new("rsync");
    command.args([
        "--dry-run",
        "--recursive",
        "--links",
        "--delete",
        "--protect-args",
        "--exclude=.git",
        "--out-format=%i\t%n",
        if checksum {
            "--checksum"
        } else {
            "--ignore-times"
        },
        "--rsh=ssh -o BatchMode=yes -o ConnectTimeout=10 -o ConnectionAttempts=1",
    ]);
    if !filters.is_empty() {
        command.args(["--from0", "--filter=merge -"]);
    }
    command
        .arg("--")
        .arg(from)
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = with_input(&mut command, filters)?;
    ensure!(
        output.status.success(),
        "Comparison failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    let output = String::from_utf8(output.stdout).context("Comparison output is not UTF-8")?;
    let mut entries = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let (code, name) = line
            .split_once('\t')
            .context("Unexpected rsync comparison output")?;
        ensure!(
            code.len() == 11 && code.is_ascii(),
            "Unexpected rsync change code: {code:?}"
        );
        if name == "./" || name == "." {
            continue;
        }
        let path = decode(name);
        ensure!(
            path.components().all(|c| matches!(c, Component::Normal(_))),
            "Unexpected comparison path: {name}"
        );
        entries.push(Entry {
            code: code.into(),
            name: name.into(),
            path,
        });
    }
    Ok(entries)
}

struct CodeOnly {
    root: PathBuf,
    prefix: PathBuf,
    selected: BTreeSet<PathBuf>,
}

impl CodeOnly {
    fn new(source: &Path) -> Result<Self> {
        let root = repo_root().context("--code-only requires a local Git working tree")?;
        let prefix = source.strip_prefix(&root)?.to_path_buf();
        let selected = snapshot::files(&root, &prefix)?
            .into_iter()
            .map(|name| {
                Path::new(OsStr::from_bytes(&name))
                    .strip_prefix(&prefix)
                    .map(Path::to_path_buf)
                    .map_err(Into::into)
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            root,
            prefix,
            selected,
        })
    }

    fn selected(&self, path: &Path) -> bool {
        path.ancestors().any(|p| self.selected.contains(p))
    }

    fn ignored(&self, entries: &[Entry]) -> Result<BTreeMap<PathBuf, bool>> {
        let mut paths = BTreeMap::new();
        for entry in entries {
            // Never ask Git to inspect a path beyond a local symlink. Tracked
            // files and non-ignored local paths also win over ignore patterns.
            if self.selected(&entry.path) {
                continue;
            }
            let link = entry.path.ancestors().find(|p| {
                fs::symlink_metadata(self.root.join(&self.prefix).join(p))
                    .is_ok_and(|m| m.is_symlink())
            });
            let path = link.unwrap_or(&entry.path);
            for parent in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
                if self.selected(parent) {
                    continue;
                }
                let directory = parent != path || (link.is_none() && entry.name.ends_with('/'));
                let mut name = Path::new(".")
                    .join(&self.prefix)
                    .join(parent)
                    .as_os_str()
                    .as_bytes()
                    .to_vec();
                if directory {
                    name.push(b'/');
                }
                paths.insert(name, (parent.to_path_buf(), directory));
            }
        }
        if paths.is_empty() {
            return Ok(BTreeMap::new());
        }
        let input: Vec<u8> = paths
            .keys()
            .flat_map(|p| p.iter().copied().chain([0]))
            .collect();
        let result = with_input(
            Command::new("git")
                .args(["check-ignore", "--stdin", "-z"])
                .current_dir(&self.root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
            input.as_slice(),
        )?;
        ensure!(
            matches!(result.status.code(), Some(0 | 1)),
            "Could not check Git ignore rules: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
        Ok(result
            .stdout
            .split(|b| *b == 0)
            .filter_map(|p| paths.get(p).cloned())
            .collect())
    }

    fn filters(&self, ignored: &BTreeMap<PathBuf, bool>) -> Vec<u8> {
        let mut filters = Vec::new();
        let directories: BTreeSet<_> = self
            .selected
            .iter()
            .flat_map(|p| p.ancestors().skip(1))
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        for directory in directories {
            sync::rule(&mut filters, b'S', directory, true, false);
        }
        for path in &self.selected {
            sync::rule(&mut filters, b'S', path, false, false);
        }
        for (path, directory) in ignored {
            // Hide sender data only. Receiver-only ignored entries can safely
            // appear as simulated deletions, then be removed from the report.
            sync::rule(&mut filters, b'H', path, *directory, *directory);
        }
        filters
    }
}

fn heading(text: &str, color: Color, colored: bool) {
    if colored {
        println!("{}", text.with(color).bold());
    } else {
        println!("{text}");
    }
}

pub fn compare(host: &str, directory: &str, code_only: bool, all: bool) -> Result<()> {
    let source = std::env::current_dir()?.canonicalize()?;
    let scope = code_only.then(|| CodeOnly::new(&source)).transpose()?;
    eprintln!("Comparing with {host}:{directory}...");
    let filters = if let Some(scope) = &scope {
        let inventory = scan(host, directory, &source, false, &[])?;
        scope.filters(&scope.ignored(&inventory)?)
    } else {
        Vec::new()
    };
    let mut entries = scan(host, directory, &source, true, &filters)?;
    if let Some(scope) = &scope {
        let ignored = scope.ignored(&entries)?;
        entries.retain(|entry| {
            scope.selected(&entry.path) || !entry.path.ancestors().any(|p| ignored.contains_key(p))
        });
    }
    let mut local = Vec::new();
    let mut remote = Vec::new();
    let mut changed = Vec::new();
    for entry in entries {
        if entry.code.starts_with("*deleting") {
            local.push(entry.name);
        } else if entry.code.contains('+')
            && fs::symlink_metadata(source.join(&entry.path)).is_err()
        {
            remote.push(entry.name);
        } else {
            changed.push(entry.name);
        }
    }
    let colored = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    println!("\nLocal   {}\nRemote  {host}:{directory}", source.display());
    println!(
        "Scope   {}\n",
        if code_only {
            "Code only (local Git ignore rules; tracked files included)"
        } else {
            "Full folder (including runs and caches; excluding .git)"
        }
    );
    let count = local.len() + remote.len() + changed.len();
    if count == 0 {
        heading(
            "In sync — names, types and contents match.",
            Color::Green,
            colored,
        );
    } else {
        heading(
            &format!("Different — {count} paths"),
            Color::Yellow,
            colored,
        );
        println!(
            "{} local only · {} remote only · {} changed",
            local.len(),
            remote.len(),
            changed.len()
        );
        for (label, mut names, color) in [
            ("Only on this PC".to_owned(), local, Color::Cyan),
            (format!("Only on {host}"), remote, Color::Magenta),
            (
                "Different contents or type".to_owned(),
                changed,
                Color::Yellow,
            ),
        ] {
            if names.is_empty() {
                continue;
            }
            names.sort();
            println!();
            heading(&format!("{label} ({})", names.len()), color, colored);
            let limit = if all { names.len() } else { 20 };
            for name in names.iter().take(limit) {
                println!("  {name}");
            }
            if names.len() > limit {
                println!(
                    "  … {} more; use --all to show every path",
                    names.len() - limit
                );
            }
        }
        println!("\nDifferent contents do not establish which version is newer.");
    }
    println!("Timestamps and permissions are not compared. Neither side was changed.");
    Ok(())
}
