use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use waybar_weather::{Config, ENDPOINT, Module};

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let refresh = match args.as_slice() {
        [] => false,
        [flag] if flag == "--refresh" => true,
        [flag] if flag == "--help" || flag == "-h" => {
            println!(
                "Usage: waybar-weather [--refresh]\n\nPrint Waybar JSON using weather.json and ~/.owm-key.\n--refresh bypasses the fresh cache; network requests still have a deadline."
            );
            return;
        }
        _ => {
            eprintln!("Usage: waybar-weather [--refresh]");
            std::process::exit(2);
        }
    };
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let cache_home = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"));
    let state_home = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"));
    let light = state_home.join("desktop/theme/light.mode").is_file();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let output = match Config::read(&config_home.join("waybar/weather.json")) {
        Ok(config) => waybar_weather::run(
            &config,
            &home,
            &cache_home.join("waybar-weather"),
            now,
            ENDPOINT,
            refresh,
            light,
        ),
        Err(_) => Module::unavailable("Invalid configuration: check waybar/weather.json", light),
    };
    println!(
        "{}",
        serde_json::to_string(&output).expect("Weather output is valid JSON")
    );
}
