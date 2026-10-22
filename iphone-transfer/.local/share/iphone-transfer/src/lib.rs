mod api;
mod process;
mod receive;
mod send;

use anyhow::{Context, Result, bail};
use std::{env, ffi::OsString, path::PathBuf, process::Command};

pub fn home() -> Result<PathBuf> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .context("HOME is not set")
}

const HELP: &str = "Usage:\n  iphone-transfer receive [--once | --list] [--directory PATH] [--socket PATH]\n  iphone-transfer send [--check | -- FILE ...]\n  iphone-transfer open\n\nreceive-iphone-files receives into ~/From iPhone and sorts by file type.\nsend-to-iphone opens a file chooser when no files are provided.\nBuild source changes with iphone-transfer-build.\n";

pub fn run(args: &[OsString]) -> Result<()> {
    if args.is_empty()
        || (args.len() == 1 && (args[0] == "--help" || args[0] == "-h"))
        || (args.len() == 2 && (args[1] == "--help" || args[1] == "-h"))
    {
        print!("{HELP}");
        return Ok(());
    }
    match args[0].to_str() {
        Some("receive") => {
            let mut root = home()?.join("From iPhone");
            let mut socket = PathBuf::from("/run/tailscale/tailscaled.sock");
            let mut once = false;
            let mut list = false;
            let mut index = 1;
            while index < args.len() {
                match args[index].to_str() {
                    Some("--once") => once = true,
                    Some("--list") => list = true,
                    Some("--directory" | "--socket") if index + 1 < args.len() => {
                        let value = PathBuf::from(&args[index + 1]);
                        if args[index] == "--directory" {
                            root = value;
                        } else {
                            socket = value;
                        }
                        index += 1;
                    }
                    _ => bail!(
                        "Unknown or incomplete receive option: {:?}\n{HELP}",
                        args[index]
                    ),
                }
                index += 1;
            }
            let api = api::Api::new(&socket)?;
            if list {
                println!("{}", serde_json::to_string(&api.pending(false)?)?);
                return Ok(());
            }
            let receiver = receive::Receiver::new(root, api)?;
            if once {
                receiver.batch(false)
            } else {
                receiver.run()
            }
        }
        Some("send") => {
            let args = &args[1..];
            if send::is_check(args) {
                return send::check();
            }
            let args = if args.first().is_some_and(|s| s == "--") {
                &args[1..]
            } else {
                args
            };
            let result = send::send(args.to_vec());
            if let Err(error) = &result {
                send::error_dialog(&format!("{error:#}"));
            }
            result
        }
        Some("open") => {
            let status = Command::new("nautilus")
                .arg(home()?.join("From iPhone"))
                .status()
                .context("Could not open Files from iPhone")?;
            anyhow::ensure!(
                status.success(),
                "File manager could not open the received files"
            );
            Ok(())
        }
        _ => bail!("Unknown command\n{HELP}"),
    }
}
