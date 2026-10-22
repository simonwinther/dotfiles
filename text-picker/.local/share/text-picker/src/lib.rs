pub mod config;
pub mod handoff;
pub mod hyprland;
pub mod paste;
pub mod picker;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

pub fn notify(message: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=Text picker", "Text picker", message])
        .status();
}

pub fn shell_quote(value: &std::ffi::OsStr) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', "'\\''"))
}
