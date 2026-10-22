use crate::process::{run, with_input};
use crate::remote::SyncEntry;
use crate::{remote, snapshot};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

// Rsync treats backslashes literally unless the pattern contains wildcards.
// Escape literal path names, while leaving the intentional subtree suffix live.
fn rule(rules: &mut Vec<u8>, kind: u8, path: &Path, directory: bool, subtree: bool) {
    let name = path.as_os_str().as_bytes();
    let wildcard = subtree || name.iter().any(|b| b"*?[".contains(b));
    rules.extend([kind, b' ', b'/']);
    for byte in name {
        if b"*?[".contains(byte) || (*byte == b'\\' && wildcard) {
            rules.push(b'\\');
        }
        rules.push(*byte);
    }
    if directory {
        rules.extend(if subtree { b"/***".as_slice() } else { b"/" });
    }
    rules.push(0);
}

fn filters(
    root: &Path,
    prefix: &Path,
    files: &[PathBuf],
    entries: Vec<SyncEntry>,
) -> Result<Vec<u8>> {
    let selected: BTreeSet<_> = files.iter().cloned().collect();
    let mut query = Vec::new();
    let mut candidates = BTreeMap::new();
    let mut inspected = Vec::new();
    for entry in entries {
        if entry.path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        rule(&mut inspected, b'R', &entry.path, entry.directory, false);
        // Selected files win over ignore rules, including directory-to-file
        // replacements. Do not ask Git to inspect paths beyond a local symlink.
        if entry.path.ancestors().any(|p| selected.contains(p)) {
            continue;
        }
        // check-ignore rejects Git's literal-pathspec option. A leading ./
        // keeps colon-prefixed names from being parsed as pathspec magic.
        let mut name = Path::new(".")
            .join(prefix)
            .join(&entry.path)
            .as_os_str()
            .as_bytes()
            .to_vec();
        if entry.directory {
            name.push(b'/');
        }
        query.extend_from_slice(&name);
        query.push(0);
        candidates.insert(name, entry);
    }
    let mut rules = b"P .git\0".to_vec();
    if !query.is_empty() {
        let mut command = Command::new("git");
        command
            .args(["check-ignore", "--stdin", "-z"])
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let ignored = with_input(&mut command, query.as_slice())?;
        ensure!(
            matches!(ignored.status.code(), Some(0 | 1)),
            "Could not check destination paths against Git ignore rules: {}",
            String::from_utf8_lossy(&ignored.stderr).trim()
        );
        let mut protected_dirs = BTreeSet::new();
        let mut protected: Vec<_> = ignored
            .stdout
            .split(|b| *b == 0)
            .filter_map(|name| candidates.get(name))
            .collect();
        protected.sort_by(|a, b| a.path.cmp(&b.path));
        for entry in protected {
            if entry.path.ancestors().any(|p| protected_dirs.contains(p)) {
                continue;
            }
            rule(
                &mut rules,
                b'P',
                &entry.path,
                entry.directory,
                entry.directory,
            );
            if entry.directory {
                protected_dirs.insert(entry.path.clone());
            }
        }
    }
    // Only inspected paths may be deleted. A running job can create ignored
    // outputs between the inventory and rsync's scan; those must remain safe.
    rules.extend(inspected);
    rules.extend(b"P *\0");
    let mut directories = BTreeSet::new();
    for file in files {
        for directory in file
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            directories.insert(directory.to_path_buf());
        }
    }
    for directory in directories {
        rule(&mut rules, b'S', &directory, true, false);
    }
    for file in files {
        rule(&mut rules, b'S', file, false, false);
    }
    // Sender-only hiding keeps ignored local data out of the transfer without
    // protecting obsolete destination code from the receiver's deletion scan.
    rules.extend(b"H *\0");
    Ok(rules)
}

pub fn copy(
    host: &str,
    root: &Path,
    source: &Path,
    destination: &str,
    dry_run: bool,
) -> Result<PathBuf> {
    let prefix = source.strip_prefix(root)?;
    let files: Vec<_> = snapshot::files(root, prefix)?
        .iter()
        .filter_map(|name| {
            Path::new(std::ffi::OsStr::from_bytes(name))
                .strip_prefix(prefix)
                .ok()
                .map(Path::to_path_buf)
        })
        .collect();
    eprintln!(
        "Checking {host}:{destination} for mirror changes ({} source files)...",
        files.len()
    );
    let directory = remote::remote(
        host,
        "sync-destination",
        json!({
            "path": destination, "create": false, "list": true,
        }),
    )?;
    let path = PathBuf::from(
        directory["path"]
            .as_str()
            .context("Missing resolved destination")?,
    );
    let exists = directory["exists"]
        .as_bool()
        .context("Missing destination status")?;
    if host == "local" {
        ensure!(
            !path.starts_with(source) && !source.starts_with(&path),
            "Local sync source and destination cannot contain one another."
        );
    }
    let rules = filters(
        root,
        prefix,
        &files,
        serde_json::from_value(directory["entries"].clone())?,
    )?;
    if !exists && dry_run {
        println!("Would create destination {}", path.display());
        for file in &files {
            println!("would upload {:?}", file);
        }
        return Ok(path);
    }
    if !exists {
        remote::remote(
            host,
            "sync-destination",
            json!({"path": path, "create": true}),
        )?;
    }
    let staging = tempfile::tempdir()?;
    let filter = staging.path().join("filters");
    fs::write(&filter, rules)?;
    let mut from = source.as_os_str().to_os_string();
    from.push("/");
    let target = if host == "local" {
        format!("{}/", path.display())
    } else {
        format!("{host}:{}/", path.display())
    };
    eprintln!(
        "{} {} files from {} to {host}:{}",
        if dry_run {
            "Previewing mirror of"
        } else {
            "Mirroring"
        },
        files.len(),
        source.display(),
        path.display()
    );
    let mut command = Command::new("rsync");
    command
        .args([
            "--archive",
            "--checksum",
            "--protect-args",
            "--delete-after",
            "--from0",
        ])
        .arg(format!("--filter=merge {}", filter.display()));
    if dry_run {
        command.args(["--dry-run", "--itemize-changes"]);
    } else {
        command.args([
            "--human-readable",
            "--info=progress2,name1,flist2",
            "--outbuf=L",
        ]);
    }
    run(command.arg("--").arg(from).arg(target))?;
    Ok(path)
}

pub fn fetch(host: &str, source: &str, destination: &Path, dry_run: bool) -> Result<PathBuf> {
    let destination = remote::resolve_destination(&std::env::current_dir()?.join(destination))?;
    ensure!(
        !destination
            .components()
            .any(|c| c == Component::ParentDir || c.as_os_str() == ".git"),
        "Choose a local fetch destination without '..' or '.git' components."
    );
    let exists = destination.try_exists()?;
    ensure!(
        !exists || destination.is_dir(),
        "Fetch destination must be a directory."
    );
    let directory = remote::remote(host, "fetch-source", json!({"path": source}))?;
    let source = directory["path"]
        .as_str()
        .context("Missing resolved fetch source")?;
    if host == "local" {
        let source = Path::new(source);
        ensure!(
            !destination.starts_with(source) && !source.starts_with(&destination),
            "Local fetch source and destination cannot contain one another."
        );
    }
    // Preview a missing destination against an empty temporary directory. This
    // also works when several destination parents do not exist, without mkdir.
    let preview = if !exists && dry_run {
        println!("Would create destination {}", destination.display());
        Some(tempfile::tempdir()?)
    } else {
        if !exists {
            fs::create_dir_all(&destination)?;
        }
        None
    };
    let target = preview
        .as_ref()
        .map_or(destination.as_path(), |directory| directory.path());
    eprintln!(
        "{} {host}:{source} into {}",
        if dry_run {
            "Previewing fetch of"
        } else {
            "Fetching"
        },
        destination.display()
    );
    let from = if host == "local" {
        format!("{source}/")
    } else {
        format!("{host}:{source}/")
    };
    let mut command = Command::new("rsync");
    command.args([
        "--archive",
        "--checksum",
        "--protect-args",
        "--exclude=.git",
    ]);
    if dry_run {
        command.args(["--dry-run", "--itemize-changes"]);
    } else {
        command.args([
            "--human-readable",
            "--info=progress2,name1,flist2",
            "--outbuf=L",
        ]);
    }
    run(command.arg("--").arg(from).arg(target))?;
    Ok(destination)
}
