use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use dictate::{
    config::{self, Config},
    desktop, evaluation, network, session,
    text::Processor,
};
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "dictate",
    version,
    about = "Native streaming and batch dictation for Hyprland"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Stream microphone audio and show a live transcript.
    Live(Mode),
    /// Record, transcribe, retry, or discard a clip.
    Batch(Mode),
    /// Cancel whichever mode is active.
    Cancel,
    /// Show session and recovery status without a network request.
    Status,
    /// Check local dependencies, settings, and credential presence.
    Check,
    /// List supported transcription models.
    Models,
    /// Create or validate personal vocabulary.
    Dictionary {
        #[arg(value_parser=["init","path","check"])]
        action: String,
    },
    /// Compare preprocessing variants (makes paid requests).
    Bench {
        source: PathBuf,
        factors: Vec<f64>,
        #[arg(long, alias = "lang", default_value = "")]
        language: String,
    },
    /// Evaluate recordings or score saved transcripts offline.
    Eval {
        #[command(subcommand)]
        action: Eval,
    },
}
#[derive(Args)]
struct Mode {
    #[arg(value_parser=["start","stop","cancel","toggle","once","status","retry","discard"])]
    action: String,
    #[arg(long, alias = "lang", default_value = "")]
    language: String,
    #[arg(long, default_value = "")]
    model: String,
}
#[derive(Subcommand)]
enum Eval {
    Init {
        #[arg(default_value = "benchmarks/scientific")]
        directory: PathBuf,
    },
    Check {
        manifest: PathBuf,
    },
    Run {
        manifest: PathBuf,
        #[arg(long, alias = "lang", default_value = "")]
        language: String,
        #[arg(long, default_value = "")]
        model: String,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Score {
        manifest: PathBuf,
        transcripts: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Dictate failed: {error:#}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "_worker") {
        return session::worker(
            args.get(2)
                .ok_or_else(|| anyhow::anyhow!("Missing session descriptor"))?
                .parse()?,
        )
        .await;
    }
    let cli = Cli::parse();
    let mut c = Config::load()?;
    match cli.command {
        Commands::Live(mode) | Commands::Batch(mode) => {
            let live = args[1] == "live";
            ensure!(
                !live || !matches!(mode.action.as_str(), "once" | "retry" | "discard"),
                "That action is available in batch mode"
            );
            if !mode.language.is_empty() {
                config::validate_language(&mode.language)?;
            }
            session::action(
                c,
                if live { "live" } else { "batch" },
                &mode.action,
                &mode.language,
                &mode.model,
            )
            .await
        }
        Commands::Cancel => session::action(c, "batch", "cancel", "", "").await,
        Commands::Status => session::action(c, "batch", "status", "", "").await,
        Commands::Check => {
            c.validate()?;
            c.key()?;
            let p = Processor::load(&c)?;
            network::live_update(&c, &p, "")?;
            network::batch_fields(&c, &p, "")?;
            for name in ["pw-record", "stdbuf", "wl-copy"] {
                ensure!(desktop::exists(name), "Missing required command: {name}");
            }
            println!(
                "Rust dictation {}: configuration valid; API key configured",
                env!("CARGO_PKG_VERSION")
            );
            println!(
                "Live: {} (delay={}); batch: {}",
                c.get("LIVE_MODEL"),
                c.get("LIVE_DELAY"),
                c.get("TRANSCRIBE_MODEL")
            );
            println!(
                "Dictionary: {} terms, {} corrections",
                p.dictionary.terms.len(),
                p.dictionary.replacements.len()
            );
            for name in ["ffmpeg", "ffprobe", "hyprctl", "wpctl", "notify-send"] {
                println!(
                    "{name}: {}",
                    if desktop::exists(name) {
                        "available"
                    } else {
                        "optional, missing"
                    }
                );
            }
            println!(
                "Wayland overlay: {}",
                if desktop::overlay_path().is_file() {
                    "installed"
                } else {
                    "missing; run dictate-build"
                }
            );
            Ok(())
        }
        Commands::Models => {
            println!(
                "Batch: gpt-transcribe, gpt-4o-transcribe, gpt-4o-mini-transcribe, whisper-1\nLive: gpt-live-transcribe\nCurrent: {} / {}",
                c.get("TRANSCRIBE_MODEL"),
                c.get("LIVE_MODEL")
            );
            Ok(())
        }
        Commands::Dictionary { action } => {
            let path = c.path("DICTIONARY_FILE");
            match action.as_str() {
                "path" => println!(
                    "{}",
                    if c.get("DICTIONARY_FILE").is_empty() {
                        "Dictionary disabled".into()
                    } else {
                        path.display().to_string()
                    }
                ),
                "init" => {
                    ensure!(
                        !c.get("DICTIONARY_FILE").is_empty(),
                        "DICTIONARY_FILE is empty"
                    );
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    match std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                    {
                        Ok(mut f) => f.write_all(include_bytes!("../dictionary.example.toml"))?,
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                        Err(e) => return Err(e.into()),
                    }
                    println!("{}", path.display());
                }
                _ => {
                    let p = Processor::load(&c)?;
                    println!(
                        "{}: {} terms, {} corrections",
                        path.display(),
                        p.dictionary.terms.len(),
                        p.dictionary.replacements.len()
                    );
                }
            }
            Ok(())
        }
        Commands::Bench {
            source,
            factors,
            language,
        } => evaluation::bench(&c, &source, &factors, &language).await,
        Commands::Eval { action } => match action {
            Eval::Init { directory } => evaluation::init(&directory),
            Eval::Check { manifest } => {
                evaluation::run(&c, "check", &manifest, None, None, "").await
            }
            Eval::Run {
                manifest,
                language,
                model,
                output,
            } => {
                if !model.is_empty() {
                    c.set("TRANSCRIBE_MODEL", model);
                }
                evaluation::run(&c, "run", &manifest, None, output.as_deref(), &language).await
            }
            Eval::Score {
                manifest,
                transcripts,
                output,
            } => {
                evaluation::run(
                    &c,
                    "score",
                    &manifest,
                    Some(&transcripts),
                    output.as_deref(),
                    "",
                )
                .await
            }
        },
    }
}
