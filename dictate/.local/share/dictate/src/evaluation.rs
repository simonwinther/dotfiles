use crate::{
    audio,
    config::{self, Config},
    network, storage,
    text::Processor,
};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Instant,
};

const NUMBER: &str = r"[+-]?(?:\d+(?:[.,]\d+)?|\.\d+)(?:[eE][+-]?\d+)?%?";
static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?:^|[^\w])(?P<number>{NUMBER})(?:$|[^\w])|(?P<word>\w+(?:[-'’./]\w+)*(?:\+\+|#)?)"
    ))
    .unwrap()
});
static NUMBERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("^{NUMBER}$")).unwrap());
pub fn tokens(text: &str) -> Vec<String> {
    let text = text.to_lowercase().replace('’', "'");
    let mut values = Vec::new();
    let mut offset = 0;
    while let Some(caps) = TOKENS.captures_at(&text, offset) {
        let word = caps.name("number").or_else(|| caps.name("word")).unwrap();
        values.push(word.as_str().to_owned());
        offset = word.end();
    }
    values
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suite {
    version: u32,
    #[serde(default = "default_name")]
    name: String,
    #[serde(default)]
    clips: Vec<Clip>,
}
fn default_name() -> String {
    "Scientific dictation".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clip {
    id: String,
    audio: PathBuf,
    expected: String,
    #[serde(default)]
    terms: Vec<String>,
    #[serde(default)]
    language: String,
}
fn term_count(term: &str, text: &str) -> usize {
    let re = Regex::new(&format!(
        "(?i){}",
        term.split_whitespace()
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join(r"[ \t]+")
    ))
    .unwrap();
    re.find_iter(text)
        .filter(|m| {
            let word = |c: char| c.is_alphanumeric() || c == '_';
            !text[..m.start()].chars().next_back().is_some_and(word)
                && !text[m.end()..].chars().next().is_some_and(word)
        })
        .count()
}
fn load(path: &Path, require_audio: bool) -> Result<Suite> {
    let mut suite: Suite =
        toml::from_str(&fs::read_to_string(path)?).context("Invalid evaluation suite")?;
    ensure!(
        suite.version == 1 && !suite.name.trim().is_empty(),
        "Suite version must be 1 and name must be nonempty"
    );
    let mut ids = BTreeSet::new();
    let valid_id = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")?;
    for clip in &mut suite.clips {
        ensure!(
            valid_id.is_match(&clip.id) && ids.insert(clip.id.clone()),
            "Clip ids must be unique and use letters, numbers, dots, dashes, or underscores"
        );
        ensure!(
            !tokens(&clip.expected).is_empty(),
            "{}: expected must contain reference words",
            clip.id
        );
        ensure!(
            !clip.audio.as_os_str().is_empty(),
            "{}: missing audio path",
            clip.id
        );
        clip.audio = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(config::expand(&clip.audio.to_string_lossy()));
        ensure!(
            matches!(
                clip.audio
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "wav" | "flac" | "mp3" | "mp4" | "mpeg" | "mpga" | "m4a" | "ogg" | "webm"
            ),
            "{}: unsupported audio format",
            clip.id
        );
        if require_audio {
            ensure!(
                clip.audio.is_file() && fs::metadata(&clip.audio)?.len() > 0,
                "{}: recording is missing or empty: {}",
                clip.id,
                clip.audio.display()
            );
            clip.audio = clip.audio.canonicalize()?;
        }
        let mut terms = BTreeSet::new();
        clip.terms
            .retain(|term| terms.insert(term.trim().to_lowercase()));
        for term in &clip.terms {
            ensure!(
                !term.trim().is_empty()
                    && !term.contains(['\r', '\n'])
                    && term_count(term, &clip.expected) > 0,
                "{}: scored terms must be nonempty and appear in expected",
                clip.id
            );
        }
        if !clip.language.is_empty() {
            config::validate_language(&clip.language)?;
        }
    }
    Ok(suite)
}
pub fn word_errors(expected: &str, transcript: &str) -> Value {
    let reference = tokens(expected);
    let hypothesis = tokens(transcript);
    let mut previous: Vec<(usize, usize, usize)> =
        (0..=hypothesis.len()).map(|n| (0, 0, n)).collect();
    for (i, word) in reference.iter().enumerate() {
        let mut current = vec![(0, i + 1, 0)];
        for (j, other) in hypothesis.iter().enumerate() {
            if word == other {
                current.push(previous[j]);
            } else {
                let s = previous[j];
                let d = previous[j + 1];
                let a = current[j];
                current.push(
                    [
                        (s.0 + 1, s.1, s.2),
                        (d.0, d.1 + 1, d.2),
                        (a.0, a.1, a.2 + 1),
                    ]
                    .into_iter()
                    .min_by_key(|x| x.0 + x.1 + x.2)
                    .unwrap(),
                );
            }
        }
        previous = current;
    }
    let (s, d, i) = previous[hypothesis.len()];
    let errors = s + d + i;
    json!({"reference_words":reference.len(),"substitutions":s,"deletions":d,"insertions":i,"errors":errors,"wer":if reference.is_empty(){if errors==0{Some(0.)}else{None}}else{Some(errors as f64/reference.len() as f64)}})
}
fn counts(words: impl IntoIterator<Item = String>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for word in words {
        *out.entry(word).or_default() += 1;
    }
    out
}
fn recall(expected: BTreeMap<String, usize>, actual: BTreeMap<String, usize>) -> Value {
    let total: usize = expected.values().sum();
    let matched: usize = expected
        .iter()
        .map(|(word, n)| (*n).min(*actual.get(word).unwrap_or(&0)))
        .sum();
    let missing: Vec<_> = expected
        .iter()
        .flat_map(|(word, n)| {
            std::iter::repeat_n(word, n.saturating_sub(*actual.get(word).unwrap_or(&0)))
        })
        .collect();
    json!({"expected":total,"matched":matched,"recall":if total>0{Some(matched as f64/total as f64)}else{None},"missing":missing})
}
fn score(clip: &Clip, transcript: &str) -> Value {
    let mut result = word_errors(&clip.expected, transcript);
    result["id"] = json!(clip.id);
    result["audio"] = json!(clip.audio);
    result["expected"] = json!(clip.expected);
    result["transcript"] = json!(transcript);
    result["terms"] = recall(
        clip.terms
            .iter()
            .map(|t| (t.clone(), term_count(t, &clip.expected)))
            .collect(),
        clip.terms
            .iter()
            .map(|t| (t.clone(), term_count(t, transcript)))
            .collect(),
    );
    result["numbers"] = recall(
        counts(
            tokens(&clip.expected)
                .into_iter()
                .filter(|w| NUMBERS.is_match(w)),
        ),
        counts(
            tokens(transcript)
                .into_iter()
                .filter(|w| NUMBERS.is_match(w)),
        ),
    );
    result
}
fn report(suite: &Suite, results: Vec<Value>, mode: &str, model: &str) -> Value {
    let mut summary = json!({"clips":results.len(),"failed_clips":results.iter().filter(|r|r["error"].as_str().is_some_and(|s|!s.is_empty())).count(),"empty_transcripts":results.iter().filter(|r|r["transcript"].as_str().unwrap_or("").trim().is_empty()).count()});
    for key in [
        "reference_words",
        "substitutions",
        "deletions",
        "insertions",
        "errors",
    ] {
        summary[key] = json!(
            results
                .iter()
                .map(|r| r[key].as_u64().unwrap_or(0))
                .sum::<u64>()
        );
    }
    let words = summary["reference_words"].as_u64().unwrap();
    summary["wer"] = json!(if words > 0 {
        Some(summary["errors"].as_u64().unwrap() as f64 / words as f64)
    } else {
        None
    });
    for key in ["terms", "numbers"] {
        let expected = results
            .iter()
            .map(|r| r[key]["expected"].as_u64().unwrap())
            .sum::<u64>();
        let matched = results
            .iter()
            .map(|r| r[key]["matched"].as_u64().unwrap())
            .sum::<u64>();
        summary[key] = json!({"expected":expected,"matched":matched,"recall":if expected>0{Some(matched as f64/expected as f64)}else{None}});
    }
    json!({"version":1,"suite":suite.name,"mode":mode,"model":model,"summary":summary,"clips":results})
}
pub fn init(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory.join("clips"))?;
    for (name, contents) in [
        (
            "suite.toml",
            "version = 1\nname = \"Scientific dictation\"\nclips = []\n\n# Add [[clips]] tables with id, audio, expected, terms, and language.\n",
        ),
        (".gitignore", "clips/\nresults/\ntranscripts.json\n"),
    ] {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(contents.as_bytes())?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
    }
    println!("{}", directory.join("suite.toml").display());
    Ok(())
}
pub async fn run(
    c: &Config,
    action: &str,
    path: &Path,
    transcripts: Option<&Path>,
    output: Option<&Path>,
    language: &str,
) -> Result<()> {
    let path = path.canonicalize()?;
    let suite = load(&path, action != "score")?;
    if action == "check" {
        println!("{}: {} clips validated", suite.name, suite.clips.len());
        return Ok(());
    }
    ensure!(
        !suite.clips.is_empty(),
        "No clips configured; add recordings and expected transcripts first"
    );
    if let Some(out) = output {
        let target = if out.exists() {
            out.canonicalize()?
        } else {
            let parent = out.parent().unwrap_or(Path::new("."));
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            fs::create_dir_all(parent)?;
            parent.canonicalize()?.join(out.file_name().unwrap())
        };
        ensure!(
            target != path
                && !suite.clips.iter().any(|clip| clip.audio == target)
                && transcripts.is_none_or(|p| p.canonicalize().ok() != Some(target.clone())),
            "Report output must differ from all input files"
        );
    }
    let results = if action == "score" {
        let mut data: Value =
            serde_json::from_slice(&fs::read(transcripts.context("Missing transcripts file")?)?)?;
        if let Some(clips) = data.get("clips").and_then(Value::as_array) {
            let mut mapping = BTreeMap::new();
            for clip in clips {
                let id = clip["id"].as_str().context("Missing clip id")?;
                ensure!(
                    mapping
                        .insert(id.to_owned(), clip["transcript"].clone())
                        .is_none(),
                    "Duplicate transcript id"
                );
            }
            data = json!(mapping);
        }
        let mapping = data
            .as_object()
            .context("Transcripts must map clip ids to text")?;
        ensure!(
            mapping.len() == suite.clips.len()
                && suite
                    .clips
                    .iter()
                    .all(|clip| mapping.get(&clip.id).is_some_and(Value::is_string)),
            "Transcript ids must exactly match the suite"
        );
        suite
            .clips
            .iter()
            .map(|clip| score(clip, mapping[&clip.id].as_str().unwrap()))
            .collect()
    } else {
        c.validate()?;
        c.key()?;
        let p = Processor::load(c)?;
        for clip in &suite.clips {
            network::batch_fields(
                c,
                &p,
                if language.is_empty() {
                    &clip.language
                } else {
                    language
                },
            )?;
        }
        let mut results = Vec::new();
        for clip in &suite.clips {
            let lang = if language.is_empty() {
                &clip.language
            } else {
                language
            };
            let started = Instant::now();
            let (text, error) = match network::batch(c, &p, lang, &clip.audio).await {
                Ok(t) => {
                    let t = p.process(&t);
                    let error = if t.is_empty() {
                        "Transcription was empty."
                    } else {
                        ""
                    };
                    (t, error.to_owned())
                }
                Err(e) => (String::new(), e.to_string()),
            };
            let mut result = score(clip, &text);
            result["error"] = json!(error);
            result["request_ms"] = json!(started.elapsed().as_millis() as u64);
            result["request_fields"] = json!(network::batch_fields(c, &p, lang)?);
            results.push(result);
        }
        results
    };
    let report = report(
        &suite,
        results,
        if action == "score" {
            "offline"
        } else {
            "batch"
        },
        if action == "score" {
            ""
        } else {
            c.get("TRANSCRIBE_MODEL")
        },
    );
    for clip in report["clips"].as_array().unwrap() {
        println!(
            "{}: WER {}, terms {}, numbers {}",
            clip["id"].as_str().unwrap(),
            percent(&clip["wer"]),
            percent(&clip["terms"]["recall"]),
            percent(&clip["numbers"]["recall"])
        );
    }
    println!(
        "Total: {} clips, WER {}, terms {}, numbers {}, {} failed",
        suite.clips.len(),
        percent(&report["summary"]["wer"]),
        percent(&report["summary"]["terms"]["recall"]),
        percent(&report["summary"]["numbers"]["recall"]),
        report["summary"]["failed_clips"]
    );
    if let Some(output) = output {
        storage::json(output, &report)?;
        println!("Report saved to {}", output.display());
    }
    ensure!(
        report["summary"]["failed_clips"] == 0,
        "Some evaluation requests failed"
    );
    Ok(())
}
fn percent(v: &Value) -> String {
    v.as_f64()
        .map(|v| format!("{:.1}%", v * 100.))
        .unwrap_or_else(|| "n/a".into())
}
pub async fn bench(c: &Config, source: &Path, factors: &[f64], language: &str) -> Result<()> {
    c.validate()?;
    c.key()?;
    let p = Processor::load(c)?;
    let source = source.canonicalize()?;
    network::batch_fields(c, &p, language)?;
    let mut variants = vec![("original".to_owned(), None), ("trim".to_owned(), Some(1.))];
    let factors = if factors.is_empty() {
        vec![1.5, 2.]
    } else {
        factors.to_vec()
    };
    for factor in factors {
        ensure!(
            factor.is_finite() && (0.5..=4.).contains(&factor),
            "Speedup factors must be between 0.5 and 4"
        );
        if (factor - 1.).abs() >= 0.01 && !variants.iter().any(|(_, v)| *v == Some(factor)) {
            variants.push((format!("trim+{factor}x"), Some(factor)));
        }
    }
    let root = config::runtime_dir()
        .join("bench")
        .join(storage::generation());
    storage::private_dir(&root)?;
    let result = async {
        let wav = match audio::read_wav(&source) {
            Ok(w) => w,
            Err(_) => {
                let converted = root.join("source.wav");
                let mut cmd = tokio::process::Command::new("ffmpeg");
                cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
                    .arg(&source)
                    .args(["-ar", c.get("RECORD_RATE"), "-ac", "1", "-c:a", "pcm_s16le"])
                    .arg(&converted)
                    .kill_on_drop(true);
                let result = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
                    .await??;
                ensure!(
                    result.status.success(),
                    "Cannot decode benchmark audio to WAV"
                );
                audio::read_wav(&converted)?
            }
        };
        let trimmed = audio::trim(&wav, c).unwrap_or(wav);
        let duration = trimmed.samples.len() as u64 * 1000 / trimmed.rate as u64;
        println!(
            "model={} source={}",
            c.get("TRANSCRIBE_MODEL"),
            source.display()
        );
        for (name, factor) in variants {
            let mut config = c.clone();
            let upload = if let Some(factor) = factor {
                config.set("SPEEDUP_FACTOR", factor.to_string());
                config.set("TRIM_SILENCE", "0");
                config.set("PREPROCESS_MIN_MS", "0");
                config.set("FFMPEG_PREPROCESS", "1");
                let copy = root.join(format!("{name}.wav"));
                audio::write_wav(&copy, &trimmed)?;
                audio::preprocess(&copy, duration, &config).await?
            } else {
                source.clone()
            };
            let started = Instant::now();
            match network::batch(&config, &p, language, &upload).await {
                Ok(text) => println!(
                    "{name} ({} ms): {}",
                    started.elapsed().as_millis(),
                    p.process(&text)
                ),
                Err(error) => println!(
                    "{name} ({} ms): error: {error}",
                    started.elapsed().as_millis()
                ),
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if !c.yes("KEEP_AUDIO") {
        let _ = fs::remove_dir_all(root);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scientific_tokens() {
        assert_eq!(
            tokens("nnU-Net C++ 1e-4 0.91 PyTorch"),
            ["nnu-net", "c++", "1e-4", "0.91", "pytorch"]
        );
    }
    #[test]
    fn weighted_word_errors() {
        let v = word_errors("one two three", "one new three four");
        assert_eq!(v["substitutions"], 1);
        assert_eq!(v["insertions"], 1);
        assert_eq!(v["reference_words"], 3);
    }
    #[test]
    fn repeated_terms_and_numbers() {
        let clip = Clip {
            id: "x".into(),
            audio: "x.wav".into(),
            expected: "PyTorch PyTorch 1e-4 1e-4".into(),
            terms: vec!["PyTorch".into()],
            language: String::new(),
        };
        let v = score(&clip, "PyTorch 1e-4");
        assert_eq!(v["terms"]["matched"], 1);
        assert_eq!(v["numbers"]["missing"], json!(["1e-4"]));
    }
}
