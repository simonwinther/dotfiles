mod job;
mod output;

use anyhow::{Context, Result, bail, ensure};
use job::Cancellation;
use output::TemporaryOutput;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::Duration,
};

fn encoder(
    kind: &str,
    input: &Path,
    format: &str,
    resolution: &str,
    output: &Path,
) -> Result<Command> {
    let mut command;
    match kind {
        "picture" => {
            let resize = match resolution {
                "high" => "3160x>",
                "medium" => "2160x>",
                "low" => "1080x>",
                _ => bail!("Invalid picture resolution: {resolution}"),
            };
            ensure!(
                matches!(format, "jpg" | "png"),
                "Invalid picture format: {format}"
            );
            command = Command::new("magick");
            command.arg(input).args(["-resize", resize]);
            if format == "jpg" {
                command.args(["-quality", "85", "-strip"]);
            } else {
                command.args([
                    "-strip",
                    "-define",
                    "png:compression-filter=5",
                    "-define",
                    "png:compression-level=9",
                    "-define",
                    "png:compression-strategy=1",
                    "-define",
                    "png:exclude-chunk=all",
                ]);
            }
        }
        "video" => {
            let scale = match resolution {
                "4k" => "scale=-2:2160",
                "1080p" => "scale=-2:1080",
                "720p" => "scale=-2:720",
                _ => bail!("Invalid video resolution: {resolution}"),
            };
            ensure!(
                matches!(format, "mp4" | "gif"),
                "Invalid video format: {format}"
            );
            command = Command::new("ffmpeg");
            command.args(["-nostdin", "-n", "-i"]).arg(input);
            if format == "gif" {
                command.arg("-vf").arg(format!(
                    "fps=10,{scale}:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse"
                ));
            } else if resolution == "4k" {
                command.args([
                    "-vf", scale, "-c:v", "libx265", "-preset", "slow", "-crf", "24", "-c:a",
                    "aac", "-b:a", "192k",
                ]);
            } else {
                command.args([
                    "-vf", scale, "-c:v", "libx264", "-preset", "fast", "-crf", "23", "-c:a",
                    "copy",
                ]);
            }
        }
        _ => bail!("Invalid media type: {kind}"),
    }
    command.arg(output);
    Ok(command)
}

fn destination(input: &Path, format: &str, resolution: &str) -> Result<PathBuf> {
    let mut name = input
        .file_stem()
        .context("Input has no filename")?
        .to_os_string();
    name.push(format!("-{resolution}.{format}"));
    Ok(input.with_file_name(name))
}

fn notify(cancel: &Cancellation, title: &str, body: &str) {
    let result = cancel.run(
        Command::new("notify-send").arg(title).arg(body),
        None,
        Some(Duration::from_secs(5)),
    );
    if let Err(error) = result {
        eprintln!("Notification unavailable: {error}");
    }
}

fn transcode(cancel: &Cancellation, args: &[OsString]) -> Result<()> {
    ensure!(
        args.len() == 4,
        "Usage: desktop-transcode-native <picture|video> <input> <format> <resolution>"
    );
    let kind = args[0].to_str().context("Invalid media type")?;
    let format = args[2].to_str().context("Invalid format")?;
    let resolution = args[3].to_str().context("Invalid resolution")?;
    // Absolute input paths also prevent leading '-' names being read as options.
    let supplied = PathBuf::from(&args[1]);
    let input = fs::canonicalize(&supplied).context("Could not resolve input file")?;
    ensure!(input.is_file(), "Input is not a regular file");
    // Validate all choices before deriving a path from them or creating files.
    encoder(kind, &input, format, resolution, Path::new("unused"))?;
    // Keep the selected filename/location when input is a symlink or alias.
    let supplied_output = destination(&supplied, format, resolution)?;
    let parent = supplied_output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let output = fs::canonicalize(parent)?.join(
        supplied_output
            .file_name()
            .context("Output has no filename")?,
    );
    match fs::symlink_metadata(&output) {
        Ok(_) => bail!("Output already exists: {}", output.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error).context("Could not inspect output path"),
    }
    cancel.check()?;
    let temporary =
        TemporaryOutput::new(output.parent().context("Output has no directory")?, format)?;
    if kind == "video" {
        notify(
            cancel,
            " Transcoding video…",
            &format!(
                "{} to {format} ({resolution})",
                input.file_name().unwrap().to_string_lossy()
            ),
        );
    }
    cancel.run(
        &mut encoder(kind, &input, format, resolution, &temporary.path)?,
        None,
        None,
    )?;
    cancel.check()?;
    temporary.publish(&output, cancel)?;
    drop(temporary);

    let uri = format!("file://{}\n", output.display());
    cancel
        .run(
            Command::new("wl-copy").args(["--type", "text/uri-list"]),
            Some(uri.as_bytes()),
            Some(Duration::from_secs(5)),
        )
        .with_context(|| {
            format!(
                "Saved {}, but could not copy it to the clipboard",
                output.display()
            )
        })?;
    let icon = if kind == "video" { "" } else { "" };
    notify(
        cancel,
        &format!("{icon} Transcoded to {resolution} {format}"),
        "Saved and copied to clipboard.",
    );
    cancel.check()
}

fn main() -> ExitCode {
    let cancel = match Cancellation::install() {
        Ok(cancel) => cancel,
        Err(error) => {
            eprintln!("desktop-transcode: {error:#}");
            return ExitCode::FAILURE;
        }
    };
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match transcode(&cancel, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("desktop-transcode: {error:#}");
            let signal = cancel.signal();
            ExitCode::from(if signal == 0 { 1 } else { (128 + signal) as u8 })
        }
    }
}
