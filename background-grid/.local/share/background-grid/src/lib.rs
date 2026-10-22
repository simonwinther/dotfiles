pub mod catalog;
pub mod config;
pub mod setup;
pub mod thumbnail;
pub mod wallpaper;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
