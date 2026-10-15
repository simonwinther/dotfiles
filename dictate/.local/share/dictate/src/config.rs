use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const DEFAULTS: &[(&str, &str)] = &[
    (
        "OPENAI_REALTIME_URL",
        "wss://api.openai.com/v1/realtime?intent=transcription",
    ),
    (
        "OPENAI_API_URL",
        "https://api.openai.com/v1/audio/transcriptions",
    ),
    ("LIVE_MODEL", "gpt-live-transcribe"),
    ("LIVE_DELAY", "low"),
    ("LIVE_FINAL_TIMEOUT_S", "30"),
    ("TRANSCRIBE_MODEL", "gpt-transcribe"),
    ("OPENAI_LANGUAGES", "en,da"),
    ("OPENAI_LANGUAGE", ""),
    ("OPENAI_PROMPT", ""),
    ("OPENAI_KEYWORDS", ""),
    ("TEXT_CLEANUP", "1"),
    ("PUNCTUATION_MODE", "spoken"),
    ("RECORD_RATE", "16000"),
    ("RECORD_CHANNELS", "1"),
    ("RECORD_FORMAT", "s16"),
    ("RECORD_LATENCY", "20ms"),
    ("MIN_AUDIO_BYTES", "4096"),
    ("MIN_RECORDING_MS", "1000"),
    ("MAX_RECORDING_MS", "300000"),
    ("SKIP_SILENCE", "1"),
    ("SILENCE_MAX_DB", "-50"),
    ("KEEP_AUDIO", "0"),
    ("FFMPEG_PREPROCESS", "1"),
    ("TRIM_SILENCE", "1"),
    ("SILENCE_AUTO_THRESHOLD", "1"),
    ("SILENCE_THRESHOLD_DB", "-45"),
    ("SILENCE_SPEECH_MARGIN_DB", "12"),
    ("SILENCE_MIN_PAUSE", "0.45"),
    ("SILENCE_KEEP", "0.25"),
    ("SPEEDUP_FACTOR", "1.5"),
    ("PREPROCESS_MIN_MS", "3000"),
    ("UPLOAD_CODEC", "wav"),
    ("DEBUG_NOTIFY", "0"),
    ("AUTO_PASTE", "0"),
    ("PASTE_METHOD", "hyprctl-super-v"),
    ("PASTE_DELAY", "0"),
    ("SHOW_LOADING", "1"),
    ("TRANSCRIBE_OVERLAY_MIN_MS", "0"),
    ("DUCK_VOLUME", "1"),
    ("VOLUME_DUCK_FACTOR", "0.3"),
    ("VOLUME_DUCK_FADE_IN_MS", "700"),
    ("VOLUME_DUCK_FADE_OUT_MS", "900"),
    ("VOLUME_DUCK_FADE_STEPS", "12"),
    ("SHOW_COST_ESTIMATE", "1"),
    ("OPENAI_COST_USD_PER_MINUTE", "auto"),
    ("OPENAI_COST_CURRENCY", "DKK"),
    ("OPENAI_COST_USD_TO_DKK", "6.4104"),
    ("NOTIFY", "0"),
    ("HTTP_CONNECT_TIMEOUT_S", "10"),
    ("HTTP_MAX_TIME_S", "120"),
    ("LAST_TRANSCRIPT_FILE", "/tmp/dictate-last.txt"),
    ("LAST_LOG_FILE", "/tmp/dictate-last.log"),
];

#[derive(Clone, Serialize, Deserialize)]
pub struct Config(pub BTreeMap<String, String>);

impl Default for Config {
    fn default() -> Self {
        let mut c = Self(
            DEFAULTS
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        c.set(
            "DICTIONARY_FILE",
            config_dir().join("dictionary.toml").to_string_lossy(),
        );
        c
    }
}

pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
pub fn xdg(name: &str, fallback: &str) -> PathBuf {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(fallback))
}
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("dictate")
}
pub fn runtime_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/dictate-{}", unsafe { libc::getuid() })))
        .join("dictate")
}
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("dictate")
}
pub fn expand(s: &str) -> PathBuf {
    if s == "~" {
        home()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home().join(rest)
    } else {
        s.into()
    }
}

pub fn env_values(contents: &str) -> BTreeMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            // shell-words does not implement comments. Strip them only outside quotes.
            let mut quote = None;
            let mut escaped = false;
            let mut end = line.len();
            let mut previous_space = true;
            for (i, c) in line.char_indices() {
                if escaped {
                    escaped = false;
                    previous_space = false;
                    continue;
                }
                if c == '\\' && quote != Some('\'') {
                    escaped = true;
                    continue;
                }
                if Some(c) == quote {
                    quote = None;
                } else if quote.is_none() && matches!(c, '\'' | '"') {
                    quote = Some(c);
                } else if quote.is_none() && c == '#' && previous_space {
                    end = i;
                    break;
                }
                previous_space = c.is_whitespace();
            }
            let words = shell_words::split(&line[..end]).ok()?;
            let words = if words.first().map(String::as_str) == Some("export") {
                &words[1..]
            } else {
                &words[..]
            };
            if words.len() != 1 {
                return None;
            }
            let (key, value) = words[0].split_once('=')?;
            (!key.is_empty()).then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut c = Self::default();
        for (key, val) in env::vars() {
            if c.0.contains_key(&key) || key.starts_with("LIVE_") || key == "OPENAI_API_KEY" {
                c.0.insert(key, val);
            }
        }
        for name in ["config.env", "secrets.env"] {
            let path = config_dir().join(name);
            match fs::read_to_string(&path) {
                Ok(text) => c.0.extend(env_values(&text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e).with_context(|| format!("Cannot read {}", path.display())),
            }
        }
        Ok(c)
    }
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or("")
    }
    pub fn inherited<'a>(&'a self, key: &str, fallback: &str) -> &'a str {
        self.0
            .get(key)
            .map(String::as_str)
            .unwrap_or_else(|| self.get(fallback))
    }
    pub fn set(&mut self, key: &str, value: impl AsRef<str>) {
        self.0.insert(key.to_owned(), value.as_ref().to_owned());
    }
    pub fn yes(&self, key: &str) -> bool {
        matches!(
            self.get(key).to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }
    pub fn number(&self, key: &str) -> f64 {
        self.get(key).replace(',', ".").parse().unwrap_or(0.0)
    }
    pub fn millis(&self, key: &str) -> u64 {
        self.number(key).max(0.0) as u64
    }
    pub fn seconds(&self, key: &str) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.number(key).max(0.001))
    }
    pub fn path(&self, key: &str) -> PathBuf {
        expand(self.get(key))
    }
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        self.0
            .iter()
            .filter(|(k, _)| {
                DEFAULTS.iter().any(|(name, _)| *name == k.as_str())
                    || *k == "DICTIONARY_FILE"
                    || matches!(
                        k.as_str(),
                        "LIVE_LANGUAGES" | "LIVE_LANGUAGE" | "LIVE_KEYWORDS" | "LIVE_PROMPT"
                    )
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.get("PUNCTUATION_MODE"), "spoken" | "explicit" | "off"),
            "PUNCTUATION_MODE must be spoken, explicit, or off"
        );
        for (key, lo, hi) in [
            ("RECORD_RATE", 8000., 192000.),
            ("RECORD_CHANNELS", 1., 8.),
            ("SPEEDUP_FACTOR", 0.5, 4.),
            ("MIN_RECORDING_MS", 0., f64::MAX),
            ("MAX_RECORDING_MS", 0., f64::MAX),
            ("MIN_AUDIO_BYTES", 0., f64::MAX),
            ("SILENCE_MAX_DB", -120., 0.),
            ("SILENCE_THRESHOLD_DB", -120., 0.),
            ("SILENCE_SPEECH_MARGIN_DB", 0., 120.),
            ("SILENCE_MIN_PAUSE", 0., 3600.),
            ("SILENCE_KEEP", 0., 3600.),
            ("PREPROCESS_MIN_MS", 0., f64::MAX),
            ("PASTE_DELAY", 0., 10.),
            ("TRANSCRIBE_OVERLAY_MIN_MS", 0., 10000.),
            ("VOLUME_DUCK_FACTOR", 0., 1.),
            ("VOLUME_DUCK_FADE_IN_MS", 0., 10000.),
            ("VOLUME_DUCK_FADE_OUT_MS", 0., 10000.),
            ("VOLUME_DUCK_FADE_STEPS", 1., 100.),
            ("HTTP_CONNECT_TIMEOUT_S", 0.01, 86400.),
            ("HTTP_MAX_TIME_S", 0.01, 86400.),
            ("LIVE_FINAL_TIMEOUT_S", 0.01, 86400.),
            ("OPENAI_COST_USD_TO_DKK", 0., f64::MAX),
        ] {
            let v = self
                .get(key)
                .replace(',', ".")
                .parse::<f64>()
                .with_context(|| format!("Invalid {key}"))?;
            ensure!(
                v.is_finite() && v >= lo && v <= hi,
                "{key} must be between {lo} and {hi}"
            );
            if key.ends_with("_MS")
                || matches!(
                    key,
                    "RECORD_RATE"
                        | "RECORD_CHANNELS"
                        | "MIN_AUDIO_BYTES"
                        | "VOLUME_DUCK_FADE_STEPS"
                )
            {
                ensure!(v.fract() == 0., "{key} must be a whole number");
            }
        }
        for key in [
            "TEXT_CLEANUP",
            "SKIP_SILENCE",
            "KEEP_AUDIO",
            "FFMPEG_PREPROCESS",
            "TRIM_SILENCE",
            "SILENCE_AUTO_THRESHOLD",
            "AUTO_PASTE",
            "SHOW_LOADING",
            "DUCK_VOLUME",
            "SHOW_COST_ESTIMATE",
            "NOTIFY",
            "DEBUG_NOTIFY",
        ] {
            ensure!(
                matches!(
                    self.get(key).to_ascii_lowercase().as_str(),
                    "" | "0" | "1" | "true" | "false" | "yes" | "no" | "on" | "off"
                ),
                "{key} must be boolean"
            );
        }
        ensure!(
            self.millis("MAX_RECORDING_MS") == 0
                || self.millis("MAX_RECORDING_MS") >= self.millis("MIN_RECORDING_MS"),
            "MAX_RECORDING_MS must be zero or at least MIN_RECORDING_MS"
        );
        ensure!(
            matches!(
                self.get("RECORD_FORMAT"),
                "s16" | "s24" | "s32" | "u8" | "f32" | "f64"
            ),
            "Unsupported RECORD_FORMAT"
        );
        ensure!(
            matches!(self.get("UPLOAD_CODEC"), "wav" | "mp3"),
            "UPLOAD_CODEC must be wav or mp3"
        );
        for (key, schemes) in [
            ("OPENAI_API_URL", &["http", "https"][..]),
            ("OPENAI_REALTIME_URL", &["ws", "wss"][..]),
        ] {
            let url =
                reqwest::Url::parse(self.get(key)).with_context(|| format!("Invalid {key}"))?;
            ensure!(
                schemes.contains(&url.scheme())
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none(),
                "Invalid {key}"
            );
        }
        for key in ["LAST_TRANSCRIPT_FILE", "LAST_LOG_FILE"] {
            ensure!(self.path(key).is_absolute(), "{key} must be absolute");
        }
        Ok(())
    }
    pub fn key(&self) -> Result<&str> {
        let key = self.get("OPENAI_API_KEY");
        if key.is_empty() || key == "replace_me" {
            bail!(
                "OPENAI_API_KEY is not configured in {}/secrets.env",
                config_dir().display()
            );
        }
        Ok(key)
    }
    pub fn live_recovery(&self, language: &str) -> Result<BTreeMap<String, String>> {
        let mut result = self.snapshot();
        result.insert("TRANSCRIBE_MODEL".into(), "gpt-transcribe".into());
        result.insert(
            "OPENAI_LANGUAGES".into(),
            self.languages(true, language)?.join(","),
        );
        result.insert("OPENAI_LANGUAGE".into(), String::new());
        for (key, live) in [
            ("OPENAI_PROMPT", "LIVE_PROMPT"),
            ("OPENAI_KEYWORDS", "LIVE_KEYWORDS"),
        ] {
            result.insert(key.into(), self.inherited(live, key).into());
        }
        for (key, val) in [
            ("RECORD_RATE", "24000"),
            ("RECORD_CHANNELS", "1"),
            ("RECORD_FORMAT", "s16"),
            ("FFMPEG_PREPROCESS", "0"),
            ("TRIM_SILENCE", "0"),
            ("SPEEDUP_FACTOR", "1"),
            ("SKIP_SILENCE", "0"),
        ] {
            result.insert(key.into(), val.into());
        }
        Ok(result)
    }
    pub fn languages(&self, live: bool, override_: &str) -> Result<Vec<String>> {
        if !override_.is_empty() {
            validate_language(override_)?;
            return Ok(vec![override_.to_lowercase()]);
        }
        let csv = if live {
            self.inherited("LIVE_LANGUAGES", "OPENAI_LANGUAGES")
        } else {
            self.get("OPENAI_LANGUAGES")
        };
        let single = if live {
            self.inherited("LIVE_LANGUAGE", "OPENAI_LANGUAGE")
        } else {
            self.get("OPENAI_LANGUAGE")
        };
        ensure!(
            csv.trim().is_empty() || single.trim().is_empty(),
            "Plural and singular language hints cannot both be set"
        );
        let mut values = Vec::new();
        for v in if csv.trim().is_empty() { single } else { csv }
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
        {
            validate_language(&v)?;
            if !values.contains(&v) {
                values.push(v);
            }
        }
        Ok(values)
    }
}
pub fn validate_language(s: &str) -> Result<()> {
    let s = s.to_ascii_lowercase();
    ensure!(
        (matches!(s.len(), 2 | 3) && s.bytes().all(|c| c.is_ascii_lowercase()))
            || matches!(s.as_str(), "zh-cn" | "zh-tw" | "zh-hk"),
        "Invalid language code: {s}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_env_is_data() {
        let v = env_values(
            "export OPENAI_PROMPT='$(touch /tmp/no) # words' # comment\nAUTO_PASTE=1\nEMPTY=''\nBAD=two words",
        );
        assert_eq!(v["OPENAI_PROMPT"], "$(touch /tmp/no) # words");
        assert_eq!(v["EMPTY"], "");
        assert!(!v.contains_key("BAD"));
    }
}
