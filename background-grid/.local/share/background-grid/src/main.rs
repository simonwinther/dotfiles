mod ui;

use background_grid::Result;
use background_grid::catalog::{catalog, next_image};
use background_grid::config::Paths;
use background_grid::wallpaper::set_wallpaper;
use std::env;
use std::path::Path;

const HELP: &str = "Wallpaper grid\n\nUsage: background-grid [--set IMAGE | --restore | --next | --list]\n\n  No arguments  Open the wallpaper picker\n  --set IMAGE   Apply an image with the configured transition\n  --restore     Restore the selected wallpaper without a transition\n  --next        Cycle to the next image in the library\n  --list        List library images without opening a window\n  -h, --help    Show this help";

fn run() -> Result<i32> {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    if matches!(arguments.as_slice(), [flag] if flag == "--help" || flag == "-h") {
        println!("{HELP}");
        return Ok(0);
    }
    let paths = Paths::discover()?;
    match arguments.as_slice() {
        [] => ui::launch(paths),
        [flag, rest @ ..] if flag == "--setup" => {
            if rest.iter().any(|value| value == "--help" || value == "-h") {
                println!("{}", background_grid::setup::HELP);
                return Ok(0);
            }
            if rest
                .iter()
                .any(|value| value != "--dry-run" && value != "--no-reload")
            {
                return Err(background_grid::setup::HELP.into());
            }
            background_grid::setup::run(
                &paths,
                rest.iter().any(|value| value == "--dry-run"),
                rest.iter().any(|value| value == "--no-reload"),
            )?;
            Ok(0)
        }
        [flag] if flag == "--set" => Err("Usage: background-grid --set IMAGE".into()),
        [flag, image] if flag == "--set" => {
            set_wallpaper(&paths, Path::new(image), None)?;
            Ok(0)
        }
        [flag] if flag == "--restore" => {
            set_wallpaper(&paths, &paths.current(), Some("none"))?;
            Ok(0)
        }
        [flag] if flag == "--next" || flag == "--list" => {
            let images = catalog(&paths.folders()?)?.all_images();
            if flag == "--list" {
                for image in images {
                    println!("{}", image.display());
                }
            } else {
                let current = paths.current().canonicalize().unwrap_or_default();
                if let Some(next) = next_image(&images, &current) {
                    set_wallpaper(&paths, &next, None)?;
                }
            }
            Ok(0)
        }
        _ => Err(format!("Invalid arguments.\n\n{HELP}").into()),
    }
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("background-grid: {error}");
            std::process::exit(1);
        }
    }
}
