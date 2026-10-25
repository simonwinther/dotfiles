use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ENDPOINT: &str = "https://api.openweathermap.org/data/2.5/weather";
const MAX_BYTES: u64 = 1024 * 1024;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub city: String,
    pub country: String,
    pub language: String,
    pub key_file: PathBuf,
    pub cache_seconds: u64,
    pub max_stale_seconds: u64,
    pub connect_timeout_seconds: u64,
    pub request_timeout_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            city: "Copenhagen".into(),
            country: "DK".into(),
            language: "en".into(),
            key_file: "~/.owm-key".into(),
            cache_seconds: 540,
            max_stale_seconds: 21_600,
            connect_timeout_seconds: 2,
            request_timeout_seconds: 5,
        }
    }
}

impl Config {
    pub fn read(path: &Path) -> Result<Self> {
        let config: Self = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error.into()),
        };
        if config.city.trim().is_empty()
            || config.country.trim().is_empty()
            || config.language.trim().is_empty()
            || !(1..=30).contains(&config.connect_timeout_seconds)
            || !(1..=30).contains(&config.request_timeout_seconds)
            || config.max_stale_seconds < config.cache_seconds
        {
            return Err("Invalid weather settings".into());
        }
        Ok(config)
    }

    fn identity(&self) -> String {
        serde_json::to_string(&(&self.city, &self.country, &self.language)).unwrap()
    }

    fn key_path(&self, home: &Path) -> PathBuf {
        self.key_file
            .strip_prefix("~")
            .map_or_else(|_| self.key_file.clone(), |relative| home.join(relative))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub id: u16,
    pub description: String,
    pub temperature: f64,
    pub wind_kmh: f64,
    pub sunrise: u64,
    pub sunset: u64,
}

impl Observation {
    fn valid(&self) -> bool {
        self.temperature.is_finite() && self.wind_kmh.is_finite() && self.wind_kmh >= 0.0
    }
}

#[derive(Deserialize)]
struct Response {
    weather: Vec<Condition>,
    main: Temperature,
    wind: Wind,
    sys: Sun,
}
#[derive(Deserialize)]
struct Condition {
    id: u16,
    description: String,
}
#[derive(Deserialize)]
struct Temperature {
    temp: f64,
}
#[derive(Deserialize)]
struct Wind {
    speed: f64,
}
#[derive(Deserialize)]
struct Sun {
    sunrise: u64,
    sunset: u64,
}

pub fn parse(bytes: &[u8]) -> Result<Observation> {
    let response: Response = serde_json::from_slice(bytes)?;
    let condition = response
        .weather
        .into_iter()
        .next()
        .ok_or("No weather conditions")?;
    let observation = Observation {
        id: condition.id,
        description: condition.description,
        temperature: response.main.temp,
        wind_kmh: response.wind.speed * 3.6,
        sunrise: response.sys.sunrise,
        sunset: response.sys.sunset,
    };
    if !observation.valid() {
        return Err("Invalid weather measurements".into());
    }
    Ok(observation)
}

fn fetch(config: &Config, key: &str, endpoint: &str) -> Result<Observation> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(config.connect_timeout_seconds))
        .timeout(Duration::from_secs(config.request_timeout_seconds))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("dotfiles-waybar-weather/1.0")
        .build()?;
    let response = client
        .get(endpoint)
        .query(&[
            ("appid", key),
            ("units", "metric"),
            ("lang", config.language.as_str()),
            ("q", &format!("{},{}", config.city, config.country)),
        ])
        .send()
        // Request errors may contain the URL, including appid. Never expose it.
        .map_err(|_| "Could not reach the weather service")?;
    if !response.status().is_success() {
        return Err(format!(
            "Weather service returned HTTP {}",
            response.status().as_u16()
        )
        .into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read the weather response")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Weather response is too large".into());
    }
    parse(&bytes).map_err(|_| "Invalid weather response".into())
}

#[derive(Serialize, Deserialize)]
struct Cache {
    identity: String,
    fetched_at: u64,
    observation: Observation,
}

fn read_cache(path: &Path, config: &Config, now: u64) -> Option<Cache> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES).read_to_end(&mut bytes).ok()?;
    let cache: Cache = serde_json::from_slice(&bytes).ok()?;
    (cache.identity == config.identity() && cache.fetched_at <= now && cache.observation.valid())
        .then_some(cache)
}

fn write_cache(directory: &Path, cache: &Cache) -> Result<()> {
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&serde_json::to_vec(cache)?)?;
    temporary.persist(directory.join("weather.json"))?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct Module {
    pub text: String,
    pub tooltip: String,
    pub class: Vec<&'static str>,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

impl Module {
    pub fn unavailable(reason: &str, light: bool) -> Self {
        let color = if light { "#d20f39" } else { "#f38ba8" };
        Self {
            text: format!("<span color='{color}'> </span>"),
            tooltip: escape(reason),
            class: vec!["weather", "unavailable"],
        }
    }

    fn weather(observation: &Observation, now: u64, stale: Option<u64>, light: bool) -> Self {
        let day = (observation.sunrise..=observation.sunset).contains(&now);
        let (icon, dark, bright) = match observation.id {
            200..=232 => (if day { "" } else { "" }, "#d3b987", "#df8e1d"),
            300..=311 => (if day { "" } else { "" }, "#73cef4", "#1e66f5"),
            312..=321 => (if day { "" } else { "" }, "#74c7ec", "#209fb5"),
            500..=531 => (if day { "" } else { "" }, "#74c7ec", "#209fb5"),
            600..=622 => ("", "#FFFFFF", "#4c4f69"),
            701..=771 => ("", "#7f849c", "#7c7f93"),
            781 => ("", "#d3b987", "#df8e1d"),
            800 if day => ("", "#f9e2af", "#df8e1d"),
            800 => ("", "#FFFFFF", "#4c4f69"),
            801 if day => ("", "#f9e2af", "#df8e1d"),
            801 => ("", "#FFFFFF", "#4c4f69"),
            802..=804 => ("", "#6c7086", "#6c6f85"),
            _ => ("", "#f38ba8", "#d20f39"),
        };
        let color = if light { bright } else { dark };
        let (white, muted, wind) = if light {
            ("#4c4f69", "#6c6f85", "#1e66f5")
        } else {
            ("#cdd6f4", "#585b70", "#73cef4")
        };
        let temperature = observation.temperature.round();
        let mut tooltip = format!(
            "Weather: {}\nTemp: {temperature:.0}°C\nWind: {:.1} km/h",
            escape(&observation.description),
            observation.wind_kmh
        );
        let mut class = vec!["weather"];
        if let Some(age) = stale {
            tooltip.push_str(&format!(
                "\nCached reading: {} minutes ago; refresh unavailable",
                age / 60
            ));
            class.push("stale");
        }
        Self {
            text: format!(
                "<span color='{color}'>{icon}</span>  <span color='{white}'>{temperature:.0}°</span> <span color='{muted}'> │ </span> <span color='{wind}'>󰖝</span> <span color='{white}'>{:.1}</span><span color='{muted}'>km/h</span>",
                observation.wind_kmh
            ),
            tooltip,
            class,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    config: &Config,
    home: &Path,
    cache_dir: &Path,
    now: u64,
    endpoint: &str,
    refresh: bool,
    light: bool,
) -> Module {
    let cached = read_cache(&cache_dir.join("weather.json"), config, now);
    if !refresh
        && let Some(cache) = &cached
        && now - cache.fetched_at < config.cache_seconds
    {
        return Module::weather(&cache.observation, now, None, light);
    }
    let result = fs::read_to_string(config.key_path(home))
        .map_err(|_| "API key missing: create ~/.owm-key".into())
        .and_then(|key| {
            let key = key.trim();
            if key.is_empty() {
                Err("Weather API key is empty".into())
            } else {
                fetch(config, key, endpoint)
            }
        });
    match result {
        Ok(observation) => {
            let cache = Cache {
                identity: config.identity(),
                fetched_at: now,
                observation,
            };
            if write_cache(cache_dir, &cache).is_err() {
                eprintln!("waybar-weather: could not save the weather cache");
            }
            Module::weather(&cache.observation, now, None, light)
        }
        Err(error) => {
            if let Some(cache) = cached
                && now - cache.fetched_at <= config.max_stale_seconds
            {
                return Module::weather(
                    &cache.observation,
                    now,
                    Some(now - cache.fetched_at),
                    light,
                );
            }
            Module::unavailable(&error.to_string(), light)
        }
    }
}
