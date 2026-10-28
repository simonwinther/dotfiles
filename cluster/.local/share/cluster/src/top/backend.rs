//! Read-only queries run locally, or in the same executable over SSH.
use crate::top::{
    Args,
    data::{self, Job, Snapshot},
    history::{self, LogView},
    hosts::{self, Target},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const PROTOCOL: u8 = 1;
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
enum Request {
    Snapshot {
        partition: String,
        timeout: f64,
    },
    History {
        user: String,
        days: u16,
        timeout: f64,
    },
    Log {
        job: Box<Job>,
        timeout: f64,
    },
    Reload {
        paths: Vec<PathBuf>,
        title: String,
    },
    FullLog {
        paths: Vec<PathBuf>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum Reply {
    Snapshot { data: Snapshot, user: String },
    History(Vec<Job>),
    Log(Box<LogView>),
    Text(String),
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    protocol: u8,
    reply: Reply,
}
fn timeout(seconds: f64) -> Result<Duration> {
    if !seconds.is_finite() || !(0.1..=86400.0).contains(&seconds) {
        bail!("Invalid query timeout");
    }
    Ok(Duration::from_secs_f64(seconds))
}
fn username() -> Result<String> {
    if let Ok(user) = std::env::var("USER")
        && !user.is_empty()
    {
        return Ok(user);
    }
    Ok(data::command("id", &["-un"], Duration::from_secs(2))?
        .trim()
        .into())
}
fn execute(request: Request) -> Result<Reply> {
    Ok(match request {
        Request::Snapshot {
            partition,
            timeout: seconds,
        } => Reply::Snapshot {
            data: data::fetch(&partition, timeout(seconds)?)?,
            user: username()?,
        },
        Request::History {
            user,
            days,
            timeout: seconds,
        } => {
            use clap::Parser;
            if !(1..=365).contains(&days) {
                bail!("Invalid history window");
            }
            let mut args = Args::parse_from(["mytop"]);
            args.user = if user.is_empty() { username()? } else { user };
            args.history_days = days;
            args.command_timeout = timeout(seconds)?.as_secs_f64();
            Reply::History(history::fetch(&args)?)
        }
        Request::Log {
            job,
            timeout: seconds,
        } => Reply::Log(Box::new(history::read_log(&job, timeout(seconds)?))),
        Request::Reload { paths, title } => Reply::Log(Box::new(history::reload(&paths, &title))),
        Request::FullLog { paths } => Reply::Text(history::full_log_text(&paths)?),
    })
}
pub fn answer(value: serde_json::Value) -> Result<serde_json::Value> {
    if serde_json::to_string(&value)?.len() > 128 * 1024 {
        bail!("Query request is too large");
    }
    let request: Request = serde_json::from_value(value).context("Invalid cluster top query")?;
    let reply = execute(request)?;
    Ok(serde_json::to_value(Envelope {
        protocol: PROTOCOL,
        reply,
    })?)
}
fn call(args: &Args, request: Request) -> Result<Reply> {
    let Some(target) = &args.target else {
        bail!("Select a cluster with c, or use cluster top -h HOST");
    };
    let Target::Ssh(host) = target else {
        return execute(request);
    };
    hosts::validate(host)?;
    let full_log = matches!(request, Request::FullLog { .. });
    let prepared = crate::remote::prepare_agent(host, "top", &serde_json::to_value(&request)?)?;
    let duration = timeout(args.command_timeout)?.mul_f64(3.0) + Duration::from_secs(6);
    let limit = if full_log {
        64 * 1024 * 1024 * 6 + 1024 * 1024
    } else {
        16 * 1024 * 1024
    };
    let options = [
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "ConnectionAttempts=1",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=1",
    ];
    let query = || -> Result<data::Capture> {
        let mut command = crate::remote::ssh_with_options(host, None, &prepared.command, &options)?;
        command.stdin(std::process::Stdio::null());
        data::capture(&mut command, duration, limit, 1024 * 1024)
    };
    let mut result = query().with_context(|| format!("Connect to {host}"))?;
    if result.status.code() == Some(78) {
        let mut upload = crate::remote::ssh_with_options(host, None, &prepared.upload, &options)?;
        upload.stdin(std::fs::File::open(&prepared.executable)?);
        let uploaded =
            data::capture(&mut upload, duration, 1024 * 1024, 1024 * 1024)?.text("ssh")?;
        if !uploaded.is_empty() {
            bail!("Unexpected output while installing cluster helper on {host}");
        }
        result = query()?;
    }
    let output = result
        .text("ssh")
        .with_context(|| format!("Query {host}"))?;
    let envelope: Envelope = serde_json::from_str(&output).with_context(|| {
        format!("Invalid cluster top reply from {host}; check shell startup output")
    })?;
    if envelope.protocol != PROTOCOL {
        bail!("Incompatible cluster monitor reply from {host}");
    }
    Ok(envelope.reply)
}
pub fn snapshot(args: &Args) -> Result<(Snapshot, String)> {
    match call(
        args,
        Request::Snapshot {
            partition: args.partition.clone(),
            timeout: args.command_timeout,
        },
    )? {
        Reply::Snapshot { mut data, user } => {
            data.fetched = Some(Instant::now());
            Ok((
                data,
                if args.user.is_empty() {
                    user
                } else {
                    args.user.clone()
                },
            ))
        }
        _ => bail!("Unexpected snapshot reply"),
    }
}
pub fn history(args: &Args) -> Result<Vec<Job>> {
    match call(
        args,
        Request::History {
            user: args.user.clone(),
            days: args.history_days,
            timeout: args.command_timeout,
        },
    )? {
        Reply::History(jobs) => Ok(jobs),
        _ => bail!("Unexpected history reply"),
    }
}
pub fn read_log(args: &Args, job: &Job) -> Result<LogView> {
    match call(
        args,
        Request::Log {
            job: Box::new(job.clone()),
            timeout: args.command_timeout,
        },
    )? {
        Reply::Log(log) => Ok(*log),
        _ => bail!("Unexpected log reply"),
    }
}
pub fn reload(args: &Args, paths: &[PathBuf], title: &str) -> Result<LogView> {
    match call(
        args,
        Request::Reload {
            paths: paths.to_vec(),
            title: title.into(),
        },
    )? {
        Reply::Log(log) => Ok(*log),
        _ => bail!("Unexpected log reply"),
    }
}
pub fn full_log(args: &Args, paths: &[PathBuf]) -> Result<String> {
    match call(
        args,
        Request::FullLog {
            paths: paths.to_vec(),
        },
    )? {
        Reply::Text(text) => Ok(text),
        _ => bail!("Unexpected log text reply"),
    }
}
pub fn log_error(error: anyhow::Error) -> LogView {
    LogView {
        title: "Log unavailable".into(),
        lines: vec![
            format!("{error:#}"),
            "Press q to return; c opens clusters from the main view.".into(),
        ],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_and_replies_preserve_remote_paths_and_text() {
        let request = Request::Reload {
            paths: vec![PathBuf::from("/remote/a'b $(no)/job.out")],
            title: "job".into(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let command = crate::process::quote(&json);
        assert!(command.contains("'\\''"));
        let Request::Reload { paths, .. } = serde_json::from_str(&json).unwrap() else {
            panic!()
        };
        assert_eq!(paths[0], PathBuf::from("/remote/a'b $(no)/job.out"));
        let envelope = Envelope {
            protocol: PROTOCOL,
            reply: Reply::Text("line\n\u{1b}[31mraw\n".into()),
        };
        let result: Envelope =
            serde_json::from_str(&serde_json::to_string(&envelope).unwrap()).unwrap();
        let Reply::Text(text) = result.reply else {
            panic!()
        };
        assert_eq!(text, "line\n\u{1b}[31mraw\n");
    }
    #[test]
    fn log_views_round_trip_and_timeouts_are_bounded() {
        let view = history::reload(&[], "no paths");
        let reply = Reply::Log(Box::new(view));
        let copy: Reply = serde_json::from_str(&serde_json::to_string(&reply).unwrap()).unwrap();
        let Reply::Log(log) = copy else { panic!() };
        assert_eq!(log.title, "no paths");
        for seconds in [-1.0, 0.0, f64::NAN, f64::INFINITY, 86401.0] {
            assert!(timeout(seconds).is_err());
        }
        assert!(timeout(8.0).is_ok());
    }
}
