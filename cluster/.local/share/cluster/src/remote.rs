use crate::process::{git, output, quote, shell_join, with_input};
use crate::snapshot;
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub type JobInfo = BTreeMap<String, String>;

pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

pub fn identity(url: &str) -> String {
    let scp = Regex::new(r"^[^/@:]+@([^/:]+):").unwrap();
    let scheme = Regex::new(r"^[a-zA-Z][a-zA-Z0-9+.-]*://(?:[^/@]+@)?").unwrap();
    let url = scp.replace(url.trim().trim_end_matches('/'), "$1/");
    let url = scheme.replace(&url, "");
    url.strip_suffix(".git").unwrap_or(&url).to_lowercase()
}

pub fn validate_host(host: &str) -> Result<&str> {
    ensure!(
        !host.starts_with('-')
            && Regex::new(r"^[A-Za-z0-9_@.\[\]:-]+$")
                .unwrap()
                .is_match(host),
        "Use an SSH alias or user@hostname; put ports and jump hosts in ~/.ssh/config."
    );
    Ok(host)
}

#[derive(Serialize, Deserialize)]
pub struct Repository {
    pub path: PathBuf,
    pub origin: String,
    pub commit: String,
    pub dirty: bool,
}

fn repository(path: &str) -> Result<Repository> {
    let path = if path == "~" {
        home()?
    } else if let Some(tail) = path.strip_prefix("~/") {
        home()?.join(tail)
    } else {
        PathBuf::from(path)
    };
    let path = if path.is_absolute() {
        path
    } else {
        home()?.join(path)
    };
    let root =
        PathBuf::from(git(Some(&path), &["rev-parse", "--show-toplevel"])?).canonicalize()?;
    Ok(Repository {
        origin: identity(&git(Some(&root), &["remote", "get-url", "origin"])?),
        commit: git(Some(&root), &["rev-parse", "HEAD"])?,
        dirty: !git(
            Some(&root),
            &["status", "--porcelain", "--untracked-files=no"],
        )?
        .is_empty(),
        path: root,
    })
}

pub fn parse_job_info(text: &str) -> JobInfo {
    let regex = Regex::new(r"(?:^|\s)([A-Za-z][A-Za-z0-9_]*)=").unwrap();
    let fields: Vec<_> = regex.captures_iter(text).collect();
    fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let end = fields
                .get(i + 1)
                .map_or(text.len(), |next| next.get(0).unwrap().start());
            (
                field[1].to_owned(),
                text[field.get(0).unwrap().end()..end].trim().to_owned(),
            )
        })
        .collect()
}

pub fn job_info(job: &str) -> Result<JobInfo> {
    validate_job(job)?;
    Ok(parse_job_info(&output(
        Command::new("scontrol").args(["-o", "show", "job", job]),
    )?))
}

#[derive(Serialize, Deserialize)]
pub struct SyncEntry {
    pub path: PathBuf,
    pub directory: bool,
}

pub(crate) fn resolve_destination(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(_) => path.canonicalize().map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().context("Missing destination parent")?;
            Ok(resolve_destination(parent)?
                .join(path.file_name().context("Missing destination name")?))
        }
        Err(error) => Err(error.into()),
    }
}

fn expand_path(path: &str) -> Result<PathBuf> {
    ensure!(!path.is_empty(), "A remote path is required.");
    Ok(if path == "~" {
        home()?
    } else if let Some(tail) = path.strip_prefix("~/") {
        home()?.join(tail)
    } else {
        ensure!(
            !path.starts_with('~'),
            "Use ~/PATH or an absolute destination path."
        );
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            home()?.join(path)
        }
    })
}

fn fetch_source(path: &str) -> Result<Value> {
    let path = expand_path(path)?
        .canonicalize()
        .context("Fetch source does not exist or cannot be accessed")?;
    ensure!(path.is_dir(), "Fetch source must be a directory.");
    Ok(json!({"path": path}))
}

fn sync_destination(path: &str, create: bool, list: bool) -> Result<Value> {
    ensure!(!path.is_empty(), "A sync destination is required.");
    let path = expand_path(path)?;
    ensure!(
        !path
            .components()
            .any(|c| c == Component::ParentDir || c.as_os_str() == ".git"),
        "Choose a project destination without '..' or '.git' components."
    );
    let path = resolve_destination(&path)?;
    ensure!(
        path.parent().is_some()
            && path != home()?.canonicalize()?
            && !path.components().any(|c| c.as_os_str() == ".git"),
        "Mirror sync requires a project directory, not the filesystem root, your home directory, or Git metadata."
    );
    if create {
        fs::create_dir_all(&path)?;
    }
    let exists = path.try_exists()?;
    ensure!(
        !exists || path.is_dir(),
        "Sync destination must be a directory."
    );
    let mut entries = Vec::new();
    if list && exists {
        let mut pending = vec![PathBuf::new()];
        while let Some(relative) = pending.pop() {
            for entry in fs::read_dir(path.join(&relative))? {
                let entry = entry?;
                let relative = relative.join(entry.file_name());
                let directory = entry.file_type()?.is_dir();
                if directory && entry.file_name() != ".git" {
                    pending.push(relative.clone());
                }
                entries.push(SyncEntry {
                    path: relative,
                    directory,
                });
            }
        }
    }
    Ok(json!({"path": path, "exists": exists, "entries": entries}))
}

#[derive(Serialize, Deserialize)]
pub struct Submission {
    pub path: String,
    pub subdir: PathBuf,
    pub origin: String,
    pub commit: String,
    pub arguments: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub dry_run: bool,
    #[serde(default)]
    pub snapshot: bool,
}

fn submit(data: Submission) -> Result<Value> {
    let root = if data.snapshot {
        snapshot::source_root(&data.path)?
    } else {
        let repo = repository(&data.path)?;
        ensure!(
            identity(&repo.origin) == identity(&data.origin),
            "The paired checkout has a different Git origin. Run cluster link again."
        );
        ensure!(
            repo.commit == data.commit,
            "Local and cluster commits differ. Push your local commit and pull it on the cluster first.\nLocal: {}\nCluster: {}",
            data.commit,
            repo.commit
        );
        ensure!(
            !repo.dirty,
            "The cluster checkout has uncommitted tracked changes. Resolve them before submitting."
        );
        repo.path
    };
    let workdir = root.join(&data.subdir).canonicalize().with_context(|| {
        format!(
            "Working directory is missing on the cluster: {}",
            root.join(&data.subdir).display()
        )
    })?;
    ensure!(
        workdir.starts_with(&root),
        "Working directory escapes the {}.",
        if data.snapshot {
            "snapshot"
        } else {
            "paired checkout"
        }
    );
    ensure!(
        workdir.is_dir(),
        "Working directory is missing on the cluster: {}",
        workdir.display()
    );
    let mut command = vec!["sbatch".to_owned(), "--parsable".to_owned()];
    command.extend(data.arguments);
    if data.dry_run {
        return Ok(json!({"cwd":workdir,"command":command,"env":data.env}));
    }
    // Execute exactly once. Neither transport errors nor malformed output cause
    // a retry: the scheduler may already have accepted the job.
    let result = output(
        Command::new(&command[0])
            .args(&command[1..])
            .current_dir(&workdir)
            .envs(&data.env),
    )?;
    ensure!(
        Regex::new(r"^[0-9]+(?:;[^\s;]+)?$")
            .unwrap()
            .is_match(&result),
        "sbatch returned no recognizable job ID; check the queue before retrying: {result}"
    );
    let job = result.split(';').next().unwrap();
    let mut answer = json!({"job":job,"cwd":workdir});
    match job_info(job) {
        Ok(info) => answer["info"] = serde_json::to_value(info)?,
        Err(error) => eprintln!("Job {job} was submitted, but log lookup failed: {error:#}"),
    }
    Ok(answer)
}

pub fn agent(action: &str, data: Value) -> Result<Value> {
    match action {
        "repo" => serde_json::to_value(repository(
            data["path"].as_str().context("Missing checkout path")?,
        )?)
        .map_err(Into::into),
        "job" => serde_json::to_value(job_info(data["job"].as_str().context("Missing job ID")?)?)
            .map_err(Into::into),
        "submit" => submit(serde_json::from_value(data)?),
        "snapshot" => snapshot::create(),
        "fetch-source" => fetch_source(data["path"].as_str().context("Missing fetch source")?),
        "sync-destination" => sync_destination(
            data["path"].as_str().context("Missing sync destination")?,
            data["create"].as_bool().unwrap_or(false),
            data["list"].as_bool().unwrap_or(false),
        ),
        _ => bail!("Unknown remote operation."),
    }
}

pub fn ssh(host: &str, port: Option<u16>, command: &str) -> Result<Command> {
    validate_host(host)?;
    let mut ssh = Command::new("ssh");
    if let Some(port) = port {
        ssh.args(["-p", &port.to_string()]);
    }
    ssh.args(["--", host, command]);
    Ok(ssh)
}

pub fn binary() -> Result<PathBuf> {
    let path = std::env::var_os("CLUSTER_REMOTE_BINARY")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?);
    path.canonicalize()
        .context("Locate Rust executable for the cluster")
}

pub fn binary_arch(path: &Path) -> Result<&'static str> {
    use std::io::Read;
    let mut header = [0; 20];
    fs::File::open(path)?.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"\x7fELF" && header[4] == 2 && header[5] == 1,
        "Remote helper must be a Linux 64-bit little-endian ELF executable"
    );
    match u16::from_le_bytes([header[18], header[19]]) {
        62 => Ok("x86_64"),
        183 => Ok("aarch64"),
        _ => bail!("Remote helper must target Linux x86_64 or ARM64"),
    }
}

pub fn arch_guard(arch: &str) -> String {
    let pattern = if arch == "aarch64" {
        "aarch64|arm64"
    } else {
        "x86_64"
    };
    format!("case \"$(uname -s):$(uname -m)\" in Linux:{pattern}) ;; *) printf '%s\\n' 'cluster: CPU architecture differs; set CLUSTER_REMOTE_BINARY to a static Rust build for this host.' >&2; exit 1 ;; esac\n").replace("Linux:aarch64|arm64", "Linux:aarch64|Linux:arm64")
}

pub fn remote(host: &str, action: &str, data: Value) -> Result<Value> {
    if host == "local" {
        return agent(action, data);
    }
    let executable = binary()?;
    let arch = binary_arch(&executable)?;
    let hash = format!("{:x}", Sha256::digest(fs::read(&executable)?));
    let cache = format!(
        "cache=\"${{XDG_CACHE_HOME:-$HOME/.cache}}/cluster/agents\"\nbinary=\"$cache/{hash}\"\n"
    );
    let invocation = shell_join(&["agent".into(), action.into(), serde_json::to_string(&data)?]);
    // 78 is reserved for an absent executable, before the operation is run.
    // Only that explicit bootstrap result may be followed by another attempt.
    let script = format!(
        "set -eu\n{}{cache}if [ ! -x \"$binary\" ]; then exit 78; fi\nexec \"$binary\" {invocation}",
        arch_guard(arch)
    );
    let command = format!("sh -c {}", quote(&script));
    let mut result = ssh(host, None, &command)?.stdin(Stdio::null()).output()?;
    if result.status.code() == Some(78) {
        let upload = format!(
            "set -eu\numask 077\n{}{cache}mkdir -p -- \"$cache\"\ntmp=$(mktemp \"$cache/incoming.XXXXXX\")\ntrap 'rm -f -- \"$tmp\"' EXIT HUP INT TERM\ncat > \"$tmp\"\nchmod 700 \"$tmp\"\nmv -f -- \"$tmp\" \"$binary\"\n",
            arch_guard(arch)
        );
        let mut upload_command = ssh(host, None, &format!("sh -c {}", quote(&upload)))?;
        upload_command
            .stderr(Stdio::inherit())
            .stdout(Stdio::piped());
        let uploaded = with_input(&mut upload_command, fs::File::open(&executable)?)?;
        ensure!(
            uploaded.status.success(),
            "Could not install the Rust cluster helper; no operation was attempted."
        );
        ensure!(
            uploaded.stdout.is_empty(),
            "Unexpected shell output while installing the helper; check shell startup files."
        );
        result = ssh(host, None, &command)?.stdin(Stdio::null()).output()?;
    }
    eprint!("{}", String::from_utf8_lossy(&result.stderr));
    ensure!(
        result.status.success(),
        "Cluster operation failed; no automatic retry was attempted.{}",
        if action == "submit" {
            " Check the queue before retrying a submission."
        } else {
            ""
        }
    );
    serde_json::from_slice(&result.stdout).with_context(|| {
        format!(
            "Expected JSON from the cluster; check for shell startup messages on stdout.{}",
            if action == "submit" {
                " Check the queue before retrying a submission."
            } else {
                ""
            }
        )
    })
}

pub fn cache_path(host: &str, job: &str) -> Result<PathBuf> {
    validate_job(job)?;
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".local/state"));
    let key = format!("{:x}", Sha256::digest(host.as_bytes()));
    Ok(state
        .join("cluster/jobs")
        .join(format!("{}-{job}.json", &key[..16])))
}

pub fn save_job(host: &str, job: &str, info: &JobInfo) {
    let save = || -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let path = cache_path(host, job)?;
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        serde_json::to_writer(&mut file, &json!({"host":host,"job":job,"info":info}))?;
        file.write_all(b"\n")?;
        file.persist(path)?;
        Ok(())
    };
    if let Err(error) = save() {
        eprintln!("Job {job}: could not save log paths: {error:#}");
    }
}

pub fn validate_job(job: &str) -> Result<()> {
    ensure!(
        Regex::new(r"^[0-9]+(?:_[0-9]+)?$").unwrap().is_match(job),
        "Use a numeric job ID, or an array task such as 12345_0."
    );
    Ok(())
}

pub fn log_path(info: &JobInfo, job: &str, stderr: bool) -> Result<PathBuf> {
    let path = info
        .get(if stderr { "StdErr" } else { "StdOut" })
        .filter(|v| !v.is_empty() && *v != "(null)" && *v != "/dev/null")
        .with_context(|| {
            format!(
                "This job has no {} log path.",
                if stderr { "stderr" } else { "stdout" }
            )
        })?;
    let (base, task) = job
        .split_once('_')
        .map_or((job, None), |(base, task)| (base, Some(task)));
    let regex = Regex::new(r"%([0-9]*)([%jAaxu])").unwrap();
    let mut error = None;
    let replaced = regex.replace_all(path, |caps: &regex::Captures<'_>| {
        let value = match &caps[2] {
            "%" => Some("%"),
            "j" => Some(info.get("JobId").map_or(base, String::as_str)),
            "A" => Some(info.get("ArrayJobId").map_or(base, String::as_str)),
            "a" => info
                .get("ArrayTaskId")
                .map(String::as_str)
                .or(task)
                .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())),
            "x" => info.get("JobName").map(String::as_str),
            "u" => Some(
                info.get("UserId")
                    .map_or("", String::as_str)
                    .split('(')
                    .next()
                    .unwrap(),
            ),
            _ => unreachable!(),
        };
        let Some(value) = value else {
            error = Some(format!(
                "Choose an array task such as {base}_0 to read its log."
            ));
            return String::new();
        };
        let width = if caps[1].is_empty() {
            0
        } else {
            caps[1].parse::<usize>().unwrap_or(usize::MAX)
        };
        if width > 4096 {
            error = Some("Slurm log padding width is too large.".into());
            return String::new();
        }
        format!("{value:0>width$}")
    });
    if let Some(error) = error {
        bail!("{error}");
    }
    ensure!(
        !Regex::new(r"%[0-9]*[A-Za-z]").unwrap().is_match(&replaced),
        "Node-specific log patterns are not supported: {replaced}"
    );
    let mut path = PathBuf::from(replaced.as_ref());
    if !path.is_absolute() {
        path = Path::new(info.get("WorkDir").map_or("", String::as_str)).join(path);
    }
    ensure!(
        path.is_absolute(),
        "Slurm returned a relative log path without a working directory."
    );
    Ok(path)
}
