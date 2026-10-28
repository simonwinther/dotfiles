use crossterm::{
    cursor::{SetCursorStyle, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    style::ResetColor,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use project_picker::{
    cache::{View as Snapshot, load},
    schema::{Project, clean},
};
use ratatui::{
    Frame,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Margin},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{HighlightSpacing, List, ListState, Paragraph},
};
use std::{
    env,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const ACCENT: Color = Color::Rgb(166, 227, 161);
const TEXT: Color = Color::Rgb(205, 214, 244);
const MUTED: Color = Color::Rgb(133, 144, 163);
const SELECTED: Color = Color::Rgb(40, 53, 46);

fn score(name: &str, query: &str) -> Option<i64> {
    let haystack: Vec<_> = name.to_lowercase().chars().collect();
    let needle: Vec<_> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut position = 0;
    let mut total = 0;
    let mut previous = None;
    for c in needle {
        let index = position + haystack[position..].iter().position(|value| *value == c)?;
        total += index as i64;
        if index == 0 || !haystack[index - 1].is_alphanumeric() {
            total -= 12;
        }
        if previous == index.checked_sub(1) {
            total -= 8;
        }
        previous = Some(index);
        position = index + 1;
    }
    Some(total)
}

struct Picker {
    snapshot: Snapshot,
    query: String,
    matches: Vec<usize>,
    selected: usize,
    list_state: ListState,
}

impl Picker {
    fn new(snapshot: Snapshot) -> Self {
        let mut picker = Self {
            snapshot,
            query: String::new(),
            matches: vec![],
            selected: 0,
            list_state: ListState::default(),
        };
        picker.filter(false);
        picker
    }

    fn current(&self) -> Option<&Project> {
        self.matches
            .get(self.selected)
            .map(|index| &self.snapshot.projects[*index])
    }

    fn filter(&mut self, preserve: bool) {
        let id = preserve
            .then(|| self.current().map(|p| p.id.clone()))
            .flatten();
        let mut matches: Vec<_> = self
            .snapshot
            .projects
            .iter()
            .enumerate()
            .filter_map(|(index, project)| {
                score(&project.name, &self.query).map(|score| (score, index))
            })
            .collect();
        matches.sort_by_key(|(score, index)| (*score, *index));
        self.matches = matches.into_iter().map(|(_, index)| index).collect();
        self.selected = id
            .and_then(|id| {
                self.matches
                    .iter()
                    .position(|i| self.snapshot.projects[*i].id == id)
            })
            .unwrap_or(if preserve { self.selected } else { 0 })
            .min(self.matches.len().saturating_sub(1));
        if !preserve {
            *self.list_state.offset_mut() = 0;
        }
    }

    fn update(&mut self, snapshot: Snapshot) {
        let selected = self.current().map(|p| p.id.clone());
        self.snapshot = snapshot;
        self.filter(false);
        if let Some(index) = selected.and_then(|id| {
            self.matches
                .iter()
                .position(|i| self.snapshot.projects[*i].id == id)
        }) {
            self.selected = index;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if !self.matches.is_empty() {
            self.selected =
                (self.selected as isize + delta).clamp(0, self.matches.len() as isize - 1) as usize;
        }
    }
}

fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    for c in text.chars() {
        if result.width() + c.width().unwrap_or(0) > width - 1 {
            break;
        }
        result.push(c);
    }
    result.push('…');
    result
}

fn query_tail(text: &str, width: usize) -> String {
    let mut chars = text.chars().rev().take(256).collect::<Vec<_>>();
    let mut result = String::new();
    while let Some(c) = chars.pop() {
        result.push(c);
    }
    while result.width() > width {
        result.remove(0);
    }
    result
}

fn draw(frame: &mut Frame, picker: &mut Picker) {
    let area = frame.area();
    if area.width < 20 || area.height < 9 {
        frame.render_widget(Paragraph::new("Resize to show projects").fg(MUTED), area);
        return;
    }
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let title = Layout::horizontal([Constraint::Min(8), Constraint::Length(14)]).split(sections[0]);
    frame.render_widget(Paragraph::new("Projects").fg(TEXT).bold(), title[0]);
    let count = format!(
        "{} / {}",
        picker.matches.len(),
        picker.snapshot.projects.len()
    );
    frame.render_widget(
        Paragraph::new(count).fg(MUTED).alignment(Alignment::Right),
        title[1],
    );
    let query = query_tail(&picker.query, inner.width.saturating_sub(3) as usize);
    let input = if query.is_empty() {
        Span::styled("Search projects", Style::new().fg(MUTED))
    } else {
        Span::styled(&query, Style::new().fg(TEXT))
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::new().fg(ACCENT)),
            input,
        ])),
        sections[1],
    );
    frame.render_widget(
        Paragraph::new("─".repeat(inner.width as usize)).fg(MUTED),
        sections[2],
    );
    if picker.matches.is_empty() {
        frame.render_widget(
            Paragraph::new("  No matching projects").fg(MUTED),
            sections[3],
        );
    } else {
        let items = picker.matches.iter().map(|index| {
            fit(
                &picker.snapshot.projects[*index].name,
                inner.width.saturating_sub(3) as usize,
            )
        });
        let list = List::new(items)
            .fg(TEXT)
            .highlight_symbol("› ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(Style::new().fg(ACCENT).bg(SELECTED));
        picker.list_state.select(Some(picker.selected));
        frame.render_stateful_widget(list, sections[3], &mut picker.list_state);
    }
    frame.render_widget(
        Paragraph::new(fit(
            picker
                .snapshot
                .notice
                .as_deref()
                .unwrap_or("↑↓ move   enter open   esc close"),
            inner.width as usize,
        ))
        .fg(MUTED),
        sections[5],
    );
    frame.render_widget(
        Paragraph::new(fit(&picker.snapshot.last_synced, inner.width as usize)).fg(MUTED),
        sections[6],
    );
    frame.set_cursor_position((sections[1].x + 2 + query.width() as u16, sections[1].y));
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stderr(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            SetCursorStyle::BlinkingBar
        )?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stderr(),
            ResetColor,
            Show,
            SetCursorStyle::DefaultUserShape,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn run(cache: &Path) -> io::Result<Option<String>> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(io::Error::other("The picker needs an interactive terminal"));
    }
    let _guard = TerminalGuard::enter()?;
    let mut terminal = ratatui::Terminal::new(CrosstermBackend::new(io::stderr()))?;
    let mut picker = Picker::new(load(cache));
    let mut refresh_at = Instant::now();
    let mut dirty = true;
    let mut last_size = (0, 0);
    loop {
        let (columns, rows) = terminal::size()?;
        if (columns, rows) != last_size {
            last_size = (columns, rows);
            dirty = true;
        }
        if dirty {
            terminal.draw(|frame| draw(frame, &mut picker))?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Resize(_, _) => dirty = true,
                Event::Paste(text) => {
                    picker.query.extend(
                        clean(&text)
                            .chars()
                            .take(256usize.saturating_sub(picker.query.chars().count())),
                    );
                    picker.filter(false);
                    dirty = true;
                }
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    let control = key.modifiers.contains(KeyModifiers::CONTROL);
                    match key.code {
                        KeyCode::Esc => return Ok(None),
                        KeyCode::Char('c' | 'd') if control => return Ok(None),
                        KeyCode::Enter => {
                            if let Some(project) = picker.current() {
                                return Ok(Some(project.id.clone()));
                            }
                        }
                        KeyCode::Up | KeyCode::BackTab => picker.move_selection(-1),
                        KeyCode::Down | KeyCode::Tab => picker.move_selection(1),
                        KeyCode::Char('p' | 'k') if control => picker.move_selection(-1),
                        KeyCode::Char('n' | 'j') if control => picker.move_selection(1),
                        KeyCode::PageUp => {
                            picker.move_selection(-(rows.saturating_sub(8) as isize))
                        }
                        KeyCode::PageDown => picker.move_selection(rows.saturating_sub(8) as isize),
                        KeyCode::Home => picker.selected = 0,
                        KeyCode::End => picker.selected = picker.matches.len().saturating_sub(1),
                        KeyCode::Backspace => {
                            picker.query.pop();
                            picker.filter(false);
                        }
                        KeyCode::Char('u') if control => {
                            picker.query.clear();
                            picker.filter(false);
                        }
                        KeyCode::Char('w') if control => {
                            picker.query = picker.query.trim_end().to_string();
                            while picker
                                .query
                                .chars()
                                .last()
                                .is_some_and(|c| !c.is_whitespace())
                            {
                                picker.query.pop();
                            }
                            picker.filter(false);
                        }
                        KeyCode::Char(c)
                            if !control
                                && !key.modifiers.contains(KeyModifiers::ALT)
                                && !c.is_control()
                                && picker.query.chars().count() < 256 =>
                        {
                            picker.query.push(c);
                            picker.filter(false);
                        }
                        _ => {}
                    }
                    dirty = true;
                }
                _ => {}
            }
        }
        if refresh_at.elapsed() >= Duration::from_millis(500) {
            let snapshot = load(cache);
            if snapshot != picker.snapshot {
                picker.update(snapshot);
                dirty = true;
            }
            refresh_at = Instant::now();
        }
    }
}

fn main() {
    let mut args = env::args_os().skip(1);
    let cache = match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None) if flag == "--cache" => PathBuf::from(path),
        _ => {
            eprintln!("Usage: project-picker-tui --cache PATH");
            std::process::exit(2);
        }
    };
    match run(&cache) {
        Ok(Some(id)) => println!("{id}"),
        Ok(None) => {}
        Err(error) => {
            eprintln!("project-picker: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn snapshot_from(cache: Option<&Value>, status: Option<&Value>) -> Snapshot {
        let cache = cache.map(|value| serde_json::from_value(value.clone()).unwrap());
        let status = status.map(|value| serde_json::from_value(value.clone()).unwrap());
        project_picker::cache::view(cache.as_ref(), status.as_ref(), 1_000)
    }

    fn fixture(projects: Value) -> Value {
        json!({"version":1,"active":"personal","contexts":{"personal":{
            "version":1,"complete":true,"account_id":"account-one","workspace_id":"personal",
            "workspace_name":"Personal","started_at":50,"synced_at":100,"projects":projects}}})
    }
    fn project(digit: &str, name: &str) -> Value {
        json!({"id":format!("g-p-{}",digit.repeat(32)),"name":name,
            "url":format!("https://chatgpt.com/g/g-p-{}/project", digit.repeat(32))})
    }
    #[test]
    fn matching_handles_fuzzy_unicode_and_no_results() {
        assert!(score("Competitive Programming", "cpg").is_some());
        assert!(score("Øresund", "ØR").is_some());
        assert!(score("JOB", "missing").is_none());
        assert!(score("JOB", "job").unwrap() < score("My old job", "job").unwrap());
    }
    #[test]
    fn live_updates_remove_and_rename_without_losing_selection() {
        let cache = fixture(json!([project("1", "Alpha"), project("2", "Beta")]));
        let mut picker = Picker::new(snapshot_from(Some(&cache), None));
        picker.selected = 2;
        let cache = fixture(json!([project("2", "Renamed")]));
        picker.update(snapshot_from(Some(&cache), None));
        assert_eq!(picker.current().unwrap().name, "Renamed");
        assert_eq!(picker.snapshot.projects.len(), 2);
        picker.update(snapshot_from(Some(&fixture(json!([]))), None));
        assert_eq!(picker.current().unwrap().id, "home");
    }
    #[test]
    fn no_matches_does_not_select_an_unrelated_project() {
        let mut picker = Picker::new(snapshot_from(None, None));
        picker.query = "not present".into();
        picker.filter(false);
        picker.move_selection(1);
        assert!(picker.current().is_none());
    }
    #[test]
    fn display_is_unicode_aware_and_drops_terminal_controls() {
        assert_eq!(fit("你好 world", 5), "你好…");
        assert_eq!(clean("A\n\x1b[31mB\u{202e}"), "A [31mB");
        assert!(query_tail("你好 world", 5).width() <= 5);
        let mut picker = Picker::new(snapshot_from(None, None));
        for (w, h) in [(1, 1), (20, 7), (64, 18), (100, 40)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal.draw(|frame| draw(frame, &mut picker)).unwrap();
            assert!(!terminal.backend().buffer().content.is_empty());
        }
    }
    #[test]
    fn compact_layout_keeps_search_list_and_footer_separate() {
        let cache = fixture(json!([project("1", "Alpha"), project("2", "Beta")]));
        let mut picker = Picker::new(snapshot_from(Some(&cache), None));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(64, 18)).unwrap();
        terminal.draw(|frame| draw(frame, &mut picker)).unwrap();
        let screen: Vec<String> = terminal
            .backend()
            .buffer()
            .content
            .chunks(64)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        assert!(screen[0].trim().is_empty());
        assert!(screen[1].contains("Projects"));
        assert!(screen[2].contains("Search projects"));
        assert!(screen[4].contains("Open main page"));
        assert!(screen[5].contains("Alpha"));
        assert!(screen[14].trim().is_empty());
        assert!(screen[15].contains("enter open"));
        assert!(screen[16].contains("Last synced"));
        assert!(screen[17].trim().is_empty());
        assert_eq!(terminal.backend().buffer()[(2, 16)].fg, MUTED);
        println!("{}", screen.join("\n"));
    }

    #[test]
    fn failed_sync_retains_the_last_complete_list() {
        let cache = fixture(json!([project("1", "Saved")]));
        let state = snapshot_from(Some(&cache), Some(&json!({"code":"network","at":101})));
        assert_eq!(state.projects.len(), 2);
        assert!(state.notice.as_deref().unwrap().contains("Offline"));
        assert!(state.last_synced.contains("Last synced"));
        let stale_error = snapshot_from(Some(&cache), Some(&json!({"code":"network","at":99})));
        assert!(stale_error.notice.is_none());
        let success = snapshot_from(Some(&cache), Some(&json!({"code":"ok","at":101})));
        assert!(success.notice.is_none());
        assert_eq!(success.last_synced, state.last_synced);
    }
}
