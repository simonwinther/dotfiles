use crate::{config::Config, storage};
use anyhow::{Result, bail, ensure};
use std::{
    collections::VecDeque,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub fn sample_bytes(format: &str) -> usize {
    match format {
        "u8" => 1,
        "s24" => 3,
        "s32" | "f32" => 4,
        "f64" => 8,
        _ => 2,
    }
}
pub fn decode(frame: &[u8], format: &str) -> f64 {
    match format {
        "u8" => (frame[0] as f64 - 128.) / 128.,
        "s24" => ((i32::from_le_bytes([0, frame[0], frame[1], frame[2]])) as f64) / 2147483648.,
        "s32" => i32::from_le_bytes(frame[..4].try_into().unwrap()) as f64 / 2147483648.,
        "f32" => f32::from_le_bytes(frame[..4].try_into().unwrap()) as f64,
        "f64" => f64::from_le_bytes(frame[..8].try_into().unwrap()),
        _ => i16::from_le_bytes(frame[..2].try_into().unwrap()) as f64 / 32768.,
    }
}
pub struct Levels {
    ring: VecDeque<f64>,
    capacity: usize,
    pub square_sum: f64,
    pub samples: u64,
}
impl Levels {
    pub fn new(rate: u32) -> Self {
        Self {
            ring: VecDeque::with_capacity(rate as usize * 120 / 1000),
            capacity: rate as usize * 120 / 1000,
            square_sum: 0.,
            samples: 0,
        }
    }
    pub fn append(&mut self, bytes: &[u8], format: &str, channels: usize) {
        let width = sample_bytes(format);
        for frame in bytes.chunks_exact(width * channels) {
            let value = frame
                .chunks_exact(width)
                .map(|s| decode(s, format))
                .sum::<f64>()
                / channels as f64;
            let value = if value.is_finite() {
                value.clamp(-1., 1.)
            } else {
                0.
            };
            self.square_sum += value * value;
            self.samples += 1;
            if self.ring.len() == self.capacity {
                self.ring.pop_front();
            }
            self.ring.push_back(value);
        }
    }
    pub fn bars(&self) -> Vec<f32> {
        let mut values = vec![0.; 42];
        let n = self.ring.len();
        if n == 0 {
            return values;
        }
        for (i, out) in values.iter_mut().enumerate() {
            let start = i * n / 42;
            let end = ((i + 1) * n / 42).max(start + 1).min(n);
            let square = (start..end)
                .map(|j| self.ring[j] * self.ring[j])
                .sum::<f64>();
            let rms = (square / (end - start) as f64).sqrt();
            *out = ((rms.max(1e-6).log10() * 20. + 60.) / 54.).clamp(0., 1.) as f32;
        }
        values
    }
    pub fn db(&self) -> f64 {
        if self.samples == 0 || self.square_sum == 0. {
            -120.
        } else {
            20. * (self.square_sum / self.samples as f64).sqrt().log10()
        }
    }
}

pub fn wav_header(bytes: u64, rate: u32, channels: u16, format: &str) -> Result<Vec<u8>> {
    ensure!(
        bytes <= u32::MAX as u64 - 36,
        "Recording exceeds WAV size limit"
    );
    let width = sample_bytes(format) as u16;
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(bytes as u32 + 36).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&(if format.starts_with('f') { 3u16 } else { 1u16 }).to_le_bytes());
    h.extend_from_slice(&channels.to_le_bytes());
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate * channels as u32 * width as u32).to_le_bytes());
    h.extend_from_slice(&(channels * width).to_le_bytes());
    h.extend_from_slice(&(width * 8).to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&(bytes as u32).to_le_bytes());
    Ok(h)
}
pub fn wrap_pcm(
    source: &Path,
    target: &Path,
    rate: u32,
    channels: u16,
    format: &str,
) -> Result<()> {
    let mut src = storage::read_file(source)?;
    let frame = sample_bytes(format) as u64 * channels as u64;
    let bytes = src.metadata()?.len() / frame * frame;
    ensure!(bytes > 0, "Recording has no complete samples");
    let mut dst = storage::private_file(target, false)?;
    dst.write_all(&wav_header(bytes, rate, channels, format)?)?;
    let copied = std::io::copy(&mut std::io::Read::by_ref(&mut src).take(bytes), &mut dst)?;
    ensure!(copied == bytes, "Truncated PCM recording");
    dst.sync_all()?;
    Ok(())
}
pub struct Wav {
    pub rate: u32,
    pub samples: Vec<i16>,
}
pub fn read_wav(path: &Path) -> Result<Wav> {
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    let mut riff = [0u8; 12];
    f.read_exact(&mut riff)?;
    ensure!(
        &riff[..4] == b"RIFF" && &riff[8..] == b"WAVE",
        "Not a WAV recording"
    );
    let mut format = None;
    let mut payload = None;
    while f.stream_position()? + 8 <= size {
        let mut header = [0u8; 8];
        f.read_exact(&mut header)?;
        let n = u32::from_le_bytes(header[4..].try_into().unwrap()) as u64;
        let offset = f.stream_position()?;
        if &header[..4] == b"fmt " {
            ensure!((16..=4096).contains(&n), "Invalid WAV format");
            let mut raw = vec![0u8; n as usize];
            f.read_exact(&mut raw)?;
            let mut encoding = u16::from_le_bytes(raw[..2].try_into().unwrap());
            if encoding == 0xfffe && raw.len() >= 40 {
                encoding = u16::from_le_bytes(raw[24..26].try_into().unwrap());
            }
            format = Some((
                encoding,
                u16::from_le_bytes(raw[2..4].try_into().unwrap()),
                u32::from_le_bytes(raw[4..8].try_into().unwrap()),
                u16::from_le_bytes(raw[14..16].try_into().unwrap()),
            ));
        } else if &header[..4] == b"data" {
            payload = Some((offset, n.min(size - offset)));
        }
        ensure!(
            offset + n + (n % 2) <= size || &header[..4] == b"data",
            "Truncated WAV chunk"
        );
        f.seek(SeekFrom::Start((offset + n + n % 2).min(size)))?;
        if format.is_some() && payload.is_some() {
            break;
        }
    }
    let (encoding, channels, rate, bits) =
        format.ok_or_else(|| anyhow::anyhow!("Missing WAV format"))?;
    let (offset, n) = payload.ok_or_else(|| anyhow::anyhow!("Missing WAV data"))?;
    ensure!(
        (1..=8).contains(&channels) && rate > 0 && n <= 512 * 1024 * 1024,
        "Invalid or oversized WAV"
    );
    let kind = match (encoding, bits) {
        (1, 8) => "u8",
        (1, 16) => "s16",
        (1, 24) => "s24",
        (1, 32) => "s32",
        (3, 32) => "f32",
        (3, 64) => "f64",
        _ => bail!("Unsupported WAV sample format"),
    };
    let width = sample_bytes(kind);
    f.seek(SeekFrom::Start(offset))?;
    let mut raw = vec![0u8; n as usize];
    f.read_exact(&mut raw)?;
    let samples = raw
        .chunks_exact(width * channels as usize)
        .map(|frame| {
            let v = frame
                .chunks_exact(width)
                .map(|s| decode(s, kind))
                .sum::<f64>()
                / channels as f64;
            (v.clamp(-1., 1.) * 32767.).round() as i16
        })
        .collect();
    Ok(Wav { rate, samples })
}
pub fn write_wav(path: &Path, w: &Wav) -> Result<()> {
    let mut f = storage::private_file(path, false)?;
    f.write_all(&wav_header(w.samples.len() as u64 * 2, w.rate, 1, "s16")?)?;
    let mut buffer = Vec::with_capacity(65536);
    for chunk in w.samples.chunks(32768) {
        buffer.clear();
        for x in chunk {
            buffer.extend_from_slice(&x.to_le_bytes());
        }
        f.write_all(&buffer)?;
    }
    Ok(())
}
pub fn rms_frames(samples: &[i16], rate: u32) -> Vec<f64> {
    samples
        .chunks((rate / 20).max(1) as usize)
        .map(|frame| {
            let square =
                frame.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / frame.len() as f64;
            if square == 0. {
                -120.
            } else {
                20. * (square.sqrt() / 32768.).log10()
            }
        })
        .collect()
}
fn runs(mask: &[bool]) -> Vec<(bool, usize, usize)> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < mask.len() {
        let mut end = start + 1;
        while end < mask.len() && mask[end] == mask[start] {
            end += 1;
        }
        result.push((mask[start], start, end));
        start = end;
    }
    result
}
pub fn trim(w: &Wav, c: &Config) -> Option<Wav> {
    let levels = rms_frames(&w.samples, w.rate);
    if levels.is_empty() {
        return None;
    }
    let mut threshold = c.number("SILENCE_THRESHOLD_DB");
    if c.yes("SILENCE_AUTO_THRESHOLD") {
        if levels.len() < 10 {
            return None;
        }
        let mut ordered = levels.clone();
        ordered.sort_by(f64::total_cmp);
        let floor = ordered[ordered.len() / 5];
        let speech = ordered[ordered.len() * 95 / 100];
        if speech - floor < 10. {
            return None;
        }
        threshold = (speech - c.number("SILENCE_SPEECH_MARGIN_DB"))
            .max(floor + 6.)
            .min(speech - 4.)
            .clamp(-60., -20.);
        if threshold - floor < 3. {
            return None;
        }
    }
    let mut mask: Vec<_> = levels.iter().map(|v| *v > threshold).collect();
    let groups = runs(&mask);
    for (i, &(speech, start, end)) in groups.iter().enumerate() {
        if !speech && i > 0 && i + 1 < groups.len() && end - start <= 6 {
            mask[start..end].fill(true);
        }
    }
    for (speech, start, end) in runs(&mask) {
        if speech && end - start < 3 {
            mask[start..end].fill(false);
        }
    }
    let frame = (w.rate / 20).max(1) as usize;
    let keep = (w.rate as f64 * c.number("SILENCE_KEEP")) as usize;
    let pause = (w.rate as f64 * c.number("SILENCE_MIN_PAUSE")) as usize;
    let mut segments: Vec<(usize, usize)> = Vec::new();
    for (speech, start, end) in runs(&mask) {
        if speech {
            let start = start * frame;
            let end = (end * frame).min(w.samples.len());
            if let Some(last) = segments.last_mut()
                && start.saturating_sub(last.1) < pause
            {
                last.1 = end;
                continue;
            }
            segments.push((start, end));
        }
    }
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (start, end) in segments {
        let start = start.saturating_sub(keep);
        let end = (end + keep).min(w.samples.len());
        if let Some(last) = ranges.last_mut()
            && start <= last.1
        {
            last.1 = end;
            continue;
        }
        ranges.push((start, end));
    }
    let kept: usize = ranges.iter().map(|(s, e)| e - s).sum();
    if kept * 1000 / (w.rate as usize) < 300
        || (w.samples.len() - kept) * 1000 / (w.rate as usize) < 250
    {
        return None;
    }
    let mut out = Vec::with_capacity(kept);
    for (start, end) in ranges {
        let mut segment = w.samples[start..end].to_vec();
        let fade = (w.rate as usize / 100).max(1).min(segment.len() / 2);
        for i in 0..fade {
            let tail = segment.len() - 1 - i;
            segment[i] = (segment[i] as i32 * i as i32 / fade as i32) as i16;
            segment[tail] = (segment[tail] as i32 * i as i32 / fade as i32) as i16;
        }
        out.extend(segment);
    }
    Some(Wav {
        rate: w.rate,
        samples: out,
    })
}

pub async fn preprocess(source: &Path, recording_ms: u64, c: &Config) -> Result<PathBuf> {
    if !c.yes("FFMPEG_PREPROCESS") || recording_ms < c.millis("PREPROCESS_MIN_MS") {
        return Ok(source.into());
    }
    let mut input = source.to_path_buf();
    if c.yes("TRIM_SILENCE")
        && let Ok(w) = read_wav(source)
        && let Some(trimmed) = trim(&w, c)
    {
        input = source.with_extension("trim.wav");
        write_wav(&input, &trimmed)?;
    }
    let speed = c.number("SPEEDUP_FACTOR");
    let codec = c.get("UPLOAD_CODEC");
    if (speed - 1.).abs() < 0.01 && input.extension().is_some_and(|ext| ext == codec) {
        return Ok(input);
    }
    if !crate::desktop::exists("ffmpeg") {
        return Ok(input);
    }
    let output = source.with_extension(format!("upload.{codec}"));
    let mut command = tokio::process::Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-i"])
        .arg(&input);
    if (speed - 1.).abs() >= 0.01 {
        command.args(["-af", &format!("atempo={speed}")]);
    }
    command.args(["-ar", c.get("RECORD_RATE"), "-ac", "1"]);
    if codec == "mp3" {
        command.args(["-c:a", "libmp3lame", "-b:a", "48k"]);
    } else {
        command.args(["-c:a", "pcm_s16le"]);
    }
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        command.arg(&output).kill_on_drop(true).output(),
    )
    .await;
    match result {
        Ok(Ok(o))
            if o.status.success() && std::fs::metadata(&output).is_ok_and(|m| m.len() >= 2048) =>
        {
            Ok(output)
        }
        _ => {
            let _ = std::fs::remove_file(&output);
            storage::log(c, "event=preprocess_failed fallback=original");
            Ok(input)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn levels_match_pcm() {
        let mut meter = Levels::new(24000);
        let bytes = [0, 64].repeat(2880);
        meter.append(&bytes, "s16", 1);
        assert!((meter.db() + 6.0206).abs() < 0.001);
        assert_eq!(meter.bars().len(), 42);
    }
    #[test]
    fn silence_is_finite() {
        let mut meter = Levels::new(24000);
        meter.append(&vec![0; 5760], "s16", 1);
        assert_eq!(meter.db(), -120.);
        assert!(meter.bars().iter().all(|&v| v == 0.));
    }
    #[test]
    fn header_size() {
        let h = wav_header(48000, 24000, 1, "s16").unwrap();
        assert_eq!(h.len(), 44);
        assert_eq!(&h[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 48000);
    }
}
