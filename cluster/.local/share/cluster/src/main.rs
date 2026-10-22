mod process;
mod remote;
mod setup;
mod snapshot;
mod sync;

use anyhow::{Context, Result, ensure};
use clap::{CommandFactory, Parser, Subcommand};
use process::{git, git_config, quote, repo_root, shell_join, status_code};
use remote::{JobInfo, Submission};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "cluster",
    bin_name = "cluster",
    version,
    disable_help_flag = true,
    about = "Set up cluster tools, submit jobs, and read Slurm output over SSH.",
    after_help = "Use sync to upload code, fetch to download results, or submit --sync to upload and submit in one step."
)]
struct Cli {
    /// SSH alias or user@hostname; required for remote operations
    #[arg(
        id = "ssh_host",
        short = 'h',
        short_alias = 'H',
        long = "host",
        global = true
    )]
    host: Option<String>,
    /// Print help
    #[arg(id = "help", long = "help", global = true, action = clap::ArgAction::Help)]
    _help: Option<bool>,
    /// Run on this machine
    #[arg(long, conflicts_with = "ssh_host")]
    local: bool,
    #[arg(long, global = true, hide = true, env = "CLUSTER_SOURCE_ROOT")]
    source_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Check and install terminal tools and configurations
    Setup(setup::SetupArgs),
    /// Pair this repository with an existing cluster checkout
    Link {
        host: String,
        path: String,
        /// Validate without saving the pairing
        #[arg(long)]
        check: bool,
    },
    /// Mirror the current directory, deleting obsolete non-ignored target files
    Sync {
        /// Destination directory on the target; relative paths start at its home
        #[arg(long, value_name = "PATH")]
        to: Option<String>,
        /// Connect read-only to preview transfers and deletions
        #[arg(long)]
        dry_run: bool,
    },
    /// Download a directory without deleting local-only files
    Fetch {
        /// Source directory on the host; relative paths start at its home
        #[arg(long, value_name = "PATH")]
        from: Option<String>,
        /// Local destination directory, relative to the current directory
        #[arg(long, value_name = "PATH")]
        to: Option<PathBuf>,
        /// Preview downloads without changing local project files
        #[arg(long)]
        dry_run: bool,
    },
    /// Submit from the matching directory on the cluster
    Submit {
        /// Upload a fresh working-tree snapshot, respecting Git ignore rules
        #[arg(long)]
        sync: bool,
        /// Validate and print the submission without running sbatch
        #[arg(long)]
        dry_run: bool,
        /// Follow stdout after submission
        #[arg(short, long)]
        follow: bool,
        /// Add a job environment variable
        #[arg(long = "env", value_name = "NAME=value")]
        environment: Vec<String>,
        /// Use -- before sbatch flags and the batch script
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "SBATCH_ARGS"
        )]
        arguments: Vec<String>,
    },
    /// Read a job's actual Slurm output file
    Logs {
        /// Job ID or array task ID
        job: String,
        #[arg(short, long)]
        follow: bool,
        /// Read stderr instead of stdout
        #[arg(long)]
        stderr: bool,
        #[arg(short = 'n', long, default_value_t = 80, value_parser = clap::value_parser!(u64))]
        lines: u64,
    },
    /// Show your queued and running jobs
    Jobs,
    #[command(hide = true)]
    Agent { action: String, data: String },
}

fn selected_host(local: bool, host: Option<String>) -> Result<String> {
    ensure!(
        !(local && host.is_some()),
        "Choose either -h HOST or --local."
    );
    if local {
        return Ok("local".into());
    }
    let host = host.context(
        "Missing SSH host. Add -h HOST, for example: cluster sync -h hendrix (or use --local).",
    )?;
    remote::validate_host(&host)?;
    Ok(host)
}

fn link(host: &str, path: &str, check: bool) -> Result<()> {
    let root = repo_root()?;
    remote::validate_host(host)?;
    let info: remote::Repository =
        serde_json::from_value(remote::remote(host, "repo", json!({"path":path}))?)?;
    ensure!(
        remote::identity(&git(Some(&root), &["remote", "get-url", "origin"])?)
            == remote::identity(&info.origin),
        "These checkouts have different Git origins; the pairing was not saved."
    );
    let path = info
        .path
        .to_str()
        .context("Remote checkout path is not UTF-8")?;
    if !check {
        git(
            Some(&root),
            &["config", "--local", &format!("cluster.{host}.path"), path],
        )?;
    }
    println!(
        "{} -> {host}:{path}{}",
        root.display(),
        if check { " (check only)" } else { "" }
    );
    Ok(())
}

fn stream_logs(host: &str, job: &str, follow: bool, stderr: bool, lines: u64) -> Result<i32> {
    remote::validate_job(job)?;
    let cached = remote::cache_path(host, job)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let cached = cached
        .filter(|v| v["host"] == host && v["job"] == job)
        .and_then(|v| serde_json::from_value::<JobInfo>(v["info"].clone()).ok());
    let info = if let Some(info) = cached {
        info
    } else {
        let info = serde_json::from_value(remote::remote(host, "job", json!({"job":job}))?)?;
        remote::save_job(host, job, &info);
        info
    };
    let path = remote::log_path(&info, job, stderr)?;
    eprintln!(
        "{host}:{}{}",
        path.display(),
        if follow {
            " (Ctrl+C stops following; the job keeps running)"
        } else {
            ""
        }
    );
    let mut arguments = vec!["tail".into(), "-n".into(), lines.to_string()];
    if follow {
        arguments.push("-F".into());
    }
    arguments.extend([
        "--".into(),
        path.to_str().context("Log path is not UTF-8")?.to_owned(),
    ]);
    let mut command = if host == "local" {
        let mut command = Command::new("tail");
        command.args(&arguments[1..]);
        command
    } else {
        remote::ssh(host, None, &format!("exec {}", shell_join(&arguments)))?
    };
    Ok(status_code(command.status()?))
}

fn submit(
    local: bool,
    host: Option<String>,
    sync: bool,
    dry_run: bool,
    follow: bool,
    environment: Vec<String>,
    arguments: Vec<String>,
) -> Result<i32> {
    let host = selected_host(local, host)?;
    let root = repo_root()?;
    ensure!(
        !arguments.is_empty(),
        "Provide a batch script: cluster submit -h HOST -- path/to/train.sbatch"
    );
    ensure!(
        !arguments
            .iter()
            .any(|v| ["-Q", "--quiet", "--quiet=yes"].contains(&v.as_str())),
        "Remove --quiet; the helper needs sbatch's job ID."
    );
    let path = if sync {
        Some(String::new())
    } else if host == "local" {
        Some(
            root.to_str()
                .context("Checkout path is not UTF-8")?
                .to_owned(),
        )
    } else {
        git_config(&root, &format!("cluster.{host}.path"))
    };
    let path = path.filter(|p| sync || !p.is_empty()).with_context(|| format!("This checkout is not linked to {host}.\nLink it once: cluster link {} REMOTE_CHECKOUT_PATH", quote(&host)))?;
    ensure!(
        sync || git(
            Some(&root),
            &["status", "--porcelain", "--untracked-files=no"]
        )?
        .is_empty(),
        "Commit your local tracked changes, then push and pull on the cluster before submitting."
    );
    let mut env = BTreeMap::new();
    let name_regex = regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap();
    for entry in environment {
        let (name, value) = entry
            .split_once('=')
            .context("Use --env NAME=value for a job environment variable.")?;
        ensure!(
            name_regex.is_match(name),
            "Use --env NAME=value for a job environment variable."
        );
        env.insert(name.to_owned(), value.to_owned());
    }
    let data = Submission {
        path,
        subdir: std::env::current_dir()?
            .canonicalize()?
            .strip_prefix(&root)?
            .to_path_buf(),
        origin: if sync {
            git_config(&root, "remote.origin.url")
                .map(|url| remote::identity(&url))
                .unwrap_or_default()
        } else {
            remote::identity(&git(Some(&root), &["remote", "get-url", "origin"])?)
        },
        commit: git(Some(&root), &["rev-parse", "HEAD"])?,
        arguments,
        env,
        dry_run,
        snapshot: sync,
    };
    let answer = if sync {
        snapshot::submit(&host, &root, data)?
    } else {
        remote::remote(&host, "submit", serde_json::to_value(data)?)?
    };
    if dry_run {
        let env: BTreeMap<String, String> = serde_json::from_value(answer["env"].clone())?;
        let mut command = Vec::new();
        if !env.is_empty() {
            command.push("env".into());
            command.extend(env.into_iter().map(|(k, v)| format!("{k}={v}")));
        }
        command.extend(serde_json::from_value::<Vec<String>>(
            answer["command"].clone(),
        )?);
        println!(
            "{host}: cd {} && {}\nDry run: no job submitted.",
            quote(
                answer["cwd"]
                    .as_str()
                    .context("Missing submission directory")?
            ),
            shell_join(&command)
        );
        return Ok(0);
    }
    let job = answer["job"]
        .as_str()
        .context("Missing submitted job ID; check the queue before retrying")?;
    println!(
        "Submitted job {job} on {host} ({}).",
        answer["cwd"].as_str().unwrap_or("unknown directory")
    );
    std::io::stdout().flush()?;
    if let Some(info) = answer.get("info") {
        match serde_json::from_value::<JobInfo>(info.clone()) {
            Ok(info) => remote::save_job(&host, job, &info),
            Err(error) => eprintln!(
                "Job {job} was submitted, but its log information could not be read: {error}"
            ),
        }
    }
    println!(
        "Logs: cluster {} logs {job} --follow",
        if host == "local" {
            "--local".into()
        } else {
            format!("--host {}", quote(&host))
        }
    );
    std::io::stdout().flush()?;
    if follow {
        stream_logs(&host, job, true, false, 80)
    } else {
        Ok(0)
    }
}

fn main_result(cli: Cli) -> Result<i32> {
    match cli.command {
        Some(Commands::Setup(args)) => {
            setup::setup(args, cli.host, cli.local, cli.source_root).map(|()| 0)
        }
        Some(Commands::Link { host, path, check }) => link(&host, &path, check).map(|()| 0),
        Some(Commands::Sync { to, dry_run }) => {
            let host = selected_host(cli.local, cli.host)?;
            let to = to.filter(|p| !p.is_empty()).context(
                "Missing sync destination. Add --to PATH, for example: cluster sync -h hendrix --to dev/WMF.",
            )?;
            let root = repo_root()?;
            let source = std::env::current_dir()?.canonicalize()?;
            let path = sync::copy(&host, &root, &source, &to, dry_run)?;
            println!(
                "{} {host}:{}",
                if dry_run {
                    "Would sync to"
                } else {
                    "Synced to"
                },
                path.display()
            );
            Ok(0)
        }
        Some(Commands::Fetch { from, to, dry_run }) => {
            let host = selected_host(cli.local, cli.host)?;
            let from = from
                .filter(|p| !p.is_empty())
                .context("Missing fetch source. Add --from REMOTE_PATH.")?;
            let to = to
                .filter(|p| !p.as_os_str().is_empty())
                .context("Missing fetch destination. Add --to LOCAL_PATH.")?;
            let path = sync::fetch(&host, &from, &to, dry_run)?;
            println!(
                "{} {}",
                if dry_run {
                    "Would fetch to"
                } else {
                    "Fetched to"
                },
                path.display()
            );
            Ok(0)
        }
        Some(Commands::Submit {
            sync,
            dry_run,
            follow,
            environment,
            arguments,
        }) => submit(
            cli.local,
            cli.host,
            sync,
            dry_run,
            follow,
            environment,
            arguments,
        ),
        Some(Commands::Logs {
            job,
            follow,
            stderr,
            lines,
        }) => {
            let host = selected_host(cli.local, cli.host)?;
            stream_logs(&host, &job, follow, stderr, lines)
        }
        Some(Commands::Jobs) => {
            let host = selected_host(cli.local, cli.host)?;
            let script =
                r#"exec squeue --user "$(id -un)" --format="%.18i %.14j %.10T %.10M %.10l %R""#;
            let mut command = if host == "local" {
                let mut c = Command::new("sh");
                c.args(["-c", script]);
                c
            } else {
                remote::ssh(&host, None, script)?
            };
            Ok(status_code(command.status()?))
        }
        Some(Commands::Agent { action, data }) => {
            let answer = remote::agent(&action, serde_json::from_str(&data)?)?;
            serde_json::to_writer(std::io::stdout().lock(), &answer)?;
            println!();
            Ok(0)
        }
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(0)
        }
    }
}

fn main() {
    match main_result(Cli::parse()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("cluster: {error:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_options_have_no_conflicting_short_flags() {
        Cli::command().debug_assert();
    }

    #[test]
    fn origins_normalize_without_sending_credentials() {
        let expected = "github.com/example/project";
        for url in [
            "git@github.com:example/Project.git",
            "https://secret:token@github.com/example/Project.git/",
            "ssh://git@github.com/example/project.git",
        ] {
            assert_eq!(remote::identity(url), expected);
        }
    }

    #[test]
    fn shell_quoting_round_trips_spaces_quotes_and_newlines() {
        let values = vec![
            "a'b\nc $(false)".to_owned(),
            "".into(),
            "--flag=value".into(),
        ];
        let text = process::output(
            Command::new("sh").args(["-c", &format!("printf '%s\\n' {}", shell_join(&values))]),
        )
        .unwrap();
        assert_eq!(text, values.join("\n"));
    }

    #[test]
    fn parses_slurm_paths_with_spaces() {
        let info = remote::parse_job_info(
            "JobId=123 JobName=training run WorkDir=/home/a b StdOut=/logs/a'b.out StdErr=/logs/error log",
        );
        assert_eq!(info["JobName"], "training run");
        assert_eq!(info["WorkDir"], "/home/a b");
        assert_eq!(info["StdErr"], "/logs/error log");
    }

    #[test]
    fn array_patterns_and_padding() {
        let mut info = JobInfo::from([
            ("StdOut".into(), "/logs/%x-%A_%04a.out".into()),
            ("JobName".into(), "train".into()),
            ("ArrayJobId".into(), "123".into()),
            ("ArrayTaskId".into(), "2".into()),
        ]);
        assert_eq!(
            remote::log_path(&info, "123_2", false).unwrap(),
            PathBuf::from("/logs/train-123_0002.out")
        );
        info.insert("ArrayTaskId".into(), "0-10".into());
        assert!(remote::log_path(&info, "123", false).is_err());
    }

    #[test]
    fn log_patterns_reject_missing_paths_nodes_and_huge_padding() {
        for path in [
            "/dev/null",
            "(null)",
            "/logs/%N.out",
            "/logs/%9999999999999999999j.out",
            "relative.log",
        ] {
            assert!(
                remote::log_path(
                    &JobInfo::from([("StdOut".into(), path.into())]),
                    "123",
                    false
                )
                .is_err()
            );
        }
    }

    #[test]
    fn padding_and_relative_logs_keep_working_directory() {
        let info = JobInfo::from([
            ("StdOut".into(), "%x-%j_%a-%%.out".into()),
            ("JobName".into(), "train".into()),
            ("WorkDir".into(), "/work dir".into()),
        ]);
        assert_eq!(
            remote::log_path(&info, "123_2", false).unwrap(),
            PathBuf::from("/work dir/train-123_2-%.out")
        );
    }

    #[test]
    fn host_and_job_validation_reject_shell_fragments() {
        for host in ["-oProxyCommand=evil", "user@host;echo bad", "host\n", ""] {
            assert!(remote::validate_host(host).is_err());
        }
        for job in ["../123", "123;id", "123_[1-5]", ""] {
            assert!(remote::validate_job(job).is_err());
        }
        assert!(remote::validate_host("user@cluster.example").is_ok());
        assert!(remote::validate_job("123_0").is_ok());
    }
}
