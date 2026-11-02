use desktop_weather::{Config, Observation};
#[derive(Clone, Debug, Default)]
pub struct Weather {
    pub observation: Option<Observation>,
    pub stale: bool,
    pub reason: String,
}
pub fn read() -> Weather {
    read_with_refresh(false)
}
pub fn read_with_refresh(refresh: bool) -> Weather {
    let config = match Config::read(&crate::theme::config().join("desktop/weather.json")) {
        Ok(config) => config,
        Err(_) => {
            return Weather {
                reason: "Invalid weather settings".into(),
                ..Default::default()
            };
        }
    };
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::theme::home().join(".cache"))
        .join("desktop-weather");
    match desktop_weather::observe(
        &config,
        &crate::theme::home(),
        &cache,
        super::clock::now(),
        desktop_weather::ENDPOINT,
        refresh,
    ) {
        Ok(reading) => Weather {
            observation: Some(reading.observation),
            stale: reading.stale_seconds.is_some(),
            reason: String::new(),
        },
        Err(error) => Weather {
            reason: error.to_string(),
            ..Default::default()
        },
    }
}
