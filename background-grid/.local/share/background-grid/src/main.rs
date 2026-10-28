mod ui;
mod workers;

use background_grid::Result;
use background_grid::catalog::catalog;
use background_grid::config::Paths;
use background_grid::wallpaper::{
    current_wallpaper, next_wallpaper, restore_wallpapers, set_wallpaper,
};
use std::env;
use std::path::Path;

const HELP: &str = "Wallpaper grid\n\nUsage: background-grid [--set IMAGE [OUTPUT] | --restore | --next | --current | --list]\n\n  No arguments        Open the wallpaper picker\n  --set IMAGE         Apply an image to all outputs and clear output overrides\n  --set IMAGE OUTPUT  Override one output, keeping the default wallpaper\n  --restore           Restore the default and output overrides without a transition\n  --next              Cycle the library on all outputs and clear output overrides\n  --current           Print the default wallpaper\n  --list              List library images without opening a window\n  -h, --help          Show this help";

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
        [flag] if flag == "--set" => Err("Usage: background-grid --set IMAGE [OUTPUT]".into()),
        [flag, image] if flag == "--set" => {
            set_wallpaper(&paths, Path::new(image), None)?;
            Ok(0)
        }
        [flag, image, output] if flag == "--set" => {
            set_wallpaper(
                &paths,
                Path::new(image),
                Some(output.to_str().ok_or("Output name must be UTF-8")?),
            )?;
            Ok(0)
        }
        [flag] if flag == "--restore" => {
            restore_wallpapers(&paths)?;
            Ok(0)
        }
        [flag] if flag == "--next" => {
            next_wallpaper(&paths)?;
            Ok(0)
        }
        [flag] if flag == "--current" => {
            if let Some(image) = current_wallpaper(&paths)? {
                println!("{}", image.display());
            } else {
                println!();
            }
            Ok(0)
        }
        [flag] if flag == "--list" => {
            let images = catalog(&paths.folders()?)?.all_images();
            for image in images {
                println!("{}", image.display());
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
