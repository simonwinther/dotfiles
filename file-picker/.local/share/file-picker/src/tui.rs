//! Disposable Ratatui picker. Scanning and fuzzy matching never run on the UI thread.
use crate::{
    Options, Result, SortOrder, appearance_colors, executable_path, home, scan_args,
    terminate_with_parent,
};
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
};
use nucleo::{
    Config, Matcher, Nucleo,
    pattern::{CaseMatching, Normalization},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Wrap,
    },
};
use std::{
    ffi::OsString,
    io::{self, BufRead, BufReader},
    os::unix::{ffi::OsStringExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelectionAction {
    Copy,
    Path,
    Drag,
    Open,
    OpenPdf,
}

#[derive(Debug)]
pub(super) struct Selection {
    pub paths: Vec<PathBuf>,
    pub action: SelectionAction,
}

#[derive(Clone, Copy)]
enum Choice {
    Default,
    Action(SelectionAction),
}

#[derive(Clone)]
struct File {
    path: PathBuf,
    display: String,
}

fn display_path(path: &Path) -> String {
    let text = path
        .strip_prefix(home())
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|_| path.display().to_string());
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

enum Update {
    Snapshot(Box<Nucleo<File>>, bool),
    Working(bool),
    Warning(String),
}

enum ScanEvent {
    Update(Update),
    Finished(io::Result<()>),
}

fn matcher() -> Nucleo<File> {
    Nucleo::new(Config::DEFAULT.match_paths(), Arc::new(|| {}), Some(2), 1)
}

fn record(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    if reader.read_until(0, &mut bytes)? == 0 {
        return Ok(None);
    }
    if bytes.pop() != Some(0) {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Incomplete file index snapshot",
        ));
    }
    Ok(Some(bytes))
}

fn read_updates(stdout: impl io::Read, sender: &mpsc::Sender<ScanEvent>) -> io::Result<()> {
    let mut reader = BufReader::new(stdout);
    let send = |update| {
        sender
            .send(ScanEvent::Update(update))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Picker closed"))
    };
    while let Some(header) = record(&mut reader)? {
        match header.as_slice() {
            b"cached" | b"fresh" => {
                // Build replacement candidates off the UI thread. Keep the old
                // matcher visible until the new snapshot matches the current query.
                let next = matcher();
                let injector = next.injector();
                let preview_matcher = matcher();
                let preview_injector = preview_matcher.injector();
                let mut preview = Some((preview_matcher, preview_injector));
                let mut count = 0;
                loop {
                    let bytes = record(&mut reader)?.ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Incomplete file index snapshot",
                        )
                    })?;
                    if bytes.is_empty() {
                        break;
                    }
                    if bytes.first() != Some(&b'/') {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Invalid indexed file path",
                        ));
                    }
                    let path = PathBuf::from(OsString::from_vec(bytes));
                    let file = File {
                        display: display_path(&path),
                        path,
                    };
                    if let Some((_, preview)) = &preview {
                        preview.push(file.clone(), |file, columns| {
                            columns[0] = file.display.as_str().into();
                        });
                    }
                    injector.push(file, |file, columns| {
                        columns[0] = file.display.as_str().into();
                    });
                    count += 1;
                    if count == 128
                        && let Some((preview, _injector)) = preview.take()
                    {
                        // Display the first page without waiting for every path
                        // to be decoded and inserted into the complete matcher.
                        send(Update::Snapshot(Box::new(preview), false))?;
                    }
                }
                send(Update::Snapshot(Box::new(next), true))?;
            }
            b"updating" => send(Update::Working(true))?,
            b"ready" => send(Update::Working(false))?,
            b"warning" => {
                let error = record(&mut reader)?.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Incomplete file index warning",
                    )
                })?;
                send(Update::Warning(
                    String::from_utf8_lossy(&error)
                        .chars()
                        .map(|c| if c.is_control() { ' ' } else { c })
                        .collect(),
                ))?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid file index response",
                ));
            }
        }
    }
    Ok(())
}

/// The feed ends with its terminal. A separate, deduplicated index refresh can
/// finish after a quick selection, without retaining any window or UI process.
struct Scan {
    child: Child,
    reader: mpsc::Receiver<ScanEvent>,
    read_done: bool,
}

impl Scan {
    fn start(options: &Options) -> Result<Self> {
        let mut command = Command::new(executable_path()?);
        scan_args(&mut command, options, "--index-stream");
        terminate_with_parent(&mut command);
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        let stdout = child.stdout.take().unwrap();
        let (sender, reader) = mpsc::channel();
        thread::spawn(move || {
            let result = read_updates(stdout, &sender);
            let _ = sender.send(ScanEvent::Finished(result));
        });
        Ok(Self {
            child,
            reader,
            read_done: false,
        })
    }

    fn poll(&mut self) -> Result<(Vec<Update>, bool)> {
        let mut updates = Vec::new();
        while !self.read_done {
            match self.reader.try_recv() {
                Ok(ScanEvent::Update(update)) => updates.push(update),
                Ok(ScanEvent::Finished(result)) => {
                    result?;
                    self.read_done = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("File index reader stopped".into());
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if self.read_done
            && let Some(status) = self.child.try_wait()?
        {
            if !status.success() {
                return Err(format!("File index feed failed ({status})").into());
            }
            return Ok((updates, true));
        }
        Ok((updates, false))
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGTERM);
            }
        }
        let _ = self.child.wait();
    }
}

struct Jump {
    labels: Vec<String>,
    prefix: String,
}

fn labels(count: usize) -> Vec<String> {
    let alphabet: Vec<_> = "ABCDEFGHIJKLMNOPQRSTUVWXYZÆØÅ".chars().collect();
    let mut width = 1;
    let mut capacity = alphabet.len();
    while capacity < count {
        width += 1;
        capacity = capacity.saturating_mul(alphabet.len());
    }
    (0..count)
        .map(|mut index| {
            let mut code = vec![alphabet[0]; width];
            for letter in code.iter_mut().rev() {
                *letter = alphabet[index % alphabet.len()];
                index /= alphabet.len();
            }
            code.into_iter().collect()
        })
        .collect()
}

struct Picker {
    initial: Options,
    options: Options,
    matcher: Nucleo<File>,
    highlighter: Matcher,
    replacement: Option<Box<Nucleo<File>>>,
    replacement_complete: bool,
    complete: bool,
    scope: (PathBuf, bool, bool, bool),
    loaded: bool,
    updating: bool,
    scan: Option<Scan>,
    query: String,
    cursor: usize,
    selected: usize,
    offset: usize,
    rows: usize,
    marked: Vec<PathBuf>,
    jump: Option<Jump>,
    menu: Option<(PathBuf, usize)>,
    pending: Option<Choice>,
    error: Option<String>,
    palette: Palette,
}

#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    fg: Color,
    selected: Color,
    accent: Color,
    muted: Color,
    border: Color,
}

impl Palette {
    fn current() -> Self {
        let colors = appearance_colors();
        let rgb = |hex: &str| {
            let n = u32::from_str_radix(&hex[1..], 16).unwrap();
            Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
        };
        Self {
            bg: rgb(colors.bg),
            fg: rgb(colors.fg),
            selected: rgb(colors.selected),
            accent: rgb(colors.accent),
            muted: rgb(colors.muted),
            border: rgb(colors.border),
        }
    }
}

impl Picker {
    fn new(options: &Options) -> Self {
        Self {
            initial: options.clone(),
            options: options.clone(),
            matcher: matcher(),
            highlighter: Matcher::new(Config::DEFAULT.match_paths()),
            replacement: None,
            replacement_complete: true,
            complete: false,
            scope: (
                options.root.clone(),
                options.all,
                options.pdf,
                options.recent,
            ),
            loaded: false,
            updating: false,
            scan: None,
            query: String::new(),
            cursor: 0,
            selected: 0,
            offset: 0,
            rows: 1,
            marked: Vec::new(),
            jump: None,
            menu: None,
            pending: None,
            error: None,
            palette: Palette::current(),
        }
    }

    fn refresh(&mut self) -> Result<()> {
        self.scan = None;
        self.replacement = None;
        let scope = (
            self.options.root.clone(),
            self.options.all,
            self.options.pdf,
            self.options.recent,
        );
        if self.scope != scope {
            self.matcher.restart(true);
            self.marked.clear();
            self.selected = 0;
            self.offset = 0;
            self.loaded = false;
            self.complete = false;
            self.scope = scope;
        }
        self.jump = None;
        self.pending = None;
        self.error = None;
        self.updating = true;
        self.scan = Some(Scan::start(&self.options)?);
        Ok(())
    }

    fn receive(&mut self, update: Update) {
        match update {
            Update::Snapshot(mut next, complete) => {
                // A partial page must never replace an already complete list.
                if !complete && self.complete {
                    return;
                }
                next.pattern.reparse(
                    0,
                    &self.query,
                    CaseMatching::Smart,
                    Normalization::Smart,
                    false,
                );
                self.replacement = Some(next);
                self.replacement_complete = complete;
            }
            Update::Working(working) => self.updating = working,
            Update::Warning(error) => self.error = Some(error),
        }
    }

    fn replace_if_ready(&mut self) -> bool {
        if self.jump.is_some() || self.menu.is_some() {
            return false;
        }
        let Some(next) = &mut self.replacement else {
            return false;
        };
        if next.tick(1).running {
            return false;
        }
        let next = self.replacement.take().unwrap();
        let selected = self.current_path();
        let snapshot = next.snapshot();
        self.selected = selected
            .and_then(|path| {
                snapshot
                    .matched_items(..)
                    .position(|item| item.data.path == path)
            })
            .unwrap_or(0);
        if !self.marked.is_empty() {
            let present: std::collections::HashSet<_> = (0..snapshot.item_count())
                .filter_map(|i| snapshot.get_item(i).map(|item| &item.data.path))
                .collect();
            self.marked.retain(|path| present.contains(path));
        }
        self.matcher = *next;
        self.loaded = true;
        self.complete = self.replacement_complete;
        true
    }

    fn update_query(&mut self) {
        self.matcher.pattern.reparse(
            0,
            &self.query,
            CaseMatching::Smart,
            Normalization::Smart,
            false,
        );
        if let Some(next) = &mut self.replacement {
            next.pattern.reparse(
                0,
                &self.query,
                CaseMatching::Smart,
                Normalization::Smart,
                false,
            );
        }
        self.selected = 0;
        self.offset = 0;
        self.pending = None;
        self.error = None;
    }

    fn insert(&mut self, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        self.query.insert_str(self.cursor, &text);
        self.cursor += text.len();
        self.update_query();
    }

    fn count(&self) -> usize {
        self.matcher.snapshot().matched_item_count() as usize
    }

    fn selection_ready(&self) -> bool {
        self.loaded && self.replacement.is_none() && (self.complete || self.query.is_empty())
    }

    fn move_by(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.count().saturating_sub(1));
    }

    fn current_path(&self) -> Option<PathBuf> {
        self.matcher
            .snapshot()
            .get_matched_item(self.selected as u32)
            .map(|item| item.data.path.clone())
    }

    fn toggle(&mut self, backwards: bool) {
        if self.options.pdf || self.options.recent {
            return;
        }
        if let Some(path) = self.current_path() {
            if let Some(index) = self.marked.iter().position(|marked| marked == &path) {
                self.marked.remove(index);
            } else {
                self.marked.push(path);
            }
            self.move_by(if backwards { -1 } else { 1 });
        }
    }

    fn choose(&mut self, choice: Choice) -> Option<Selection> {
        let paths = if self.marked.is_empty() {
            vec![self.current_path()?]
        } else {
            self.marked.clone()
        };
        let action = match choice {
            Choice::Default if self.options.recent => {
                self.menu = Some((paths[0].clone(), 0));
                return None;
            }
            Choice::Default if self.options.pdf => SelectionAction::OpenPdf,
            Choice::Default => SelectionAction::Copy,
            Choice::Action(action) => action,
        };
        if action == SelectionAction::Path && paths.len() != 1 {
            self.error = Some("Select one file to copy its path".into());
            return None;
        }
        Some(Selection { paths, action })
    }

    /// Returns true when the picker is cancelled. Actions are queued until the
    /// current fuzzy match completes, so fast typing + Enter cannot select a stale row.
    fn key(&mut self, key: KeyEvent) -> Result<bool> {
        if key.kind == KeyEventKind::Release {
            return Ok(false);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code == KeyCode::Esc || (ctrl && key.code == KeyCode::Char('c')) {
            return Ok(true);
        }
        if let Some((_, selected)) = &mut self.menu {
            match key.code {
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Down => *selected = (*selected + 1).min(2),
                KeyCode::Char('o') => *selected = 0,
                KeyCode::Char('c') => *selected = 1,
                KeyCode::Char('a') => *selected = 2,
                KeyCode::Enter => self.pending = Some(Choice::Default),
                _ => {}
            }
            return Ok(false);
        }
        if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('s' | 'S'))
        {
            self.jump = if self.jump.is_some() {
                None
            } else {
                Some(Jump {
                    labels: labels(self.count().saturating_sub(self.offset).min(self.rows)),
                    prefix: String::new(),
                })
            };
            return Ok(false);
        }
        if let Some(jump) = &mut self.jump {
            match key.code {
                KeyCode::Backspace => {
                    jump.prefix.pop();
                }
                KeyCode::Char(c) => {
                    jump.prefix.extend(c.to_uppercase());
                    if let Some(index) = jump.labels.iter().position(|label| label == &jump.prefix)
                    {
                        self.selected = self.offset + index;
                        self.jump = None;
                    } else if !jump
                        .labels
                        .iter()
                        .any(|label| label.starts_with(&jump.prefix))
                    {
                        self.jump = None;
                    }
                }
                _ => {}
            }
            return Ok(false);
        }
        match key.code {
            KeyCode::Enter => self.pending = Some(Choice::Default),
            KeyCode::Down => self.move_by(1),
            KeyCode::Up => self.move_by(-1),
            KeyCode::PageDown => self.move_by(self.rows as isize),
            KeyCode::PageUp => self.move_by(-(self.rows as isize)),
            KeyCode::Tab => self.toggle(false),
            KeyCode::BackTab => self.toggle(true),
            KeyCode::Char('d') if ctrl => self.move_by((self.rows / 2).max(1) as isize),
            KeyCode::Char('u') if ctrl => self.move_by(-((self.rows / 2).max(1) as isize)),
            KeyCode::Char('n') if ctrl => self.move_by(1),
            KeyCode::Char('p') if ctrl => self.move_by(-1),
            KeyCode::Char('o') if ctrl => {
                self.pending = Some(Choice::Action(SelectionAction::Open))
            }
            KeyCode::Char('g') if ctrl => {
                self.pending = Some(Choice::Action(SelectionAction::Drag))
            }
            KeyCode::Char('y') if ctrl => {
                self.pending = Some(Choice::Action(SelectionAction::Path))
            }
            KeyCode::Char('t') if ctrl => {
                self.options.sort = if self.options.sort == SortOrder::Name {
                    SortOrder::Recent
                } else {
                    SortOrder::Name
                };
                self.refresh()?;
            }
            KeyCode::Char('r') if ctrl => {
                self.options.root = self.initial.root.clone();
                self.options.all = self.initial.all;
                self.refresh()?;
            }
            KeyCode::Char('a') if ctrl && !self.options.pdf => {
                self.options.root = self.initial.root.clone();
                self.options.all = true;
                self.refresh()?;
            }
            KeyCode::Char('s') if ctrl && !self.options.pdf => {
                self.options.root = PathBuf::from("/");
                self.options.all = true;
                self.refresh()?;
            }
            KeyCode::Left => {
                self.cursor = self.query[..self.cursor]
                    .char_indices()
                    .last()
                    .map_or(0, |(i, _)| i)
            }
            KeyCode::Right => {
                self.cursor += self.query[self.cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.query.len(),
            KeyCode::Char('e') if ctrl => self.cursor = self.query.len(),
            KeyCode::Backspace | KeyCode::Char('h') if key.code == KeyCode::Backspace || ctrl => {
                if self.cursor > 0 {
                    let start = self.query[..self.cursor].char_indices().last().unwrap().0;
                    self.query.drain(start..self.cursor);
                    self.cursor = start;
                    self.update_query();
                }
            }
            KeyCode::Delete => {
                if let Some(c) = self.query[self.cursor..].chars().next() {
                    self.query.drain(self.cursor..self.cursor + c.len_utf8());
                    self.update_query();
                }
            }
            KeyCode::Char('w') if ctrl => {
                let prefix = &self.query[..self.cursor];
                let start = prefix
                    .trim_end()
                    .char_indices()
                    .rev()
                    .find(|(_, c)| c.is_whitespace())
                    .map_or(0, |(i, c)| i + c.len_utf8());
                self.query.drain(start..self.cursor);
                self.cursor = start;
                self.update_query();
            }
            KeyCode::Char('k') if ctrl => {
                self.query.truncate(self.cursor);
                self.update_query();
            }
            KeyCode::Char('l') if ctrl => {
                self.query.clear();
                self.cursor = 0;
                self.update_query();
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert(&c.to_string())
            }
            _ => {}
        }
        Ok(false)
    }

    fn finish_menu(&mut self) -> Option<Selection> {
        let (path, index) = self.menu.take()?;
        let action = match index {
            0 if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) =>
            {
                SelectionAction::OpenPdf
            }
            0 => SelectionAction::Open,
            1 => SelectionAction::Path,
            _ => SelectionAction::Copy,
        };
        Some(Selection {
            paths: vec![path],
            action,
        })
    }

    fn draw(&mut self, frame: &mut Frame) {
        let p = self.palette;
        let base = Style::default().fg(p.fg).bg(p.bg);
        let area = frame.area();
        frame.render_widget(Block::default().style(base), area);
        let inner = area.inner(ratatui::layout::Margin::new(2, 1));
        let footer_height = if self.options.pdf || self.options.recent {
            2
        } else {
            3
        };
        let [heading, search, files, status, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(footer_height),
        ])
        .areas(inner);
        let title = if self.options.pdf {
            "Find a PDF"
        } else if self.options.recent {
            "Recent files"
        } else {
            "Find a file"
        };
        frame.render_widget(
            Paragraph::new(title).style(Style::default().fg(p.accent).add_modifier(Modifier::BOLD)),
            heading,
        );
        let search_block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(p.border))
            .title(" Search ");
        let input = search_block.inner(search);
        let cursor_width = UnicodeWidthStr::width(&self.query[..self.cursor]);
        let scroll = cursor_width.saturating_sub(input.width.saturating_sub(1) as usize) as u16;
        frame.render_widget(
            Paragraph::new(self.query.as_str())
                .scroll((0, scroll))
                .block(search_block),
            search,
        );
        if input.width > 0 && input.height > 0 && self.jump.is_none() && self.menu.is_none() {
            frame.set_cursor_position((
                input.x + (cursor_width as u16).saturating_sub(scroll),
                input.y,
            ));
        }
        let rows = files.height.max(1) as usize;
        if self.rows != rows {
            self.jump = None;
            self.rows = rows;
        }
        self.selected = self.selected.min(self.count().saturating_sub(1));
        if self.selected < self.offset {
            self.offset = self.selected;
        }
        if self.selected >= self.offset + self.rows {
            self.offset = self.selected + 1 - self.rows;
        }
        let end = (self.offset + self.rows).min(self.count());
        let snapshot = self.matcher.snapshot();
        let items: Vec<_> = snapshot
            .matched_items(self.offset as u32..end as u32)
            .enumerate()
            .map(|(row, item)| {
                let mut spans = vec![Span::styled(
                    if self.marked.contains(&item.data.path) {
                        "* "
                    } else {
                        "  "
                    },
                    Style::default().fg(p.accent),
                )];
                let mut reserved = 4;
                if let Some(jump) = &self.jump {
                    let label = &jump.labels[row];
                    reserved += label.width() + 1;
                    spans.push(Span::styled(
                        if label.starts_with(&jump.prefix) {
                            format!("{label} ")
                        } else {
                            " ".repeat(label.width() + 1)
                        },
                        Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                    ));
                }
                let mut indices = Vec::new();
                snapshot.pattern().column_pattern(0).indices(
                    item.matcher_columns[0].slice(..),
                    &mut self.highlighter,
                    &mut indices,
                );
                indices.sort_unstable();
                indices.dedup();
                let graphemes: Vec<_> = item.data.display.graphemes(true).collect();
                let available = (files.width as usize).saturating_sub(reserved);
                let mut width = item.data.display.width();
                let mut start = 0;
                if width > available {
                    while start < graphemes.len() && width > available.saturating_sub(1) {
                        width = width.saturating_sub(graphemes[start].width());
                        start += 1;
                    }
                    if available > 0 {
                        spans.push(Span::styled("…", Style::default().fg(p.muted)));
                    }
                }
                for (i, grapheme) in graphemes.iter().enumerate().skip(start) {
                    let style = if indices.binary_search(&(i as u32)).is_ok() {
                        Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(p.fg)
                    };
                    spans.push(Span::styled(grapheme.to_string(), style));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let mut state = ListState::default().with_selected(if self.count() == 0 {
            None
        } else {
            Some(self.selected - self.offset)
        });
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_spacing(HighlightSpacing::Always)
                .highlight_style(Style::default().bg(p.selected)),
            files,
            &mut state,
        );
        if self.count() == 0 {
            frame.render_widget(
                Paragraph::new(if self.updating {
                    "Loading files…"
                } else {
                    "No matching files"
                })
                .style(Style::default().fg(p.muted)),
                files,
            );
        }
        let scope = if self.options.root == Path::new("/") {
            "whole PC".to_owned()
        } else if self.options.all {
            "all files".to_owned()
        } else {
            display_path(&self.options.root)
        };
        let detail = self.error.clone().unwrap_or_else(|| {
            if !self.complete && self.count() > 0 {
                return format!("{} shown · loading remaining files…", self.count());
            }
            format!(
                "{} / {} files · {} · {}{}{}",
                self.count(),
                snapshot.item_count(),
                scope,
                self.options.sort.label(),
                if self.updating { " · updating…" } else { "" },
                if self.marked.is_empty() {
                    String::new()
                } else {
                    format!(" · {} selected", self.marked.len())
                }
            )
        });
        frame.render_widget(
            Paragraph::new(detail).style(Style::default().fg(p.muted)),
            status,
        );
        let hints = if self.options.recent {
            "Enter choose action   Alt+S jump   Ctrl+D/U half page\nCtrl+T sort   Ctrl+R refresh   Esc close"
        } else if self.options.pdf {
            "Enter open   Alt+S jump   Ctrl+D/U half page\nCtrl+T sort   Ctrl+R refresh   Esc close"
        } else {
            "Enter copy   Ctrl+O open   Ctrl+G drag   Ctrl+Y path   Tab select\nAlt+S jump   Ctrl+D/U half page   Ctrl+T sort   Ctrl+R refresh\nCtrl+A all files   Ctrl+S whole PC   Esc close"
        };
        frame.render_widget(
            Paragraph::new(hints).style(Style::default().fg(p.muted)),
            footer,
        );
        if let Some((path, selected)) = &self.menu {
            let popup = Rect::new(
                area.x + area.width.saturating_sub(64) / 2,
                area.y + area.height.saturating_sub(9) / 2,
                area.width.min(64),
                area.height.min(9),
            );
            frame.render_widget(Clear, popup);
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .title(" Choose an action ")
                .style(base)
                .border_style(Style::default().fg(p.accent));
            let content = block.inner(popup);
            frame.render_widget(block, popup);
            let [name, actions] =
                Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(content);
            frame.render_widget(
                Paragraph::new(display_path(path)).wrap(Wrap { trim: false }),
                name,
            );
            let mut state = ListState::default().with_selected(Some(*selected));
            frame.render_stateful_widget(
                List::new(["o  Open", "c  Copy path", "a  Copy attachment"])
                    .highlight_symbol("> ")
                    .highlight_style(Style::default().bg(p.selected)),
                actions,
                &mut state,
            );
        }
    }
}

struct PasteMode;
impl Drop for PasteMode {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
    }
}

pub(super) fn pick(options: &Options) -> Result<Option<Selection>> {
    let mut picker = Picker::new(options);
    ratatui::run(|terminal| {
        execute!(io::stdout(), EnableBracketedPaste)?;
        let _paste = PasteMode;
        picker.refresh()?;
        let mut redraw = true;
        loop {
            if let Some(scan) = &mut picker.scan {
                match scan.poll() {
                    Ok((updates, finished)) => {
                        for update in updates {
                            picker.receive(update);
                            redraw = true;
                        }
                        if finished {
                            picker.scan = None;
                            picker.updating = false;
                            redraw = true;
                        }
                    }
                    Err(error) => {
                        picker.error = Some(error.to_string());
                        picker.scan = None;
                        picker.updating = false;
                        picker.pending = None;
                        redraw = true;
                    }
                }
            }
            redraw |= picker.replace_if_ready();
            // Freeze the snapshot while jump labels refer to the visible rows.
            let running = if picker.jump.is_none() {
                let status = picker.matcher.tick(1);
                redraw |= status.changed;
                status.running
            } else {
                true
            };
            if !running
                && picker.selection_ready()
                && let Some(choice) = picker.pending.take()
            {
                let selection = if picker.menu.is_some() {
                    picker.finish_menu()
                } else {
                    picker.choose(choice)
                };
                if let Some(selection) = selection {
                    if let Err(error) = crate::checked_files(selection.paths.clone()) {
                        picker.marked.retain(|path| path.is_file());
                        picker.refresh()?;
                        picker.error = Some(format!("{error}; refreshing files…"));
                        redraw = true;
                        continue;
                    }
                    return Ok(Some(selection));
                }
            }
            if redraw {
                terminal.draw(|frame| picker.draw(frame))?;
                redraw = false;
            }
            let delay = if running || picker.updating || picker.replacement.is_some() {
                8
            } else {
                250
            };
            if !event::poll(Duration::from_millis(delay))? {
                continue;
            }
            redraw = true;
            match event::read()? {
                Event::Key(key) => {
                    if picker.key(key)? {
                        return Ok(None);
                    }
                }
                Event::Paste(text) if picker.jump.is_none() && picker.menu.is_none() => {
                    picker.insert(&text)
                }
                Event::Resize(_, _) => picker.jump = None,
                _ => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use std::time::Instant;

    fn fixture(names: &[PathBuf]) -> Picker {
        let mut picker = Picker::new(&Options::parse([]).unwrap());
        let injector = picker.matcher.injector();
        for path in names {
            injector.push(
                File {
                    path: path.clone(),
                    display: display_path(path),
                },
                |file, columns| {
                    columns[0] = file.display.as_str().into();
                },
            );
        }
        settle(&mut picker);
        picker.loaded = true;
        picker.complete = true;
        picker
    }

    fn settle(picker: &mut Picker) {
        let start = Instant::now();
        while picker.matcher.tick(10).running {
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn fuzzy_unicode_search_and_multiselect_preserve_exact_paths() {
        let names = [
            PathBuf::from("/tmp/ø paper\nfinal.pdf"),
            PathBuf::from("/tmp/another.pdf"),
        ];
        let mut picker = fixture(&names);
        picker.insert("øpp");
        settle(&mut picker);
        assert_eq!(picker.count(), 1);
        assert_eq!(picker.current_path(), Some(names[0].clone()));
        picker.toggle(false);
        picker.query.clear();
        picker.cursor = 0;
        picker.update_query();
        settle(&mut picker);
        picker.selected = 1;
        picker.toggle(false);
        assert_eq!(picker.choose(Choice::Default).unwrap().paths, names);
        assert!(
            picker
                .choose(Choice::Action(SelectionAction::Path))
                .is_none()
        );
        picker.selected = 1;
        picker.toggle(true);
        assert_eq!(picker.marked, [names[0].clone()]);
        picker.toggle(true);
        assert!(picker.marked.is_empty());
        picker.selected = 1;
        picker.toggle(true);
        assert_eq!(picker.marked, [names[1].clone()]);
    }

    #[test]
    fn unicode_query_editing_and_paste_cannot_inject_terminal_controls() {
        let mut picker = fixture(&[]);
        picker.insert("aø\x1b\nb");
        assert_eq!(picker.query, "aøb");
        picker
            .key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE))
            .unwrap();
        picker
            .key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .unwrap();
        assert_eq!(picker.query, "ab");
        picker.insert("æ");
        assert_eq!(picker.query, "aæb");
    }

    #[test]
    fn jump_labels_follow_viewport_and_accept_danish_lowercase() {
        assert_eq!(&labels(29)[26..], ["Æ", "Ø", "Å"]);
        assert_eq!(&labels(31)[..3], ["AA", "AB", "AC"]);
        let names: Vec<_> = (0..40)
            .map(|n| PathBuf::from(format!("/tmp/{n:02}")))
            .collect();
        let mut picker = fixture(&names);
        picker.rows = 29;
        picker
            .key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT))
            .unwrap();
        picker
            .key(KeyEvent::new(KeyCode::Char('ø'), KeyModifiers::NONE))
            .unwrap();
        assert_eq!(picker.selected, 27);
        assert_eq!(picker.query, "");
        assert!(
            picker
                .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap()
        );
    }

    #[test]
    fn recent_pdf_menu_opens_in_zathura_and_escape_always_cancels() {
        let path = PathBuf::from("/tmp/report.PDF");
        let mut picker = fixture(std::slice::from_ref(&path));
        picker.options.recent = true;
        assert!(picker.choose(Choice::Default).is_none());
        let selection = picker.finish_menu().unwrap();
        assert_eq!(selection.action, SelectionAction::OpenPdf);
        assert_eq!(selection.paths, [path]);
        assert!(
            picker
                .key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .unwrap()
        );
    }

    #[test]
    fn rendering_survives_tiny_terminals_resize_and_hostile_filenames() {
        let names = [PathBuf::from("/tmp/\x1b[31mø\nreport.pdf")];
        let mut picker = fixture(&names);
        for (width, height) in [(100, 30), (8, 5), (1, 1), (160, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| picker.draw(frame)).unwrap();
            assert!(!display_path(&names[0]).chars().any(char::is_control));
        }
    }

    #[test]
    fn index_snapshots_replace_atomically_and_preserve_query_focus_and_marks() {
        let alpha = PathBuf::from("/tmp/alpha.pdf");
        let beta = PathBuf::from("/tmp/beta.pdf");
        let mut picker = fixture(&[alpha.clone(), beta.clone()]);
        picker.marked.push(alpha.clone());
        picker.insert("beta");
        settle(&mut picker);
        let (sender, receiver) = mpsc::channel();
        read_updates(
            b"fresh\0/tmp/new.pdf\0/tmp/beta.pdf\0/tmp/alpha.pdf\0\0ready\0".as_slice(),
            &sender,
        )
        .unwrap();
        for event in receiver.try_iter() {
            if let ScanEvent::Update(update) = event {
                picker.receive(update);
            }
        }
        assert_eq!(picker.current_path(), Some(beta.clone()));
        let start = Instant::now();
        while !picker.replace_if_ready() {
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(picker.query, "beta");
        assert_eq!(picker.current_path(), Some(beta));
        assert_eq!(picker.marked, [alpha]);
        assert_eq!(picker.count(), 1);
        assert_eq!(picker.matcher.snapshot().item_count(), 3);
    }

    #[test]
    fn empty_fresh_snapshot_removes_deleted_rows_and_marked_files() {
        let mut picker = fixture(&[PathBuf::from("/tmp/removed.pdf")]);
        picker.toggle(false);
        picker.receive(Update::Snapshot(Box::new(matcher()), true));
        let start = Instant::now();
        while !picker.replace_if_ready() {
            assert!(start.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(picker.count(), 0);
        assert!(picker.marked.is_empty());
    }

    #[test]
    fn index_protocol_preserves_raw_paths_and_rejects_partial_or_invalid_frames() {
        let (sender, receiver) = mpsc::channel();
        read_updates(b"cached\0/tmp/raw-\xff\n.pdf\0\0".as_slice(), &sender).unwrap();
        let ScanEvent::Update(Update::Snapshot(mut next, true)) = receiver.try_recv().unwrap()
        else {
            panic!("Expected snapshot");
        };
        while next.tick(10).running {
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            next.snapshot()
                .get_matched_item(0)
                .unwrap()
                .data
                .path
                .as_os_str()
                .as_encoded_bytes(),
            b"/tmp/raw-\xff\n.pdf"
        );
        assert!(read_updates(b"fresh\0/tmp/incomplete".as_slice(), &sender).is_err());
        assert!(read_updates(b"fresh\0not-an-absolute-path\0\0".as_slice(), &sender).is_err());
    }

    #[test]
    fn first_page_is_available_before_the_full_index_and_search_accept_waits() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || read_updates(reader, &sender));
        writer.write_all(b"cached\0").unwrap();
        for number in 0..128 {
            write!(writer, "/tmp/recent-{number}.txt\0").unwrap();
        }
        let ScanEvent::Update(Update::Snapshot(preview, false)) =
            receiver.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("Expected early page");
        };
        let mut picker = Picker::new(&Options::parse([]).unwrap());
        picker.receive(Update::Snapshot(preview, false));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !picker.replace_if_ready() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(picker.count(), 128);
        assert!(picker.selection_ready());
        picker.insert("needle");
        assert!(!picker.selection_ready());
        writer.write_all(b"/tmp/needle.pdf\0\0").unwrap();
        drop(writer);
        let ScanEvent::Update(Update::Snapshot(full, true)) =
            receiver.recv_timeout(Duration::from_secs(2)).unwrap()
        else {
            panic!("Expected complete index");
        };
        picker.receive(Update::Snapshot(full, true));
        while !picker.replace_if_ready() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert!(picker.selection_ready());
        assert_eq!(
            picker.current_path(),
            Some(PathBuf::from("/tmp/needle.pdf"))
        );
        picker.receive(Update::Snapshot(Box::new(matcher()), false));
        assert!(picker.replacement.is_none());
        assert_eq!(picker.count(), 1);
        worker.join().unwrap().unwrap();
    }
}
