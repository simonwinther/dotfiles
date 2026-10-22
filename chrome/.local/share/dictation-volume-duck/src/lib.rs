mod audio;
mod command;
pub mod native;
mod state;

use anyhow::{Context, Result, bail, ensure};
use command::Deadline;
use state::{State, Store};
use std::{env, time::Duration};

const HELP: &str = "Usage: dictation-volume-duck [start|stop|reset|status] [client] [factor]\n\nCommands:\n  start   Lower the default output volume for a client.\n  stop    Restore volume when the last active client stops.\n  reset   Force restore the saved volume and clear state on success.\n  status  Show current ducking state.\n\nBuild source changes with dictation-volume-duck-build.\n";

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Clone)]
pub enum Action {
    Start,
    Stop,
    Reset,
    Status,
}

pub struct Request {
    pub action: Action,
    pub client: String,
    pub factor: f64,
}

struct Settings {
    fade_in: u64,
    fade_out: u64,
    steps: u64,
}

impl Settings {
    fn load() -> Result<Self> {
        let number = |key: &str, default: u64, max: u64| -> Result<u64> {
            let Some(value) = env::var_os(key).filter(|s| !s.is_empty()) else {
                return Ok(default);
            };
            let n = value
                .to_str()
                .context("Non-UTF8 fade setting")?
                .parse::<i64>()
                .with_context(|| format!("Invalid {key}"))?;
            Ok((n.max(0) as u64).min(max))
        };
        Ok(Self {
            fade_in: number("VOLUME_DUCK_FADE_IN_MS", 700, 60_000)?,
            fade_out: number("VOLUME_DUCK_FADE_OUT_MS", 900, 60_000)?,
            steps: number("VOLUME_DUCK_FADE_STEPS", 12, 100)?.max(1),
        })
    }
}

pub fn factor(value: &str) -> Result<f64> {
    let number = value.parse::<f64>().context("Invalid volume duck factor")?;
    ensure!(number.is_finite(), "Invalid volume duck factor");
    Ok(if number <= 0. { 0.3 } else { number })
}

fn execute(request: Request, budget: Duration) -> Result<String> {
    ensure!(
        request.client.len() <= 256,
        "Volume client name is too long"
    );
    let deadline = Deadline::new(budget);
    let store = Store::lock(&deadline)?;
    let saved = store.load()?;
    if matches!(request.action, Action::Status) {
        return Ok(saved.map(|s| s.status()).unwrap_or_else(|| "idle\n".into()));
    }
    let settings = Settings::load()?;
    match request.action {
        Action::Start => {
            if let Some(mut state) = saved {
                state.audio.bind_legacy(&deadline)?;
                if !state.clients.is_empty() {
                    if !state.clients.contains(&request.client) {
                        state.clients.push(request.client);
                    }
                    store.save(&state)?;
                    return Ok(String::new());
                }
                // An earlier failed fade/restore still owns the original volume.
                state
                    .audio
                    .restore(settings.fade_out, settings.steps, &deadline)?;
                store.clear()?;
            }
            let audio = audio::Audio::current(&deadline)?;
            let target = audio.volume * request.factor;
            ensure!(target.is_finite(), "Invalid target volume");
            let mut state = State {
                audio,
                clients: vec![request.client],
            };
            store.save(&state)?;
            if let Err(error) = state.audio.fade(
                state.audio.volume,
                target,
                settings.fade_in,
                settings.steps,
                &deadline,
            ) {
                state.clients.clear();
                store.save(&state)?;
                // Attempt immediate rollback within the same operation budget.
                if state.audio.restore(0, 1, &deadline).is_ok() {
                    store.clear()?;
                }
                return Err(error);
            }
        }
        Action::Stop | Action::Reset => {
            if let Some(mut state) = saved {
                if matches!(request.action, Action::Reset) {
                    state.clients.clear();
                } else {
                    state.clients.retain(|c| c != &request.client);
                }
                if !state.clients.is_empty() {
                    store.save(&state)?;
                    return Ok(String::new());
                }
                state.audio.bind_legacy(&deadline)?;
                store.save(&state)?;
                state
                    .audio
                    .restore(settings.fade_out, settings.steps, &deadline)?;
                store.clear()?;
            }
        }
        Action::Status => unreachable!(),
    }
    Ok(String::new())
}

pub fn run(request: Request, budget: Duration) -> Outcome {
    match execute(request, budget) {
        Ok(stdout) => Outcome {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => Outcome {
            code: 1,
            stdout: String::new(),
            stderr: format!("dictation-volume-duck: {error:#}\n"),
        },
    }
}

pub fn cli(args: &[String]) -> Outcome {
    if matches!(
        args.first().map(String::as_str),
        Some("help" | "--help" | "-h")
    ) {
        return Outcome {
            code: 0,
            stdout: HELP.into(),
            stderr: String::new(),
        };
    }
    let parse = || -> Result<Request> {
        let action = match args.first().map(String::as_str).unwrap_or("status") {
            "start" => Action::Start,
            "stop" => Action::Stop,
            "reset" => Action::Reset,
            "status" => Action::Status,
            _ => bail!("Unknown command\n{HELP}"),
        };
        let value = args
            .get(2)
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| {
                env::var("VOLUME_DUCK_FACTOR")
                    .ok()
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "0.3".into());
        let factor = if matches!(action, Action::Start) {
            factor(&value)?
        } else {
            0.3
        };
        Ok(Request {
            action,
            client: state::sanitize_client(args.get(1).map(String::as_str).unwrap_or("default")),
            factor,
        })
    };
    match parse() {
        Ok(request) => run(request, Duration::from_secs(90)),
        Err(error) => Outcome {
            code: 2,
            stdout: String::new(),
            stderr: format!("dictation-volume-duck: {error:#}\n"),
        },
    }
}
