use crate::top::{
    App, Args,
    data::{self, Job},
    theme::{ACCENT, DIM, EDGE, FG, GREEN, RED, focus_style, selection_style},
};
use anyhow::{Context, Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    time::Duration,
};

pub fn fetch(args: &Args) -> Result<Vec<Job>> {
    let start = format!("now-{}days", args.history_days);
    let text = data::command(
        "sacct",
        &[
            "-u",
            &args.user,
            "-S",
            &start,
            "-X",
            "-nP",
            "--delimiter=\x1f",
            "--format=JobID,User,JobName,Partition,State,Submit,Elapsed,Timelimit,AllocTRES,NodeList,End,ExitCode,WorkDir,Start,Eligible,AllocCPUS,ReqMem,Account,QOS,ReqTRES,Reason",
        ],
        Duration::from_secs_f64(args.command_timeout),
    )?;
    parse(&text)
}
fn parse(text: &str) -> Result<Vec<Job>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let p: Vec<_> = line.split('\x1f').collect();
            if p.len() != 21 {
                bail!("Unrecognized sacct history response");
            }
            Ok(Job {
                id: p[0].into(),
                user: p[1].into(),
                name: p[2].into(),
                partition: p[3].into(),
                state: p[4].into(),
                submitted: p[5].into(),
                elapsed: p[6].into(),
                limit: p[7].into(),
                gres: p[8].into(),
                nodes: p[9].into(),
                end: p[10].into(),
                exit_code: p[11].into(),
                work_dir: p[12].into(),
                start: p[13].into(),
                eligible: p[14].into(),
                cpus: p[15].into(),
                req_mem: p[16].into(),
                account: p[17].into(),
                qos: p[18].into(),
                req_tres: p[19].into(),
                reason: p[20].into(),
                ..Default::default()
            })
        })
        .collect()
}
// scontrol's single-line output has space-delimited Key= markers, but values
// (including paths) can themselves contain spaces.
fn field<'a>(record: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!(" {key}=");
    let start = record.find(&marker)? + marker.len();
    let value = &record[start..];
    for (i, _) in value.match_indices(' ') {
        let next = &value[i + 1..];
        let token = next.split_whitespace().next().unwrap_or("");
        if let Some((key, _)) = token.split_once('=')
            && !key.is_empty()
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Some(&value[..i]);
        }
    }
    Some(value.trim_end())
}
fn expand(template: &str, job: &Job, record: &str) -> Result<PathBuf> {
    let mut chars = template.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut width = String::new();
        while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
            width.push(chars.next().unwrap());
        }
        let width = width.parse::<usize>().unwrap_or(0).min(10);
        let value = match chars.next() {
            Some('%') => "%".to_string(),
            Some('j') => field(record, "JobId").unwrap_or(&job.id).to_string(),
            Some('x') => job.name.clone(),
            Some('u') => job.user.clone(),
            Some('A') => field(record, "ArrayJobId")
                .or_else(|| job.id.split_once('_').map(|p| p.0))
                .unwrap_or(&job.id)
                .to_string(),
            Some('a') => field(record, "ArrayTaskId")
                .or_else(|| job.id.split_once('_').map(|p| p.1))
                .context("Array task ID is unavailable")?
                .to_string(),
            Some(c) => bail!("Cannot resolve %{c} in log path: {template}"),
            None => bail!("Incomplete log path: {template}"),
        };
        if value.chars().all(|c| c.is_ascii_digit()) {
            out.push_str(&format!("{value:0>width$}"));
        } else {
            out.push_str(&value);
        }
    }
    let path = PathBuf::from(out);
    if path.is_absolute() {
        Ok(path)
    } else {
        let dir = field(record, "WorkDir").unwrap_or(&job.work_dir);
        if !PathBuf::from(dir).is_absolute() {
            bail!("Slurm did not retain an absolute working directory");
        }
        Ok(PathBuf::from(dir).join(path))
    }
}
// Search filenames, not file contents. Include ignored logs without following
// symlinks; the command timeout bounds traversal of large working directories.
fn discover_logs(job: &Job, timeout: Duration) -> Result<Vec<PathBuf>> {
    let root = PathBuf::from(&job.work_dir);
    if !root.is_absolute() {
        bail!("No absolute working directory available for log search");
    }
    if job.id.is_empty() || !job.id.chars().all(|c| c.is_ascii_digit() || c == '_') {
        bail!("Cannot search logs for an unexpanded job ID: {}", job.id);
    }
    let pattern = format!("*{}*", job.id);
    let output = match data::command(
        "rg",
        &[
            "--files",
            "--null",
            "--hidden",
            "--no-ignore",
            "-g",
            &pattern,
            "-g",
            "!.git/**",
            "--",
            &job.work_dir,
        ],
        timeout,
    ) {
        Ok(text) => text,
        // rg exits 1 (with no diagnostic) when the filename glob has no match.
        Err(e) if e.to_string().trim() == "rg: exit status: 1" => return Ok(Vec::new()),
        Err(e) => return Err(e.context("Recursive log search failed")),
    };
    let mut found: Vec<_> = output
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|path| {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            ["out", "err", "log"].contains(&ext) && filename_matches(name, &job.id)
        })
        .collect();
    found.sort();
    found.dedup();
    found.truncate(10);
    Ok(found)
}
fn filename_matches(name: &str, id: &str) -> bool {
    !id.is_empty()
        && name.match_indices(id).any(|(i, _)| {
            let before = name[..i].chars().next_back();
            let after = name[i + id.len()..].chars().next();
            before.is_none_or(|c| !c.is_ascii_alphanumeric())
                && after.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
        })
}
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct LogView {
    pub title: String,
    pub lines: Vec<String>,
    pub offset: usize,
    pub horizontal: u16,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub paths: Vec<PathBuf>,
    pub status: String,
    pub copying: bool,
    pub editing_search: bool,
    pub search: String,
    pub(crate) matches: Vec<usize>,
    pub(crate) search_restore: Option<(String, usize, usize, u16)>,
}
impl LogView {
    pub fn loading(id: &str) -> Self {
        Self {
            title: format!("Job {id}"),
            lines: vec!["Locating and reading logs…".into()],
            ..Default::default()
        }
    }
    pub fn selection(&self) -> Option<std::ops::RangeInclusive<usize>> {
        self.anchor.map(|a| a.min(self.cursor)..=a.max(self.cursor))
    }
    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|range| {
            let mut text = String::new();
            for line in range.filter_map(|i| self.lines.get(i)) {
                text.push_str(line);
                text.push('\n');
            }
            text
        })
    }
    pub fn restore_position(&mut self, previous: &Self) {
        let max = self.lines.len().saturating_sub(1);
        self.cursor = previous.cursor.min(max);
        self.offset = previous.offset.min(self.cursor);
        self.horizontal = previous.horizontal;
        self.anchor = previous.anchor.map(|i| i.min(max));
        self.copying = previous.copying;
        self.editing_search = previous.editing_search;
        self.search_restore =
            previous
                .search_restore
                .as_ref()
                .map(|(search, cursor, offset, horizontal)| {
                    let cursor = (*cursor).min(max);
                    (search.clone(), cursor, (*offset).min(cursor), *horizontal)
                });
        self.search.clone_from(&previous.search);
        self.update_matches();
    }
    fn update_matches(&mut self) {
        self.matches.clear();
        if !self.search.is_empty() {
            self.matches.extend(
                self.lines
                    .iter()
                    .enumerate()
                    .filter_map(|(i, line)| line.contains(&self.search).then_some(i)),
            );
        }
    }
    fn next_match(&mut self, reverse: bool) {
        if self.matches.is_empty() {
            return;
        }
        self.cursor = if reverse {
            self.matches
                .iter()
                .rev()
                .find(|&&i| i < self.cursor)
                .or_else(|| self.matches.last())
        } else {
            self.matches
                .iter()
                .find(|&&i| i > self.cursor)
                .or_else(|| self.matches.first())
        }
        .copied()
        .unwrap_or(self.cursor);
        self.horizontal = 0;
    }
    pub fn key(&mut self, key: KeyEvent, page: usize) {
        let page = page.max(1);
        let max = self.lines.len().saturating_sub(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.editing_search {
            match key.code {
                KeyCode::Enter => {
                    self.editing_search = false;
                    self.search_restore = None;
                }
                KeyCode::Esc => {
                    if let Some((search, cursor, offset, horizontal)) = self.search_restore.take() {
                        self.search = search;
                        self.cursor = cursor.min(max);
                        self.offset = offset;
                        self.horizontal = horizontal;
                        self.update_matches();
                    }
                    self.editing_search = false;
                }
                KeyCode::Backspace => {
                    self.search.pop();
                    self.update_matches();
                }
                KeyCode::Char('u') if ctrl => {
                    self.search.clear();
                    self.update_matches();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.search.len() < 512 =>
                {
                    self.search.push(c);
                    self.update_matches();
                }
                _ => {}
            }
            if self.editing_search {
                let origin = self.search_restore.as_ref().map_or(self.cursor, |s| s.1);
                self.cursor = self
                    .matches
                    .iter()
                    .find(|&&i| i >= origin)
                    .or_else(|| self.matches.first())
                    .copied()
                    .unwrap_or(origin);
                self.horizontal = 0;
            }
            self.ensure_visible(page);
            return;
        }
        if !self.copying {
            self.status.clear();
        }
        match key.code {
            KeyCode::Char('d') if ctrl => {
                self.cursor = self.cursor.saturating_add((page / 2).max(1)).min(max);
                self.offset = self.offset.saturating_add((page / 2).max(1));
            }
            KeyCode::Char('u') if ctrl => {
                self.cursor = self.cursor.saturating_sub((page / 2).max(1));
                self.offset = self.offset.saturating_sub((page / 2).max(1));
            }
            KeyCode::Char('f') if ctrl => {
                self.cursor = self.cursor.saturating_add(page).min(max);
                self.offset = self.offset.saturating_add(page);
            }
            KeyCode::Char('b') if ctrl => {
                self.cursor = self.cursor.saturating_sub(page);
                self.offset = self.offset.saturating_sub(page);
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = self.cursor.saturating_add(1).min(max)
            }
            KeyCode::Char('k') | KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(' ') => {
                self.cursor = self.cursor.saturating_add(page).min(max);
                self.offset = self.offset.saturating_add(page);
            }
            KeyCode::PageUp | KeyCode::Backspace => {
                self.cursor = self.cursor.saturating_sub(page);
                self.offset = self.offset.saturating_sub(page);
            }
            KeyCode::Char('g') | KeyCode::Home => self.cursor = 0,
            KeyCode::Char('G') | KeyCode::End => self.cursor = max,
            KeyCode::Char('h') | KeyCode::Left => {
                self.horizontal = self.horizontal.saturating_sub(4)
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.horizontal = self.horizontal.saturating_add(4)
            }
            KeyCode::Char('v' | 'V') => {
                self.anchor = if self.anchor.is_some() {
                    None
                } else {
                    Some(self.cursor)
                }
            }
            KeyCode::Esc => self.anchor = None,
            KeyCode::Char('/') => {
                self.search_restore = Some((
                    std::mem::take(&mut self.search),
                    self.cursor,
                    self.offset,
                    self.horizontal,
                ));
                self.matches.clear();
                self.editing_search = true;
            }
            KeyCode::Char('n') => self.next_match(false),
            KeyCode::Char('N') => self.next_match(true),
            _ => {}
        }
        self.ensure_visible(page);
    }
    fn ensure_visible(&mut self, page: usize) {
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + page {
            self.offset = self.cursor + 1 - page;
        }
        self.offset = self.offset.min(self.lines.len().saturating_sub(page));
    }
}
// Read the original files again: the display is only a bounded, sanitized tail.
// Fail the whole copy if a file cannot be read, rather than silently omit it.
pub fn full_log_text(paths: &[PathBuf]) -> Result<String> {
    if paths.is_empty() {
        bail!("No log files available to yank yet");
    }
    const MAX_BYTES: usize = 64 * 1024 * 1024;
    let mut text = String::new();
    for path in paths {
        if !std::fs::metadata(path)?.is_file() {
            bail!("Not a regular log file: {}", path.display());
        }
        if paths.len() > 1 {
            text.push_str(&format!("── {} ──\n", path.display()));
        }
        let remaining = MAX_BYTES.saturating_sub(text.len());
        let file = File::open(path).with_context(|| format!("Cannot yank {}", path.display()))?;
        if file.metadata()?.len() > remaining as u64 {
            bail!(
                "Entire log exceeds the 64 MiB clipboard limit; select the relevant lines with v"
            );
        }
        // Bound the read as well as checking metadata: a running job may append
        // to the file while it is being copied.
        file.take(remaining as u64 + 1)
            .read_to_string(&mut text)
            .with_context(|| format!("Cannot yank {}", path.display()))?;
        if text.len() > MAX_BYTES {
            bail!(
                "Entire log exceeds the 64 MiB clipboard limit; select the relevant lines with v"
            );
        }
        if paths.len() > 1 && !text.ends_with('\n') {
            text.push('\n');
        }
    }
    Ok(text)
}

// Strip whole escape sequences, not just the ESC byte: colored logs must not
// leave strings such as "[31m" in the viewer or interpret OSC hyperlinks.
fn sanitize_log(text: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        Intermediate,
        Csi,
        String,
        StringEscape,
    }
    let mut state = State::Text;
    let mut clean = String::with_capacity(text.len());
    for c in text.chars() {
        state = match state {
            State::Text => match c {
                '\u{1b}' => State::Escape,
                '\u{9b}' => State::Csi,
                '\u{90}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => State::String,
                c => {
                    if !c.is_control() || c == '\n' || c == '\t' {
                        clean.push(c);
                    }
                    State::Text
                }
            },
            State::Escape => match c {
                '[' => State::Csi,
                ']' | 'P' | '^' | '_' => State::String,
                '\u{20}'..='\u{2f}' => State::Intermediate,
                '\u{1b}' => State::Escape,
                _ => State::Text,
            },
            State::Intermediate => {
                if ('\u{30}'..='\u{7e}').contains(&c) {
                    State::Text
                } else {
                    State::Intermediate
                }
            }
            State::Csi => {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    State::Text
                } else if c == '\u{1b}' {
                    State::Escape
                } else {
                    State::Csi
                }
            }
            State::String => match c {
                '\u{7}' | '\u{9c}' => State::Text,
                '\u{1b}' => State::StringEscape,
                _ => State::String,
            },
            State::StringEscape => match c {
                '\\' | '\u{7}' | '\u{9c}' => State::Text,
                '\u{1b}' => State::StringEscape,
                _ => State::String,
            },
        };
    }
    clean
}

const DISPLAY_FILE_BYTES: u64 = 2 * 1024 * 1024;
const DISPLAY_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
const DISPLAY_LINES: usize = 50_000;

fn append_logs(view: &mut LogView) {
    let mut remaining_bytes = DISPLAY_TOTAL_BYTES;
    let mut remaining_lines = DISPLAY_LINES;
    for path in &view.paths {
        view.lines.push(format!("── {} ──", path.display()));
        if remaining_bytes == 0 || remaining_lines == 0 {
            view.lines
                .push("[Display limit reached; y still yanks the entire log]".into());
            continue;
        }
        let result = (|| -> Result<(String, bool)> {
            if !std::fs::metadata(path)?.is_file() {
                bail!("Not a regular log file");
            }
            let mut file = File::open(path)?;
            let limit = DISPLAY_FILE_BYTES.min(remaining_bytes);
            let start = file.metadata()?.len().saturating_sub(limit);
            file.seek(SeekFrom::Start(start))?;
            let mut bytes = Vec::new();
            file.take(limit).read_to_end(&mut bytes)?;
            remaining_bytes = remaining_bytes.saturating_sub(bytes.len() as u64);
            // Drop a partial first line when tailing. Besides avoiding broken
            // UTF-8, this prevents starting in the middle of an ANSI sequence.
            let bytes = if start > 0 {
                bytes
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(bytes.as_slice(), |i| &bytes[i + 1..])
            } else {
                bytes.as_slice()
            };
            Ok((sanitize_log(&String::from_utf8_lossy(bytes)), start > 0))
        })();
        match result {
            Ok((text, truncated)) => {
                if truncated {
                    view.lines.push("[Showing a bounded tail; y yanks the entire log]".into());
                }
                if text.is_empty() {
                    view.lines.push("[Log is empty]".into());
                } else {
                    // Limit line count as well as bytes: millions of blank
                    // lines otherwise consume far more memory than the file.
                    let count = text.lines().count();
                    if count > remaining_lines {
                        view.lines.push(format!("[Showing last {remaining_lines} lines]"));
                    }
                    let skip = count.saturating_sub(remaining_lines);
                    let added = count.min(remaining_lines);
                    view.lines.extend(text.lines().skip(skip).map(str::to_owned));
                    remaining_lines -= added;
                }
            }
            Err(e) => view.lines.push(format!("Cannot read log: {e:#}. It may be deleted, inaccessible, or stored on a compute node.")),
        }
        view.lines.push(String::new());
    }
}

pub fn reload(paths: &[PathBuf], title: &str) -> LogView {
    let mut view = LogView {
        title: title.into(),
        paths: paths.to_vec(),
        ..Default::default()
    };
    append_logs(&mut view);
    if paths.is_empty() {
        view.lines
            .push("No log paths are available. Reopen the job to search again.".into());
    }
    view
}

pub fn read_log(job: &Job, timeout: Duration) -> LogView {
    let mut view = LogView {
        title: format!("Job {} · {}", job.id, job.name),
        ..Default::default()
    };
    let record =
        data::command("scontrol", &["show", "job", &job.id, "-o"], timeout).unwrap_or_default();
    // Prefix ensures the first key can be parsed by field().
    let record = format!(" {record}");
    let mut paths = Vec::new();
    for key in ["StdOut", "StdErr"] {
        if let Some(path) = field(&record, key).filter(|p| !p.is_empty() && *p != "(null)") {
            match expand(path, job, &record) {
                Ok(path) => {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
                Err(e) => view.lines.push(format!("{key}: {e:#}")),
            }
        }
    }
    let had_recorded_paths = !paths.is_empty();
    if paths.is_empty() && view.lines.is_empty() {
        view.lines.push(
            "Slurm no longer has the log paths. Trying the standard filename in WorkDir.".into(),
        );
        let template = if job.id.contains('_') {
            "slurm-%A_%a.out"
        } else {
            "slurm-%j.out"
        };
        match expand(template, job, &record) {
            Ok(path) => {
                if path.is_file() {
                    paths.push(path);
                } else {
                    match discover_logs(job, timeout) {
                        Ok(found) => paths = found,
                        Err(e) => view.lines.push(format!("{e:#}")),
                    }
                    if paths.is_empty() {
                        paths.push(path);
                    } else {
                        view.lines.push("Inferred paths: matching job ID in files below WorkDir (up to 10 files).".into());
                    }
                }
            }
            Err(e) => view.lines.push(format!("{e:#}")),
        }
        view.lines.push(
            "Custom paths are not stored in accounting; filename matches below are best-effort."
                .into(),
        );
    }
    if had_recorded_paths && !paths.iter().any(|p| p.is_file()) {
        match discover_logs(job, timeout) {
            Ok(found) if !found.is_empty() => {
                paths = found;
                view.lines.push("Recorded paths are unavailable. Inferred paths below match the job ID in WorkDir.".into());
            }
            Err(e) => view.lines.push(format!("{e:#}")),
            _ => {}
        }
    }
    view.paths = paths;
    append_logs(&mut view);
    view
}
pub fn draw_log(f: &mut Frame, area: Rect, log: &LogView) {
    let match_label = if log.search.is_empty() {
        String::new()
    } else {
        let current = log.matches.iter().position(|&i| i == log.cursor);
        format!(
            " · match {}/{}",
            current.map_or(0, |i| i + 1),
            log.matches.len()
        )
    };
    let title = format!(
        " {} · line {}/{}{}{} ",
        log.title,
        (log.cursor + 1).min(log.lines.len()),
        log.lines.len(),
        if log.anchor.is_some() {
            " · VISUAL LINE"
        } else {
            ""
        },
        match_label,
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(EDGE))
        .style(Style::default().fg(FG))
        .title(title)
        .title_bottom(" / search · y yank · v select · r reload · ? help · q back ");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let height = inner.height.saturating_sub(1);
    let range = log.selection();
    let lines: Vec<_> = log
        .lines
        .iter()
        .enumerate()
        .skip(log.offset)
        .take(height as usize)
        .map(|(i, text)| {
            let selected = range.as_ref().is_some_and(|r| r.contains(&i));
            let style = if selected {
                selection_style()
            } else if i == log.cursor {
                focus_style()
            } else {
                Style::default().fg(FG)
            };
            if log.search.is_empty() || selected {
                return Line::styled(text.as_str(), style);
            }
            let mut spans = Vec::new();
            let mut start = 0;
            for (index, matched) in text.match_indices(&log.search) {
                spans.push(Span::raw(&text[start..index]));
                spans.push(Span::styled(
                    matched,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ));
                start = index + matched.len();
            }
            spans.push(Span::raw(&text[start..]));
            Line::from(spans).style(style)
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).scroll((0, log.horizontal)),
        Rect { height, ..inner },
    );
    if inner.height > 0 {
        let status = if log.editing_search {
            format!(
                "/{}▏  · Enter keep · Esc cancel · {} matching lines",
                log.search,
                log.matches.len()
            )
        } else if !log.status.is_empty() {
            log.status.clone()
        } else if log.anchor.is_some() {
            "VISUAL LINE: j/k extend · y yank selection · Esc cancel".into()
        } else if !log.search.is_empty() {
            format!(
                "/{} · {} matching lines · n/N next/previous · y entire file",
                log.search,
                log.matches.len()
            )
        } else {
            "j/k move · Ctrl-d/u half-page · g/G top/end · h/l sideways · y entire file".into()
        };
        f.render_widget(
            Paragraph::new(status).style(Style::default().fg(if log.editing_search {
                ACCENT
            } else {
                DIM
            })),
            Rect::new(inner.x, inner.y + height, inner.width, 1),
        );
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let title = format!(
        " Last {} days · newest first{} · Enter logs · r refresh ",
        app.args.history_days,
        if app.history_loading {
            " · refreshing…"
        } else {
            ""
        },
    );
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(EDGE))
        .title(title);
    let mut inner = block.inner(area);
    f.render_widget(block, area);
    if let Some(error) = &app.history_error {
        let message = if app.history.is_empty() {
            format!("History unavailable: {error}. Press r to retry.")
        } else {
            format!("Refresh failed; keeping previous jobs: {error}. Press r to retry.")
        };
        f.render_widget(
            Paragraph::new(message).style(Style::default().fg(RED)),
            Rect {
                height: inner.height.min(1),
                ..inner
            },
        );
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);
        if app.history.is_empty() {
            return;
        }
    }
    if app.job_view.is_empty() {
        let message = if app.history_loading {
            "Loading history…"
        } else {
            "No recent jobs match this user, partition and filters."
        };
        f.render_widget(
            Paragraph::new(message).style(Style::default().fg(DIM)),
            inner,
        );
        return;
    }
    let (start, capacity, mut local_state) =
        crate::top::ui::viewport(&mut app.selected, app.job_view.len(), inner.height);
    let rows = app.job_view.iter().skip(start).take(capacity).map(|&i| {
        let j = &app.history[i];
        let color = if j.state.starts_with("COMPLETED") {
            GREEN
        } else if j.state.starts_with("FAILED")
            || j.state.starts_with("TIMEOUT")
            || j.state.starts_with("OUT_OF_MEMORY")
            || j.state.starts_with("NODE_FAIL")
        {
            RED
        } else {
            FG
        };
        Row::new(vec![
            j.id.as_str(),
            j.name.as_str(),
            j.state.as_str(),
            j.elapsed.as_str(),
            j.exit_code.as_str(),
            j.end.as_str(),
        ])
        .style(Style::default().fg(color))
    });
    f.render_stateful_widget(
        Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Min(12),
                Constraint::Length(14),
                Constraint::Length(10),
                Constraint::Length(5),
                Constraint::Length(if inner.width >= 95 { 19 } else { 0 }),
            ],
        )
        .header(
            Row::new(["JOB", "NAME", "STATE", "ELAPSED", "EXIT", "ENDED"])
                .style(Style::default().fg(DIM)),
        )
        .row_highlight_style(selection_style())
        .highlight_symbol("› "),
        inner,
        &mut local_state,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounting_and_paths() {
        let record = [
            "12_3",
            "me",
            "job with | pipe",
            "gpu",
            "FAILED",
            "2026-10-18",
            "00:01",
            "01:00",
            "gres/gpu=1",
            "node",
            "2026-10-18",
            "1:0",
            "/tmp/work dir",
            "2026-10-18T01:00:00",
            "2026-10-18T00:59:00",
            "8",
            "4Gc",
            "students",
            "normal",
            "gres/gpu=1,mem=32G",
            "None",
        ]
        .join("\x1f");
        let job = parse(&record).unwrap().remove(0);
        assert_eq!(job.name, "job with | pipe");
        assert_eq!(job.start, "2026-10-18T01:00:00");
        assert_eq!(job.cpus, "8");
        assert!(parse("bad record").is_err());
        let control = " JobId=42 ArrayJobId=12 ArrayTaskId=3 WorkDir=/tmp/work dir StdOut=logs/%x-%A_%04a.out StdErr=/tmp/err Power=";
        assert_eq!(field(control, "WorkDir"), Some("/tmp/work dir"));
        assert_eq!(
            expand(field(control, "StdOut").unwrap(), &job, control).unwrap(),
            PathBuf::from("/tmp/work dir/logs/job with | pipe-12_0003.out")
        );
        assert_eq!(
            expand("slurm-%j.out", &job, control).unwrap(),
            PathBuf::from("/tmp/work dir/slurm-42.out")
        );
        assert!(expand("log-%N", &job, control).is_err());
    }
    #[test]
    fn filename_boundaries() {
        assert!(filename_matches("wmf-smoke-1698.out", "1698"));
        assert!(filename_matches("slurm-12_3.out", "12_3"));
        assert!(!filename_matches("wmf-11698.out", "1698"));
        assert!(!filename_matches("slurm-1698_3.out", "1698"));
        assert!(!filename_matches("wmf-16980.out", "1698"));
    }
    #[test]
    fn recursive_search_includes_ignored_nested_logs() {
        let root = std::env::temp_dir().join(format!(
            "mytop-search-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nested = root.join("unusual/.hidden/deep/output");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join(".ignore"), "*\n").unwrap();
        for file in [
            "train_1698.out",
            "train-11698.out",
            "train-16980.out",
            "slurm-1698_3.out",
            "model-1698.pt",
        ] {
            std::fs::write(nested.join(file), "test").unwrap();
        }
        let mut job = Job {
            id: "1698".into(),
            work_dir: root.to_str().unwrap().into(),
            ..Default::default()
        };
        let found = discover_logs(&job, Duration::from_secs(2)).unwrap();
        assert_eq!(found, vec![nested.join("train_1698.out")]);
        job.id = "999999".into();
        assert!(
            discover_logs(&job, Duration::from_secs(2))
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn log_navigation_and_visual_selection() {
        let mut log = LogView {
            lines: (0..100).map(|i| format!("line {i}")).collect(),
            ..Default::default()
        };
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        log.key(key('G'), 20);
        assert_eq!((log.cursor, log.offset), (99, 80));
        log.key(key('g'), 20);
        log.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL), 20);
        assert_eq!((log.cursor, log.offset), (10, 10));
        log.key(key('v'), 20);
        log.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL), 20);
        assert_eq!(log.cursor, 0);
        assert_eq!(
            log.selected_text().unwrap(),
            (0..=10).map(|i| format!("line {i}\n")).collect::<String>()
        );
        log.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 20);
        assert!(log.anchor.is_none());
        log.key(key('k'), 20);
        assert_eq!(log.cursor, 0);
    }
    #[test]
    fn log_search_is_incremental_wraps_and_escape_restores_context() {
        let mut log = LogView {
            lines: ["first error", "healthy", "second error", "last"]
                .map(str::to_owned)
                .into(),
            cursor: 1,
            anchor: Some(1),
            ..Default::default()
        };
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        log.key(key('/'), 2);
        for c in "error".chars() {
            log.key(key(c), 2);
        }
        assert!(log.editing_search);
        assert_eq!(log.cursor, 2);
        assert_eq!(log.matches, [0, 2]);
        log.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), 2);
        assert!(!log.editing_search);
        log.key(key('n'), 2);
        assert_eq!(log.cursor, 0);
        log.key(key('N'), 2);
        assert_eq!(log.cursor, 2);
        let offset = log.offset;
        log.key(key('/'), 2);
        // Shortcut letters are query text while the search is being edited.
        log.key(key('q'), 2);
        log.key(key('y'), 2);
        assert_eq!(log.search, "qy");
        assert!(log.matches.is_empty());
        log.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 2);
        assert!(!log.editing_search);
        assert_eq!(log.search, "error");
        assert_eq!((log.cursor, log.offset), (2, offset));
        assert_eq!(log.anchor, Some(1));
        // A second Escape leaves search text alone and cancels visual mode.
        log.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 2);
        assert_eq!(log.anchor, None);
        assert_eq!(log.search, "error");
    }
    #[test]
    fn ansi_sequences_are_removed_without_losing_log_text() {
        let text = "\x1b[31mERROR\x1b[0m: \x1b]8;;https://example.test\x1b\\link\x1b]8;;\x07\n\
                    \x1bPprivate control\x1b\\next\tline\r\n\x1b(Bdone\0";
        assert_eq!(sanitize_log(text), "ERROR: link\nnext\tline\ndone");
        assert_eq!(sanitize_log("visible\x1b[31"), "visible");
    }
    #[test]
    fn reload_bounds_line_count_and_preserves_search_position() {
        let path = std::env::temp_dir().join(format!("mytop-reload-{}", std::process::id()));
        std::fs::write(&path, "\n".repeat(DISPLAY_LINES + 100)).unwrap();
        let mut view = reload(std::slice::from_ref(&path), "job");
        assert!(view.lines.len() <= DISPLAY_LINES + 3);
        assert!(
            view.lines
                .iter()
                .any(|l| l.contains("Showing last 50000 lines"))
        );
        std::fs::write(&path, "one\n\x1b[31mERROR\x1b[0m\nthree\n").unwrap();
        view.cursor = 2;
        view.search = "ERROR".into();
        view.anchor = Some(1);
        let mut refreshed = reload(std::slice::from_ref(&path), "job");
        refreshed.restore_position(&view);
        assert_eq!(refreshed.lines[2], "ERROR");
        assert_eq!(refreshed.matches, [2]);
        assert_eq!(refreshed.cursor, 2);
        assert_eq!(refreshed.anchor, Some(1));
        // A reload response may arrive after typing has started. Keep the
        // editing mode and clamp its saved position if the file got shorter.
        refreshed.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE), 2);
        refreshed.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE), 2);
        refreshed.copying = true;
        std::fs::write(&path, "short\n").unwrap();
        let mut shorter = reload(std::slice::from_ref(&path), "job");
        shorter.restore_position(&refreshed);
        assert!(shorter.editing_search && shorter.copying);
        assert_eq!(shorter.search, "o");
        shorter.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), 2);
        assert!(!shorter.editing_search);
        assert_eq!(shorter.search, "ERROR");
        assert!(shorter.cursor < shorter.lines.len());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn yank_rejects_oversized_files_without_reading_them() {
        let path = std::env::temp_dir().join(format!("mytop-oversize-{}", std::process::id()));
        let file = File::create(&path).unwrap();
        file.set_len(64 * 1024 * 1024 + 1).unwrap();
        let error = full_log_text(std::slice::from_ref(&path)).unwrap_err();
        assert!(error.to_string().contains("64 MiB clipboard limit"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn previous_history_stays_visible_during_refresh_and_after_failure() {
        use clap::Parser;
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new(Args::parse_from(["mytop", "--tab", "4"]));
        app.history.push(Job {
            id: "4242".into(),
            user: app.args.user.clone(),
            name: "retained-job".into(),
            partition: "gpu".into(),
            state: "COMPLETED".into(),
            ..Default::default()
        });
        app.rebuild_views();
        let mut terminal = Terminal::new(TestBackend::new(140, 10)).unwrap();
        for failed in [false, true] {
            app.history_loading = !failed;
            app.history_error = failed.then(|| "temporary timeout".into());
            terminal.draw(|f| draw(f, f.area(), &mut app)).unwrap();
            let screen: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(screen.contains("retained-job"));
            if failed {
                assert!(screen.contains("keeping previous jobs"));
            }
        }
    }
    #[test]
    fn yank_reads_whole_file_without_display_truncation() {
        let path = std::env::temp_dir().join(format!("mytop-yank-{}", std::process::id()));
        let text = format!("BEGIN\n{}\nEND\n", "x".repeat(2 * 1024 * 1024 + 100));
        std::fs::write(&path, &text).unwrap();
        assert_eq!(full_log_text(std::slice::from_ref(&path)).unwrap(), text);
        std::fs::remove_file(&path).unwrap();
        assert!(full_log_text(&[path]).is_err());
        assert!(full_log_text(&[]).is_err());
    }
}
