use crate::process::{run, shell_join, with_input};
use crate::remote::{self, Submission};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::fs;
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn files(root: &Path, scope: &Path) -> Result<Vec<Vec<u8>>> {
    let listed = Command::new("git")
        .arg("--literal-pathspecs")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .arg("--")
        .arg(if scope.as_os_str().is_empty() {
            Path::new(".")
        } else {
            scope
        })
        .current_dir(root)
        .output()?;
    ensure!(
        listed.status.success(),
        "Could not list the working tree with Git."
    );
    let mut files = Vec::new();
    for name in listed.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        let relative = Path::new(std::ffi::OsStr::from_bytes(name));
        ensure!(
            relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
            "Unexpected path in Git's file list: {}",
            relative.display()
        );
        if relative.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        // A directory replaced with a symlink leaves stale children in the index.
        // Copy the link itself, without following it to files outside the tree.
        if relative
            .ancestors()
            .skip(1)
            .any(|parent| fs::symlink_metadata(root.join(parent)).is_ok_and(|m| m.is_symlink()))
        {
            continue;
        }
        let metadata = match fs::symlink_metadata(root.join(relative)) {
            Ok(metadata) => metadata,
            // An unstaged directory-to-file replacement also leaves indexed
            // children behind, but those paths are no longer source files.
            Err(error)
                if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
            {
                continue;
            }
            Err(error) => return Err(error).context("Read snapshot file metadata"),
        };
        ensure!(
            metadata.is_file() || metadata.is_symlink(),
            "Cannot snapshot {}: only files and symlinks are supported (submodules need a separate checkout).",
            relative.display()
        );
        files.push(name.to_vec());
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn runs_root() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or(remote::home()?.join(".local/share"))
        .join("cluster/runs");
    ensure!(
        root.is_absolute(),
        "The snapshot run directory must be absolute."
    );
    Ok(root)
}

pub fn create() -> Result<Value> {
    let root = runs_root()?;
    fs::create_dir_all(&root)?;
    let run = tempfile::Builder::new().prefix("run-").tempdir_in(root)?;
    fs::create_dir(run.path().join("source"))?;
    Ok(json!({"path": run.keep().canonicalize()?}))
}

pub fn source_root(path: &str) -> Result<PathBuf> {
    let run = Path::new(path).canonicalize()?;
    let runs = runs_root()?
        .canonicalize()
        .context("Snapshot is outside the cluster run directory.")?;
    ensure!(
        run.parent() == Some(runs.as_path()),
        "Snapshot is outside the cluster run directory."
    );
    let manifest: Value = serde_json::from_slice(&fs::read(run.join("manifest.json"))?)?;
    ensure!(manifest["format"] == 1, "Unsupported snapshot manifest.");
    let source = run.join("source").canonicalize()?;
    ensure!(
        source == run.join("source"),
        "Snapshot source cannot be a symlink."
    );
    Ok(source)
}

pub fn submit(host: &str, root: &Path, mut data: Submission) -> Result<Value> {
    let path = upload(
        host,
        root,
        &data.commit,
        &data.origin,
        &data.subdir,
        data.dry_run,
    )?;
    if data.dry_run {
        let mut command = vec!["sbatch".to_owned(), "--parsable".to_owned()];
        command.extend(data.arguments);
        return Ok(json!({
            "cwd": path.join("source").join(data.subdir),
            "command": command, "env": data.env,
        }));
    }
    data.path = path
        .to_str()
        .context("Snapshot path is not UTF-8")?
        .to_owned();
    remote::remote(host, "submit", serde_json::to_value(data)?)
}

pub fn upload(
    host: &str,
    root: &Path,
    commit: &str,
    origin: &str,
    subdir: &Path,
    dry_run: bool,
) -> Result<PathBuf> {
    let files = files(root, Path::new(""))?;
    if dry_run {
        eprintln!(
            "Would sync {} files using Git ignore rules; no project files uploaded.",
            files.len()
        );
        return Ok(PathBuf::from("<new snapshot>"));
    }

    let staging = tempfile::tempdir()?;
    let archive = staging.path().join("snapshot.tar");
    let list = staging.path().join("files");
    let mut names = Vec::new();
    for name in &files {
        names.extend(name);
        names.push(0);
    }
    fs::write(&list, names)?;
    // Do not recurse or dereference: Git chose the exact files to transfer.
    // Transform member names only, preserving relative symlink targets.
    run(Command::new("tar")
        .arg("-C")
        .arg(root)
        .args(["--no-recursion", "--transform=flags=rh;s,^,source/,"])
        .args(["--null", "--verbatim-files-from", "-T"])
        .arg(&list)
        .arg("-cf")
        .arg(&archive))?;
    let diff = Command::new("git")
        .args([
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            commit,
            "--",
        ])
        .current_dir(root)
        .output()?;
    ensure!(
        diff.status.success(),
        "Could not record the working-tree diff."
    );
    fs::write(staging.path().join("changes.patch"), diff.stdout)?;
    fs::write(
        staging.path().join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "format": 1, "base_commit": commit, "origin": origin,
            "subdir": subdir, "file_count": files.len(),
        }))?,
    )?;
    run(Command::new("tar")
        .arg("-C")
        .arg(staging.path())
        .arg("-rf")
        .arg(&archive)
        .args(["manifest.json", "changes.patch"]))?;

    let created = remote::remote(host, "snapshot", json!({}))?;
    let path = created["path"]
        .as_str()
        .context("Missing remote snapshot path")?;
    eprintln!("Syncing {} files to {host}:{path}", files.len());
    let arguments = vec![
        "tar".into(),
        "-xpf".into(),
        "-".into(),
        "-C".into(),
        path.into(),
    ];
    let mut command = if host == "local" {
        let mut command = Command::new("tar");
        command.args(&arguments[1..]);
        command
    } else {
        remote::ssh(host, None, &format!("exec {}", shell_join(&arguments)))?
    };
    command.stderr(Stdio::inherit()).stdout(Stdio::piped());
    let uploaded = with_input(&mut command, fs::File::open(archive)?)?;
    ensure!(
        uploaded.status.success(),
        "Snapshot upload failed; no job was submitted."
    );
    ensure!(
        uploaded.stdout.is_empty(),
        "Unexpected upload output; no job was submitted."
    );
    Ok(PathBuf::from(path))
}
