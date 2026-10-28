mod actions;
pub(crate) mod backend;
mod clipboard;
mod data;
mod history;
use crate::hosts;
mod theme;
mod ui;
use anyhow::Result;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use data::Snapshot;
use ratatui::{Terminal, backend::TestBackend, widgets::TableState};
use std::{
    io::{self, IsTerminal, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Parser, Clone, Debug)]
#[command(
    name = "cluster top",
    bin_name = "cluster top",
    disable_help_flag = true
)]
pub struct Args {
    #[arg(long, help = "Query Slurm on this machine")]
    local: bool,
    #[arg(skip = Some(hosts::Target::Local))]
    target: Option<hosts::Target>,
    #[arg(
        short = 'p',
        long,
        default_value = "gpu",
        help = "Partition; use '' for all"
    )]
    partition: String,
    #[arg(long, default_value = "")]
    gpu: String,
    #[arg(
        short = 'u',
        long,
        default_value = "",
        help = "Job owner; defaults to the connected host's user"
    )]
    user: String,
    #[arg(short = 's', long, default_value = "")]
    search: String,
    #[arg(short='r', long, default_value="5", value_parser=positive_seconds)]
    interval: f64,
    #[arg(long, default_value="8", value_parser=positive_seconds)]
    command_timeout: f64,
    #[arg(long, default_value="free", value_parser=["free","gpu","cpu","state","node"])]
    sort: String,
    #[arg(long, default_value="1", value_parser=clap::value_parser!(u8).range(1..=4))]
    tab: u8,
    #[arg(long, help = "Print a snapshot and exit")]
    once: bool,
    #[arg(long, default_value="7", value_parser=clap::value_parser!(u16).range(1..=365))]
    history_days: u16,
}
fn positive_seconds(s: &str) -> std::result::Result<f64, String> {
    let n: f64 = s.parse().map_err(|_| "Expected a number")?;
    if n.is_finite() && (0.1..=86400.0).contains(&n) {
        Ok(n)
    } else {
        Err("Use 0.1–86400 seconds".into())
    }
}
pub struct App {
    args: Args,
    picker: hosts::Picker,
    revision: u64,
    requested_user: String,
    data: Snapshot,
    error: Option<String>,
    loading: bool,
    tab: u8,
    selected: TableState,
    query: String,
    editing: bool,
    filter_open: bool,
    filter_cursors: [usize; 4],
    category_masks: [u8; 4],
    available: bool,
    detail: bool,
    history_detail_scroll: u16,
    help: bool,
    sort: String,
    started: Instant,
    node_view: Vec<usize>,
    job_view: Vec<usize>,
    history: Vec<data::Job>,
    history_loaded: bool,
    history_fetched: Option<Instant>,
    paused: bool,
    notice: Option<String>,
    job_sort: String,
    tab_queries: [String; 4],
    tab_selections: [TableState; 4],
    tab_identities: [Option<String>; 4],
    tab_job_sorts: [String; 4],
    query_before_edit: String,
    history_loading: bool,
    history_error: Option<String>,
    log: Option<history::LogView>,
    log_loading: bool,
    copy_loading: bool,
}
impl App {
    fn new(args: Args) -> Self {
        Self {
            requested_user: args.user.clone(),
            picker: hosts::Picker::default(),
            revision: 0,
            tab: args.tab,
            query: args.search.clone(),
            sort: args.sort.clone(),
            job_sort: if args.tab == 4 {
                "submitted".into()
            } else {
                "priority".into()
            },
            tab_queries: std::array::from_fn(|_| args.search.clone()),
            tab_selections: std::array::from_fn(|_| TableState::default().with_selected(0)),
            tab_identities: std::array::from_fn(|_| None),
            tab_job_sorts: ["priority", "priority", "priority", "submitted"].map(str::to_string),
            query_before_edit: String::new(),
            args,
            data: Snapshot::default(),
            error: None,
            loading: true,
            selected: TableState::default().with_selected(0),
            editing: false,
            filter_open: false,
            filter_cursors: [0; 4],
            category_masks: [0; 4],
            available: false,
            detail: false,
            history_detail_scroll: 0,
            help: false,
            started: Instant::now(),
            node_view: Vec::new(),
            job_view: Vec::new(),
            history: Vec::new(),
            history_loaded: false,
            history_fetched: None,
            paused: false,
            notice: None,
            history_loading: false,
            history_error: None,
            log: None,
            log_loading: false,
            copy_loading: false,
        }
    }
    fn connect(&mut self, target: Option<hosts::Target>) {
        let mut args = self.args.clone();
        args.target = target;
        args.tab = self.tab;
        args.user = self.requested_user.clone();
        let revision = self.revision.wrapping_add(1);
        let picker = std::mem::take(&mut self.picker);
        *self = Self::new(args);
        self.revision = revision;
        self.picker = picker;
        self.picker.open = self.args.target.is_none();
        self.loading = self.args.target.is_some();
    }
    fn jobs(&self) -> &[data::Job] {
        if self.tab == 4 {
            &self.history
        } else {
            &self.data.jobs
        }
    }
    fn node_indices(&self) -> &[usize] {
        &self.node_view
    }
    fn job_indices(&self) -> &[usize] {
        &self.job_view
    }
    // Rebuild only when the snapshot, filter, sort or view changes.
    fn rebuild_views(&mut self) {
        if self.tab == 1 {
            self.node_view = self.compute_node_indices();
        } else {
            self.job_view = self.compute_job_indices();
        }
    }
    fn begin_filter(&mut self) {
        self.filter_open = true;
    }
    fn filter_options(&self) -> &'static [&'static str] {
        if self.tab == 1 {
            &["all", "idle", "mixed", "full", "offline", "unknown"]
        } else if self.tab == 4 {
            &[
                "all",
                "completed",
                "failed",
                "cancelled",
                "timeout",
                "out_of_memory",
                "other",
            ]
        } else {
            &[
                "all",
                "running",
                "pending",
                "completing",
                "configuring",
                "suspended",
                "other",
            ]
        }
    }
    fn category_selected(&self, option: usize) -> bool {
        let mask = self.category_masks[usize::from(self.tab - 1)];
        if option == 0 {
            mask == 0
        } else {
            mask & (1 << option) != 0
        }
    }
    fn category(&self) -> String {
        if self.category_masks[usize::from(self.tab - 1)] == 0 {
            return "all".into();
        }
        self.filter_options()
            .iter()
            .enumerate()
            .filter(|(i, _)| self.category_selected(*i))
            .map(|(_, label)| *label)
            .collect::<Vec<_>>()
            .join(" + ")
    }
    fn toggle_category(&mut self) {
        let tab = usize::from(self.tab - 1);
        let cursor = self.filter_cursors[tab];
        if cursor == 0 {
            self.category_masks[tab] = 0;
        } else {
            self.category_masks[tab] ^= 1 << cursor;
        }
        self.reset_selection();
    }
    fn filter_key(&mut self, key: KeyCode) {
        let index = usize::from(self.tab - 1);
        let count = self.filter_options().len();
        match key {
            KeyCode::Tab => {
                self.filter_cursors[index] = (self.filter_cursors[index] + 1) % count;
            }
            KeyCode::BackTab => {
                self.filter_cursors[index] = (self.filter_cursors[index] + count - 1) % count;
            }
            KeyCode::Down | KeyCode::Right | KeyCode::Char('j') => {
                self.filter_cursors[index] = (self.filter_cursors[index] + 1) % count;
            }
            KeyCode::Up | KeyCode::Left | KeyCode::Char('k') => {
                self.filter_cursors[index] = (self.filter_cursors[index] + count - 1) % count;
            }
            KeyCode::Char(' ') => self.toggle_category(),
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('f' | 'q') => self.filter_open = false,
            KeyCode::Char('/') => {
                self.filter_open = false;
                self.begin_search();
            }
            _ => {}
        }
    }
    fn compute_node_indices(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        let gpu = self.args.gpu.to_lowercase();
        let mut v: Vec<_> = self
            .data
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                (gpu.is_empty() || n.models.keys().any(|m| m.contains(&gpu)))
                    && (self.category_masks[0] == 0
                        || self.category_masks[0]
                            & (1 << match n.category() {
                                "idle" => 1,
                                "mixed" => 2,
                                "full" => 3,
                                "offline" => 4,
                                _ => 5,
                            })
                            != 0)
                    && (!self.available || n.free() > 0)
                    && (q.is_empty()
                        || format!("{} {} {}", n.name, n.state, n.models_label())
                            .to_lowercase()
                            .contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        v.sort_by(|a, b| {
            let (a, b) = (&self.data.nodes[*a], &self.data.nodes[*b]);
            let cmp = match self.sort.as_str() {
                "gpu" => b.total().cmp(&a.total()),
                "cpu" => b.cpu_free().cmp(&a.cpu_free()),
                "state" => a.state.cmp(&b.state),
                "node" => a.name.cmp(&b.name),
                _ => a
                    .unavailable()
                    .cmp(&b.unavailable())
                    .then(b.free().cmp(&a.free())),
            };
            cmp.then(a.name.cmp(&b.name))
        });
        v
    }
    fn compute_job_indices(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        let mut v: Vec<_> = self
            .jobs()
            .iter()
            .enumerate()
            .filter(|(_, j)| {
                (self.tab == 3 || j.user == self.args.user)
                    && (self.args.partition.is_empty()
                        || self
                            .args
                            .partition
                            .split(',')
                            .any(|p| j.partition.split(',').any(|jp| jp == p)))
                    && (self.tab == 1
                        || self.category_masks[usize::from(self.tab - 1)] == 0
                        || self.category_masks[usize::from(self.tab - 1)]
                            & (1 << match (
                                self.tab,
                                j.state.split_whitespace().next().unwrap_or(""),
                            ) {
                                (4, "COMPLETED") => 1,
                                (4, "FAILED") => 2,
                                (4, "CANCELLED") => 3,
                                (4, "TIMEOUT") => 4,
                                (4, "OUT_OF_MEMORY") => 5,
                                (4, _) => 6,
                                (_, "RUNNING") => 1,
                                (_, "PENDING") => 2,
                                (_, "COMPLETING") => 3,
                                (_, "CONFIGURING") => 4,
                                (_, "SUSPENDED") => 5,
                                _ => 6,
                            })
                            != 0)
                    && j.matches_gpu(&self.args.gpu)
                    && (q.is_empty()
                        || format!(
                            "{} {} {} {} {} {} {}",
                            j.id, j.user, j.name, j.state, j.reason, j.gres, j.partition
                        )
                        .to_lowercase()
                        .contains(&q))
            })
            .map(|(i, _)| i)
            .collect();
        v.sort_unstable_by(|&a, &b| {
            let a = &self.jobs()[a];
            let b = &self.jobs()[b];
            let custom = match self.job_sort.as_str() {
                "submitted" => Some(b.submitted.cmp(&a.submitted)),
                "name" => Some(a.name.cmp(&b.name)),
                "state" => Some(a.state.cmp(&b.state)),
                "ended" => Some(b.end.cmp(&a.end)),
                _ if self.tab == 4 => Some(b.submitted.cmp(&a.submitted)),
                _ => None,
            };
            if let Some(cmp) = custom {
                return cmp
                    .then(b.submitted.cmp(&a.submitted))
                    .then_with(|| natural_job_id(&b.id, &a.id));
            }
            (
                !a.running(),
                !a.pending(),
                std::cmp::Reverse(a.priority),
                &a.submitted,
                &a.id,
            )
                .cmp(&(
                    !b.running(),
                    !b.pending(),
                    std::cmp::Reverse(b.priority),
                    &b.submitted,
                    &b.id,
                ))
        });
        v
    }
    fn count(&self) -> usize {
        if self.tab == 1 {
            self.node_indices().len()
        } else {
            self.job_indices().len()
        }
    }
    fn move_by(&mut self, delta: isize) {
        if delta != 0 {
            self.history_detail_scroll = 0;
        }
        let count = self.count();
        self.selected.select(if count == 0 {
            None
        } else {
            Some(
                self.selected
                    .selected()
                    .unwrap_or(0)
                    .saturating_add_signed(delta)
                    .min(count - 1),
            )
        });
    }
    fn reset_selection(&mut self) {
        self.rebuild_views();
        self.selected = TableState::default().with_selected(0);
        self.move_by(0);
    }
    fn identity(&self) -> Option<String> {
        let i = self.selected.selected()?;
        if self.tab == 1 {
            self.node_indices()
                .get(i)
                .map(|&n| self.data.nodes[n].name.clone())
        } else {
            self.job_indices()
                .get(i)
                .map(|&j| self.jobs()[j].id.clone())
        }
    }
    fn replace_data(&mut self, data: Snapshot) {
        if self.tab == 4 {
            self.data = data;
            return;
        }
        let id = self.identity();
        self.data = data;
        self.rebuild_views();
        if let Some(id) = id {
            let position = if self.tab == 1 {
                self.node_indices()
                    .iter()
                    .position(|&n| self.data.nodes[n].name == id)
            } else {
                self.job_indices()
                    .iter()
                    .position(|&j| self.jobs()[j].id == id)
            };
            self.selected.select(position.or(Some(0)));
        }
        self.move_by(0);
    }
}
#[derive(Clone)]
enum WorkerControl {
    Refresh,
    Pause(bool),
    Switch(Box<Args>, u64),
}
type LiveResult = (u64, Result<(Snapshot, String)>);
fn worker(mut args: Args, tx: mpsc::Sender<LiveResult>, rx: mpsc::Receiver<WorkerControl>) {
    let mut paused = false;
    let mut failures = 0u32;
    let mut revision = 0;
    loop {
        if args.target.is_some() {
            let result = backend::snapshot(&args);
            failures = if result.is_err() {
                failures.saturating_add(1)
            } else {
                0
            };
            if tx.send((revision, result)).is_err() {
                return;
            }
        }
        loop {
            let delay = Duration::from_secs_f64(
                (args.interval * 2f64.powi(failures.min(4) as i32)).min(args.interval.max(120.0)),
            );
            let event = if paused || args.target.is_none() {
                rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
            } else {
                rx.recv_timeout(delay)
            };
            match event {
                Ok(WorkerControl::Switch(next, next_revision)) => {
                    args = *next;
                    revision = next_revision;
                    paused = false;
                    failures = 0;
                    break;
                }
                Ok(WorkerControl::Pause(value)) => {
                    paused = value;
                    if !paused {
                        break;
                    }
                }
                Ok(WorkerControl::Refresh) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}
// Accounting job IDs can be numeric or include array components. Numeric IDs
// must sort numerically for jobs submitted within the same timestamp second.
fn natural_job_id(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
}
struct CommandCleanup;
impl Drop for CommandCleanup {
    fn drop(&mut self) {
        data::shutdown_commands();
    }
}
pub fn run_monitor(mut args: Args, host: Option<String>, local: bool) -> Result<()> {
    let _cleanup = CommandCleanup;
    anyhow::ensure!(
        !(host.is_some() && (local || args.local)),
        "Choose either -h HOST or --local."
    );
    let config = hosts::Config::load()?;
    let local_available = hosts::local_available();
    args.target = if let Some(host) = &host {
        hosts::validate(host)?;
        Some(hosts::Target::Ssh(host.clone()))
    } else if local || args.local || local_available {
        Some(hosts::Target::Local)
    } else {
        None
    };
    if args.once || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        let mut app = App::new(args.clone());
        match backend::snapshot(&args) {
            Ok((snapshot, user)) => {
                app.args.user = user;
                app.replace_data(snapshot);
            }
            Err(error) if args.tab == 4 => app.error = Some(format!("{error:#}")),
            Err(error) => return Err(error),
        }
        app.loading = false;
        if args.tab == 4 {
            app.history = backend::history(&app.args)?;
            app.history_fetched = Some(Instant::now());
            app.history_loaded = true;
            app.reset_selection();
        }
        let (w, h) = crossterm::terminal::size().unwrap_or((120, 36));
        let mut terminal = Terminal::new(TestBackend::new(w.max(40), h.max(15)))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let buffer = terminal.backend().buffer();
        let mut output = io::stdout().lock();
        for y in 0..buffer.area.height {
            let line = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Err(error) = writeln!(output, "{}", line.trim_end()) {
                if error.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(error.into());
            }
        }
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    let (refresh_tx, refresh_rx) = mpsc::channel();
    let worker_args = args.clone();
    thread::spawn(move || worker(worker_args, tx, refresh_rx));
    let mut app = App::new(args);
    app.picker = hosts::Picker {
        hosts: config,
        local: local_available,
        open: app.args.target.is_none(),
        ..Default::default()
    };
    app.loading = app.args.target.is_some();
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, rx, refresh_tx);
    data::shutdown_commands();
    ratatui::restore();
    result
}
fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    rx: mpsc::Receiver<LiveResult>,
    refresh: mpsc::Sender<WorkerControl>,
) -> Result<()> {
    let (history_tx, history_rx) = mpsc::channel::<(u64, Result<Vec<data::Job>>)>();
    let (log_tx, log_rx) = mpsc::channel::<(u64, u64, bool, history::LogView)>();
    let mut log_request = 0u64;
    let (copy_tx, copy_rx) = mpsc::channel();
    let mut dirty = true;
    let mut last_draw = Instant::now();
    let mut transfer: Option<clipboard::Transfer> = None;
    let mut pending_clipboard: Option<String> = None;
    let mut clipboard_cancelled = false;
    let mut quit_after_clipboard = false;
    loop {
        if app.args.target.is_some() && app.tab == 4 && !app.history_loaded && !app.history_loading
        {
            app.history_loading = true;
            let args = app.args.clone();
            let tx = history_tx.clone();
            let revision = app.revision;
            thread::spawn(move || {
                let _ = tx.send((revision, backend::history(&args)));
            });
        }
        while let Ok((revision, result)) = history_rx.try_recv() {
            if revision != app.revision {
                continue;
            }
            let id = app.identity();
            app.history_loading = false;
            app.history_loaded = true;
            match result {
                Ok(jobs) => {
                    app.history = jobs;
                    app.history_error = None;
                    app.history_fetched = Some(Instant::now());
                }
                Err(e) => app.history_error = Some(format!("{e:#}")),
            }
            app.rebuild_views();
            if app.tab == 4 {
                app.selected.select(
                    id.and_then(|id| app.job_view.iter().position(|&i| app.history[i].id == id))
                        .or(Some(0)),
                );
                app.move_by(0);
            }
            dirty = true;
        }
        while let Ok((revision, request, reload, mut log)) = log_rx.try_recv() {
            if revision != app.revision || request != log_request {
                continue;
            }
            app.log_loading = false;
            if request == log_request
                && let Some(previous) = &app.log
            {
                if reload {
                    log.restore_position(previous);
                }
                app.log = Some(log);
            }
            dirty = true;
        }
        while let Ok((revision, request, result)) = copy_rx.try_recv() {
            if revision != app.revision || request != log_request {
                continue;
            }
            app.copy_loading = false;
            if request == log_request
                && let Some(log) = &mut app.log
            {
                log.copying = false;
                match result {
                    Ok(text) => {
                        let text: String = text;
                        log.status = format!(
                            "Sending {} bytes to local clipboard… q/Esc cancel · Q quit",
                            text.len()
                        );
                        pending_clipboard = Some(text);
                    }
                    Err(e) => log.status = format!("Yank failed: {e:#}"),
                }
                dirty = true;
            }
        }
        while let Ok((revision, result)) = rx.try_recv() {
            if revision != app.revision {
                continue;
            }
            dirty = true;
            app.loading = false;
            match result {
                Ok((data, user)) => {
                    app.args.user = user;
                    app.replace_data(data);
                    app.error = None
                }
                Err(e) => app.error = Some(format!("{e:#}")),
            }
        }
        if let Some(active) = &mut transfer {
            match active.advance() {
                Ok(true) => {
                    if let Some(log) = &mut app.log {
                        log.status = if clipboard_cancelled {
                            "Yank cancelled".into()
                        } else {
                            format!(
                                "Sent {} bytes to terminal clipboard — paste to verify (OSC 52)",
                                active.bytes()
                            )
                        };
                        log.copying = false;
                        if !clipboard_cancelled {
                            log.anchor = None;
                        }
                    }
                    transfer = None;
                    dirty = true;
                    if quit_after_clipboard {
                        return Ok(());
                    }
                }
                Ok(false) => {}
                Err(e) => {
                    if let Some(log) = &mut app.log {
                        log.status = format!("Yank failed: {e:#}");
                        log.copying = false;
                    }
                    transfer = None;
                    dirty = true;
                    if quit_after_clipboard {
                        return Ok(());
                    }
                }
            }
        }
        // Clipboard payloads are a single terminal control sequence. Do not
        // interleave redraws, while still polling keys and advancing output.
        if transfer.is_none() && (dirty || last_draw.elapsed() >= Duration::from_secs(1)) {
            let size = terminal.size()?;
            app.history_detail_scroll =
                app.history_detail_scroll
                    .min(ui::detail_max_scroll(app, size.width, size.height));
            terminal.draw(|f| ui::draw(f, app))?;
            last_draw = Instant::now();
            dirty = false;
        }
        if let Some(text) = pending_clipboard.take() {
            match clipboard::Transfer::start(text) {
                Ok(active) => {
                    transfer = Some(active);
                    clipboard_cancelled = false;
                    quit_after_clipboard = false;
                }
                Err(e) => {
                    if let Some(log) = &mut app.log {
                        log.status = format!("Yank failed: {e:#}");
                    }
                    dirty = true;
                }
            }
        }
        if !event::poll(Duration::from_millis(if transfer.is_some() {
            10
        } else {
            100
        }))? {
            continue;
        }
        let input = event::read()?;
        if matches!(input, Event::Resize(_, _)) {
            dirty = true;
        }
        if let Event::Key(key) = input {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            dirty = true;
            if let Some(active) = &mut transfer {
                let quit = (key.code == KeyCode::Char('c')
                    && key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL))
                    || key.code == KeyCode::Char('Q');
                if quit || matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    active.cancel();
                    clipboard_cancelled = true;
                    quit_after_clipboard |= quit;
                }
                continue;
            }
            let size = terminal.size()?;
            app.notice = None;
            let effect = app.handle_key(key, size.width, size.height);
            match effect {
                actions::Effect::Connect(target) => {
                    app.connect(target);
                    log_request += 1;
                    pending_clipboard = None;
                    let _ = refresh.send(WorkerControl::Switch(
                        Box::new(app.args.clone()),
                        app.revision,
                    ));
                }
                actions::Effect::Quit => return Ok(()),
                actions::Effect::RefreshLive => {
                    let _ = refresh.send(WorkerControl::Refresh);
                    app.loading = true;
                }
                actions::Effect::Pause(value) => {
                    let _ = refresh.send(WorkerControl::Pause(value));
                }
                actions::Effect::OpenLog(job) => {
                    app.log_loading = true;
                    app.log = Some(history::LogView::loading(&job.id));
                    log_request += 1;
                    let request = log_request;
                    let tx = log_tx.clone();
                    let args = app.args.clone();
                    let revision = app.revision;
                    thread::spawn(move || {
                        let log = backend::read_log(&args, &job).unwrap_or_else(backend::log_error);
                        let _ = tx.send((revision, request, false, log));
                    });
                }
                actions::Effect::ReloadLog => {
                    app.log_loading = true;
                    if let Some(log) = &app.log {
                        let paths = log.paths.clone();
                        let title = log.title.clone();
                        let request = log_request;
                        let tx = log_tx.clone();
                        let args = app.args.clone();
                        let revision = app.revision;
                        thread::spawn(move || {
                            let log = backend::reload(&args, &paths, &title)
                                .unwrap_or_else(backend::log_error);
                            let _ = tx.send((revision, request, true, log));
                        });
                    }
                }
                actions::Effect::Yank => {
                    app.copy_loading = true;
                    if let Some(log) = &mut app.log {
                        let selection = log.selected_text();
                        let paths = log.paths.clone();
                        let tx = copy_tx.clone();
                        let request = log_request;
                        let args = app.args.clone();
                        let revision = app.revision;
                        log.copying = true;
                        log.status = "Preparing yank…".into();
                        thread::spawn(move || {
                            let result = match selection {
                                Some(text) => Ok(text),
                                None => backend::full_log(&args, &paths),
                            };
                            let _ = tx.send((revision, request, result));
                        });
                    }
                }
                actions::Effect::None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cluster_switch_clears_data_logs_history_and_remote_identity() {
        let mut app = App::new(Args::parse_from(["mytop"]));
        app.args.user = "old-remote-user".into();
        app.data.nodes.push(data::Node {
            name: "old-node".into(),
            ..Default::default()
        });
        app.history.push(data::Job {
            id: "same-job-id".into(),
            ..Default::default()
        });
        app.history_loaded = true;
        app.log = Some(history::LogView::loading("same-job-id"));
        app.log_loading = true;
        app.copy_loading = true;
        app.error = Some("old error".into());
        app.connect(Some(hosts::Target::Ssh("next".into())));
        assert_eq!(app.revision, 1);
        assert!(app.args.user.is_empty());
        assert!(app.data.nodes.is_empty());
        assert!(app.history.is_empty());
        assert!(app.log.is_none());
        assert!(!app.history_loaded && !app.log_loading && !app.copy_loading);
        assert!(app.error.is_none());
        assert!(app.loading);
        app.connect(None);
        assert_eq!(app.revision, 2);
        assert!(app.picker.open && !app.loading);
        let mut explicit = App::new(Args::parse_from(["mytop", "--user", "chosen"]));
        explicit.connect(Some(hosts::Target::Ssh("next".into())));
        assert_eq!(explicit.args.user, "chosen");
    }
    fn sample() -> App {
        let mut app = App::new(Args::parse_from(["mytop", "--user", "tester"]));
        app.data.nodes = data::parse_nodes(
            "NodeName=full State=ALLOCATED Partitions=gpu Gres=gpu:a100:4 AllocTRES=gres/gpu=4\nNodeName=free State=MIXED Partitions=gpu Gres=gpu:h100:4 AllocTRES=gres/gpu=1\nNodeName=down State=DOWN Partitions=gpu Gres=gpu:a100:8 AllocTRES=",
            "gpu",
        );
        app.rebuild_views();
        app
    }
    #[test]
    fn filter_sort_and_selection() {
        let mut a = sample();
        assert_eq!(a.identity().as_deref(), Some("free"));
        a.move_by(1);
        assert_eq!(a.identity().as_deref(), Some("full"));
        let mut next = a.data.clone();
        next.nodes.reverse();
        a.replace_data(next);
        assert_eq!(a.identity().as_deref(), Some("full"));
        a.available = true;
        a.reset_selection();
        assert_eq!(a.count(), 1);
        assert_eq!(a.identity().as_deref(), Some("free"));
        a.query = "absent".into();
        a.reset_selection();
        assert_eq!(a.count(), 0);
        assert_eq!(a.selected.selected(), None);
    }
    #[test]
    fn queue_scope_and_search() {
        let mut a = sample();
        a.data.jobs = vec![
            data::Job {
                id: "1".into(),
                user: "tester".into(),
                partition: "gpu".into(),
                state: "RUNNING".into(),
                ..Default::default()
            },
            data::Job {
                id: "2".into(),
                user: "other".into(),
                partition: "gpu".into(),
                ..Default::default()
            },
            data::Job {
                id: "3".into(),
                user: "tester".into(),
                partition: "private".into(),
                ..Default::default()
            },
        ];
        a.tab = 2;
        a.rebuild_views();
        assert_eq!(a.job_indices(), vec![0]);
        a.tab = 3;
        a.rebuild_views();
        assert_eq!(a.count(), 2);
        a.query = "other".into();
        a.rebuild_views();
        assert_eq!(a.job_indices(), vec![1]);
    }
    #[test]
    fn filter_editor_and_cached_navigation() {
        let mut app = sample();
        app.begin_filter();
        assert!(app.filter_open);
        app.filter_key(KeyCode::Char('/'));
        assert!(app.editing);
        app.query = "H100".into();
        app.reset_selection();
        assert_eq!(app.identity().as_deref(), Some("free"));
        let cached = app.node_view.as_ptr();
        for _ in 0..100 {
            app.move_by(1);
            assert_eq!(app.count(), 1);
        }
        assert_eq!(app.node_view.as_ptr(), cached);
        let mut next = app.data.clone();
        next.nodes[1].models.clear();
        app.replace_data(next);
        assert_eq!(app.count(), 0);
        app.query.clear();
        app.reset_selection();
        assert_eq!(app.count(), 3);
    }
    #[test]
    fn job_gpu_filter_text() {
        let mut app = sample();
        app.tab = 3;
        app.data.jobs = vec![data::Job {
            id: "1".into(),
            partition: "gpu".into(),
            gres: "gpu:a100:2".into(),
            ..Default::default()
        }];
        app.query = "A100".into();
        app.reset_selection();
        assert_eq!(app.count(), 1);
        app.query = "h100".into();
        app.reset_selection();
        assert_eq!(app.count(), 0);
    }
    #[test]
    fn fleet_multiselect_separates_movement_from_toggle() {
        let mut app = sample();
        app.begin_filter();
        let cache = app.node_view.as_ptr();
        app.filter_key(KeyCode::Char('k'));
        assert_eq!(app.filter_cursors[0], 5);
        app.filter_key(KeyCode::Char('j'));
        assert_eq!(app.filter_cursors[0], 0);
        app.filter_key(KeyCode::Char('j'));
        app.filter_key(KeyCode::Char('j'));
        assert_eq!(app.category(), "all");
        assert_eq!(app.count(), 3);
        assert_eq!(app.node_view.as_ptr(), cache); // Moving does not rebuild rows.
        app.filter_key(KeyCode::Char(' '));
        app.filter_key(KeyCode::Tab); // Move from mixed to full.
        assert_eq!(app.category(), "mixed");
        assert_eq!(app.filter_cursors[0], 3);
        assert_eq!(app.count(), 1);
        app.filter_key(KeyCode::Char(' '));
        app.filter_key(KeyCode::Tab); // Move from full to offline.
        assert_eq!(app.category(), "mixed + full");
        assert_eq!(app.count(), 2);
        app.filter_key(KeyCode::Char('k')); // Move back without deselecting.
        app.filter_key(KeyCode::Char(' '));
        app.filter_key(KeyCode::BackTab); // Move from full to mixed.
        assert_eq!(app.category(), "mixed");
        assert_eq!(app.filter_cursors[0], 2);
        assert_eq!(app.count(), 1);
        app.filter_key(KeyCode::Char(' '));
        app.filter_key(KeyCode::BackTab); // Move up after deselecting the last category.
        assert_eq!(app.category(), "all");
        assert_eq!(app.count(), 3);
        assert_eq!(app.filter_cursors[0], 1);
        assert_eq!(app.tab, 1);
    }
    #[test]
    fn space_toggles_in_place_and_all_clears() {
        let mut app = sample();
        app.filter_cursors[0] = 3;
        app.filter_key(KeyCode::Char(' '));
        assert_eq!(app.category(), "full");
        assert_eq!(app.filter_cursors[0], 3);
        app.filter_key(KeyCode::Char(' '));
        assert_eq!(app.category(), "all");
        app.filter_key(KeyCode::Char(' '));
        app.filter_cursors[0] = 0;
        app.filter_key(KeyCode::Char(' '));
        assert_eq!(app.category(), "all");
        assert!(app.category_selected(0));
    }
    #[test]
    fn job_multiselect_is_independent_and_combines_with_search() {
        let mut app = sample();
        app.data.jobs = [
            "RUNNING",
            "PENDING",
            "COMPLETING",
            "CONFIGURING",
            "SUSPENDED",
            "FAILED",
        ]
        .iter()
        .enumerate()
        .map(|(i, state)| data::Job {
            id: i.to_string(),
            user: "tester".into(),
            partition: "gpu".into(),
            state: (*state).into(),
            name: "train".into(),
            ..Default::default()
        })
        .collect();
        app.tab = 2;
        app.reset_selection();
        app.begin_filter();
        app.filter_key(KeyCode::Char('j'));
        app.filter_key(KeyCode::Char(' '));
        app.filter_key(KeyCode::Tab);
        app.filter_key(KeyCode::Char(' '));
        assert_eq!(app.category(), "running + pending");
        assert_eq!(app.count(), 2);
        app.filter_key(KeyCode::Enter);
        assert!(!app.filter_open);
        app.tab = 3;
        app.reset_selection();
        assert_eq!(app.category(), "all");
        assert_eq!(app.count(), 6);
        app.tab = 2;
        app.reset_selection();
        assert_eq!(app.category(), "running + pending");
        assert_eq!(app.count(), 2);
        app.query = "absent".into();
        app.reset_selection();
        assert_eq!(app.count(), 0);
        app.query.clear();
        app.reset_selection();
        app.replace_data(app.data.clone());
        assert_eq!(app.category(), "running + pending");
        assert_eq!(app.count(), 2);
        app.tab = 1;
        app.reset_selection();
        assert_eq!(app.category(), "all");
    }
    #[test]
    fn history_order_filter_and_selection() {
        let mut app = sample();
        app.tab = 4;
        app.history = [
            ("10", "2026-10-17", "COMPLETED"),
            ("11", "2026-10-18", "FAILED"),
        ]
        .into_iter()
        .map(|(id, submitted, state)| data::Job {
            id: id.into(),
            submitted: submitted.into(),
            state: state.into(),
            user: "tester".into(),
            partition: "gpu".into(),
            ..Default::default()
        })
        .collect();
        app.reset_selection();
        assert_eq!(app.identity().as_deref(), Some("11"));
        app.move_by(1);
        assert_eq!(app.identity().as_deref(), Some("10"));
        app.replace_data(Snapshot::default());
        assert_eq!(app.identity().as_deref(), Some("10"));
        app.category_masks[3] = 1 << 2;
        app.reset_selection();
        assert_eq!(app.count(), 1);
        assert_eq!(app.identity().as_deref(), Some("11"));
    }
    #[test]
    fn cli_rejects_bad_intervals() {
        for n in ["NaN", "inf", "0", "-1"] {
            assert!(positive_seconds(n).is_err());
        }
    }
}
