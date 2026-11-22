use crate::top::{
    App,
    data::{Job, Node},
    theme::{
        ACCENT, BG, BLUE, DIM, EDGE, FG, GREEN, PANEL, RED, focus_style, muted, selection_style,
    },
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};
pub(crate) fn style(c: Color) -> Style {
    if c == DIM {
        muted()
    } else {
        Style::default().fg(c)
    }
}
fn bold(c: Color) -> Style {
    style(c).add_modifier(Modifier::BOLD)
}
fn label(s: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(s.into(), muted()))
}
fn inset(r: Rect, x: u16, y: u16) -> Rect {
    Rect::new(
        r.x + x.min(r.width),
        r.y + y.min(r.height),
        r.width.saturating_sub(x * 2),
        r.height.saturating_sub(y * 2),
    )
}
fn gb(m: u64) -> String {
    if m >= 1024 * 1024 {
        format!("{:.1}T", m as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0}G", m as f64 / 1024.0)
    }
}
/// Screen regions from the last frame, used to route mouse clicks.
#[derive(Default)]
pub struct Hits {
    pub tabs: Vec<(Rect, u8)>,
    pub rows: Option<RowHits>,
    pub detail: Option<Rect>,
    pub picker: Option<(Rect, Rect, usize)>,
    pub filter: Option<(Rect, Rect)>,
}
/// Visible table rows: `area` covers rendered rows only, starting at index `start`.
#[derive(Clone, Copy)]
pub struct RowHits {
    pub area: Rect,
    pub start: usize,
}
impl RowHits {
    pub fn new(table: Rect, header: u16, start: usize, visible: usize) -> Self {
        let y = table.y + header.min(table.height);
        let height = (visible as u16).min(table.bottom().saturating_sub(y));
        Self {
            area: Rect::new(table.x, y, table.width, height),
            start,
        }
    }
}
/// Slurm placeholders for "no value" rendered as a dash.
pub(crate) fn present(s: &str) -> &str {
    match s.trim() {
        "" | "Unknown" | "None" | "N/A" | "(null)" => "—",
        _ => s,
    }
}
/// Full cluster-local timestamp for details: `2026-11-22T15:28:10` → `2026-11-22 15:28:10`.
pub(crate) fn full_time(s: &str) -> String {
    present(s).replacen('T', " ", 1)
}
/// Compact cluster-local timestamp for tables: `2026-11-22T15:28:10` → `11-22 15:28`.
pub(crate) fn short_time(s: &str) -> String {
    let s = present(s);
    let is_date = |d: &str| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-';
    match s.split_once('T') {
        Some((date, time)) if is_date(date) && time.len() >= 5 => {
            format!("{} {}", &date[5..], &time[..5])
        }
        None if is_date(s) => s[5..].to_string(),
        _ => s.to_string(),
    }
}
/// Elapsed time, dashed when the job has not run yet (`00:00:00` or `0:00`).
pub(crate) fn elapsed_label(elapsed: &str) -> String {
    let elapsed = present(elapsed);
    if elapsed.chars().any(|c| c.is_ascii_digit())
        && elapsed.chars().all(|c| matches!(c, '0' | ':' | '-'))
    {
        "—".into()
    } else {
        elapsed.into()
    }
}
/// Exit code worth surfacing: anything but a clean `0:0`, including signal-only exits like `0:9`.
pub(crate) fn notable_exit(exit_code: &str) -> Option<&str> {
    match present(exit_code) {
        "—" | "0:0" | "0" => None,
        code => Some(code),
    }
}
/// Glyph, short label and color for a Slurm job state; the glyph keeps it readable without color.
pub(crate) fn job_state(state: &str) -> (String, Color) {
    let base = state.split_whitespace().next().unwrap_or("");
    let (glyph, label, color) = match base {
        "RUNNING" => ("●", "running", GREEN),
        "PENDING" => ("◌", "pending", ACCENT),
        "COMPLETING" => ("◍", "completing", GREEN),
        "CONFIGURING" => ("◌", "configuring", ACCENT),
        "REQUEUED" | "RESIZING" => ("◌", "requeued", ACCENT),
        "SUSPENDED" | "PREEMPTED" | "STOPPED" => ("Ⅱ", "suspended", ACCENT),
        "COMPLETED" => ("✓", "completed", DIM),
        "CANCELLED" => ("–", "cancelled", DIM),
        "FAILED" => ("✗", "failed", RED),
        "TIMEOUT" => ("✗", "timeout", RED),
        "OUT_OF_MEMORY" => ("✗", "oom", RED),
        "NODE_FAIL" => ("✗", "node_fail", RED),
        "BOOT_FAIL" => ("✗", "boot_fail", RED),
        "DEADLINE" => ("✗", "deadline", RED),
        "" => ("·", "unknown", DIM),
        _ => return (format!("· {}", base.to_lowercase()), DIM),
    };
    (format!("{glyph} {label}"), color)
}
/// GPUs from a GRES/TRES string: `2×A100`, `1 GPU`, or a dash.
pub(crate) fn gpu_label(gres: &str) -> String {
    let counts = crate::top::data::gpu_counts(gres);
    if counts.is_empty() {
        return "—".into();
    }
    counts
        .iter()
        .map(|(model, n)| {
            if model == "gpu" {
                format!("{n} GPU")
            } else {
                format!("{n}×{}", model.to_uppercase())
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}
fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        muted().add_modifier(Modifier::BOLD),
    ))
}
fn kv(key: &str, value: impl Into<String>) -> Line<'static> {
    kv_styled(key, value, style(FG))
}
fn kv_styled(key: &str, value: impl Into<String>, value_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<10}"), muted()),
        Span::styled(value.into(), value_style),
    ])
}
/// Key hints in two tones: the key stands out, its action recedes.
fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, action)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", muted()));
        }
        spans.push(Span::styled(key.to_string(), bold(ACCENT)));
        spans.push(Span::styled(format!(" {action}"), muted()));
    }
    Line::from(spans)
}
/// Rows above the main panes (header + metrics + toolbar) plus the footer.
fn chrome_rows(height: u16) -> (u16, u16, u16, u16) {
    let compact = height < 20;
    (if compact { 1 } else { 2 }, 3, 2, 3)
}
fn meter(free: u64, total: u64, width: usize) -> Line<'static> {
    let n = if total == 0 {
        0
    } else {
        ((free as f64 / total as f64) * width as f64).round() as usize
    }
    .min(width);
    let color = match (free, total) {
        (_, 0) => DIM,
        (f, t) if f * 4 >= t => GREEN,
        (0, _) => RED,
        _ => ACCENT,
    };
    Line::from(vec![
        Span::styled("━".repeat(n), style(color)),
        Span::styled("─".repeat(width - n), style(EDGE)),
    ])
}
fn plural(n: impl Into<u64>, word: &str) -> String {
    let n = n.into();
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}
/// Untyped GRES (`gpu:4`) only says "gpu"; showing that as a model adds nothing.
fn model_label(n: &Node) -> String {
    if n.models.is_empty() || n.models.keys().all(|m| m == "gpu") {
        "—".into()
    } else {
        n.models_label()
    }
}
fn state(n: &Node) -> (&str, Color) {
    let category = n.category();
    (
        category,
        match category {
            "offline" => RED,
            "idle" => GREEN,
            "mixed" => ACCENT,
            _ => DIM,
        },
    )
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits = Hits::default();
    let area = f.area();
    f.render_widget(Block::default().style(style(FG).bg(BG)), area);
    if area.width < 48 || area.height < 14 {
        f.render_widget(
            Paragraph::new(
                "cluster top\nResize to at least 48 × 14.\nq quit · Q / Ctrl-C quit anywhere",
            )
            .style(style(ACCENT)),
            area,
        );
        return;
    }
    if let Some(log) = &app.log {
        crate::top::history::draw_log(f, area, log);
        if app.help {
            help(f, area, app);
        }
        if app.picker.open {
            app.hits.picker = Some(cluster_picker(f, area, app));
        }
        return;
    }
    let a = inset(area, 2, 0);
    let (header_rows, metric_rows, toolbar_rows, footer_rows) = chrome_rows(area.height);
    let parts = Layout::vertical([
        Constraint::Length(header_rows),
        Constraint::Length(metric_rows),
        Constraint::Length(toolbar_rows),
        Constraint::Min(1),
        Constraint::Length(footer_rows),
    ])
    .split(a);
    header(f, parts[0], app);
    metrics(f, parts[1], app);
    app.hits.tabs = toolbar(f, parts[2], app);
    let wide = area.width >= 112;
    let panes = if wide {
        Layout::horizontal([
            Constraint::Min(55),
            Constraint::Length(if app.tab == 4 { 42 } else { 33 }),
        ])
        .spacing(2)
        .split(parts[3])
    } else {
        Layout::horizontal([Constraint::Percentage(100)]).split(parts[3])
    };
    app.hits.detail = if app.detail {
        Some(parts[3])
    } else {
        wide.then(|| panes[1])
    };
    if app.tab == 4 {
        if app.detail {
            history_inspector(f, parts[3], app);
        } else {
            // Nothing can be selected without accounting, so the list takes the full width.
            let full = app.history_unsupported && app.history.is_empty();
            crate::top::history::draw(f, if full { parts[3] } else { panes[0] }, app);
            if wide && !full {
                history_inspector(f, panes[1], app);
            }
        }
    } else if app.detail {
        inspector(f, parts[3], app)
    } else {
        if app.tab == 1 {
            nodes(f, panes[0], app)
        } else {
            jobs(f, panes[0], app)
        }
        if wide {
            inspector(f, panes[1], app)
        }
    }
    footer(f, parts[4], app);
    if app.filter_open {
        app.hits.filter = Some(filter_panel(f, area, app));
    }
    if app.help {
        help(f, area, app)
    }
    if app.picker.open {
        app.hits.picker = Some(cluster_picker(f, area, app));
    }
}
fn cluster_picker(f: &mut Frame, area: Rect, app: &App) -> (Rect, Rect, usize) {
    let choices = app.picker.choices();
    let height = (choices.len() as u16 + 9)
        .max(11)
        .min(area.height.saturating_sub(2));
    let width = 76.min(area.width.saturating_sub(4));
    let panel = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, panel);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style(ACCENT))
        .style(style(FG).bg(BG))
        .title(" Clusters ");
    let inner = block.inner(panel);
    f.render_widget(block, panel);
    let list_height = inner.height.saturating_sub(5) as usize;
    let start = app
        .picker
        .selected
        .saturating_sub(list_height.saturating_sub(1));
    let mut lines = Vec::new();
    if choices.is_empty() {
        lines.push(label("No saved clusters. Press a to add an SSH alias."));
    } else {
        for (i, target) in choices.iter().enumerate().skip(start).take(list_height) {
            let selected = i == app.picker.selected;
            let active = Some(target) == app.args.target.as_ref();
            lines.push(Line::styled(
                format!(
                    " {} {}{}",
                    if selected { "›" } else { " " },
                    target.label(),
                    if active { "  ● current" } else { "" }
                ),
                if selected { focus_style() } else { style(FG) },
            ));
        }
    }
    let list = Rect::new(inner.x, inner.y, inner.width, list_height as u16);
    f.render_widget(Paragraph::new(lines), list);
    let mut footer = vec![label(
        "↑/↓ or click · Enter/double-click connect · a add · d remove",
    )];
    if let Some(input) = &app.picker.input {
        footer.push(Line::styled(format!("SSH alias: {input}▏"), style(ACCENT)));
        footer.push(label("Enter saves · Esc cancels"));
    } else {
        footer.push(label("SSH settings and keys come from ~/.ssh/config."));
        footer.push(label(if app.args.target.is_some() {
            "Esc back · Ctrl-C quit"
        } else {
            "q quit · Ctrl-C quit"
        }));
    }
    if let Some(error) = &app.picker.error {
        footer.push(Line::styled(error, style(RED)));
    }
    f.render_widget(
        Paragraph::new(footer).wrap(Wrap { trim: false }),
        Rect::new(inner.x, inner.bottom().saturating_sub(5), inner.width, 5),
    );
    (panel, list, start)
}
fn header(f: &mut Frame, a: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled(
            " cluster top ",
            bold(ACCENT).add_modifier(Modifier::REVERSED),
        ),
        Span::styled(
            format!(
                "  {}",
                app.args
                    .target
                    .as_ref()
                    .map_or("choose cluster", |target| target.label())
            ),
            bold(FG),
        ),
        Span::styled("  ›  ", style(EDGE)),
        Span::styled(
            if app.args.partition.is_empty() {
                "all partitions".to_string()
            } else {
                format!("partition {}", app.args.partition)
            },
            muted(),
        ),
    ]);
    let y = a.bottom().saturating_sub(1);
    f.render_widget(Paragraph::new(line), Rect::new(a.x, y, a.width, 1));
    if a.width >= 75 {
        let (error, loading) = if app.tab == 4 {
            (
                app.history_error.is_some() && !app.history_unsupported,
                app.history_loading,
            )
        } else {
            (app.error.is_some(), app.loading)
        };
        let unsupported = app.tab == 4 && app.history_unsupported;
        let badge = if app.args.target.is_none() {
            "○ SELECT CLUSTER"
        } else if unsupported {
            "○ NO ACCOUNTING"
        } else if error {
            "● STALE"
        } else if loading {
            "◌ SYNCING"
        } else if app.paused && app.tab != 4 {
            "Ⅱ PAUSED"
        } else if app.tab == 4 {
            "● HISTORY"
        } else {
            "● LIVE"
        };
        f.render_widget(
            Paragraph::new(badge)
                .style(style(if error {
                    RED
                } else if unsupported {
                    DIM
                } else if app.paused && app.tab != 4 {
                    ACCENT
                } else {
                    GREEN
                }))
                .alignment(ratatui::layout::Alignment::Right),
            Rect::new(a.right() - 16, y, 16, 1),
        );
    }
}
fn metrics(f: &mut Frame, a: Rect, app: &App) {
    let gpu_filter = app.args.gpu.to_lowercase();
    let (free, total, ready, node_count) = app
        .data
        .nodes
        .iter()
        .filter(|n| gpu_filter.is_empty() || n.models.keys().any(|m| m.contains(&gpu_filter)))
        .fold((0, 0, 0, 0), |(free, total, ready, count), n| {
            (
                free + n.free(),
                total + n.total(),
                ready + usize::from(!n.unavailable()),
                count + 1,
            )
        });
    let (running, pending) = app
        .data
        .jobs
        .iter()
        .filter(|j| j.user == app.args.user && app.partition_matches(j))
        .fold((0, 0), |(running, pending), j| {
            (
                running + usize::from(j.running()),
                pending + usize::from(j.pending()),
            )
        });
    let cols = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(a);
    let specs = [
        (
            free.to_string(),
            format!(" / {}", plural(total, "GPU")),
            "FREE GPUS",
            GREEN,
            Some((u64::from(free), u64::from(total))),
        ),
        (
            ready.to_string(),
            format!(" / {}", plural(node_count as u64, "node")),
            "USABLE NODES",
            BLUE,
            Some((ready as u64, node_count as u64)),
        ),
        (
            running.to_string(),
            format!(" running · {pending} waiting"),
            "YOUR JOBS",
            ACCENT,
            None,
        ),
    ];
    for (r, (value, detail, title, color, ratio)) in cols.iter().zip(specs) {
        let mut caption = vec![Span::styled(format!("{title}  "), muted())];
        if let Some((part, whole)) = ratio {
            let width = usize::from(r.width.saturating_sub(title.len() as u16 + 4)).min(24);
            caption.extend(meter(part, whole, width).spans);
        }
        let lines = vec![
            Line::from(vec![
                Span::styled(value, bold(color)),
                Span::styled(detail, style(FG)),
            ]),
            Line::from(caption),
        ];
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(style(EDGE)),
            ),
            *r,
        );
    }
}
fn toolbar(f: &mut Frame, a: Rect, app: &App) -> Vec<(Rect, u8)> {
    let mut spans = vec![];
    let mut tabs = vec![];
    let mut x = a.x;
    for (n, title) in [(1, "Fleet"), (2, "My jobs"), (3, "Queue"), (4, "History")] {
        let text = format!(" {n} {title} ");
        let width = text.chars().count() as u16;
        tabs.push((
            Rect::new(x, a.y, width.min(a.right().saturating_sub(x)), 1),
            n,
        ));
        x = x.saturating_add(width + 1);
        spans.push(Span::styled(
            text,
            if n == app.tab {
                bold(ACCENT).add_modifier(Modifier::REVERSED)
            } else {
                muted()
            },
        ));
        spans.push(Span::raw(" "));
    }
    spans.push(Span::raw(" "));
    let window = format!("{}d", app.args.history_days);
    let (long, short) = if app.tab == 1 {
        (
            format!(
                "{} · sort: {}{}",
                plural(app.count() as u64, "node"),
                app.sort,
                if app.available { " · free only" } else { "" }
            ),
            app.count().to_string(),
        )
    } else if app.tab == 4 && app.history_error.is_some() && app.history.is_empty() {
        (format!("{window} · sort: {}", app.job_sort), window.clone())
    } else if app.tab == 4 {
        (
            format!(
                "{} · {window} · sort: {}",
                plural(app.count() as u64, "job"),
                app.job_sort
            ),
            format!("{} · {window}", app.count()),
        )
    } else {
        (
            format!(
                "{} · sort: {}",
                plural(app.count() as u64, "job"),
                app.job_sort
            ),
            app.count().to_string(),
        )
    };
    let room = usize::from(a.right().saturating_sub(x));
    if let Some(summary) = [long, short].into_iter().find(|s| s.chars().count() < room) {
        spans.push(Span::styled(summary, muted()));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect::new(a.x, a.y, a.width, 1),
    );
    if !app.query.is_empty()
        || app.editing
        || app.category() != "all"
        || app.available && app.tab == 1
    {
        f.render_widget(
            Paragraph::new(format!(
                " Filter: {}{}{}{}",
                app.category(),
                if app.available && app.tab == 1 {
                    " · free GPUs"
                } else {
                    ""
                },
                if app.query.is_empty() && !app.editing {
                    String::new()
                } else {
                    format!(" · / {}", app.query)
                },
                if app.editing { "▏" } else { "" }
            ))
            .style(style(ACCENT)),
            Rect::new(a.x, a.y + 1, a.width, 1),
        );
    }
    tabs
}
// Materialize only visible rows, retaining a global selection and scroll offset.
pub(crate) fn viewport(
    state: &mut TableState,
    count: usize,
    height: u16,
) -> (usize, usize, TableState) {
    let capacity = usize::from(height.saturating_sub(2)).max(1);
    let selected = state.selected().unwrap_or(0).min(count.saturating_sub(1));
    let mut start = state.offset().min(count.saturating_sub(capacity));
    if selected < start {
        start = selected;
    }
    if selected >= start + capacity {
        start = selected + 1 - capacity;
    }
    *state.offset_mut() = start;
    (
        start,
        capacity,
        TableState::default().with_selected(selected - start),
    )
}
fn nodes(f: &mut Frame, a: Rect, app: &mut App) {
    let ids = &app.node_view;
    if ids.is_empty() {
        empty(
            f,
            a,
            app,
            "No matching nodes",
            "Press x to clear filters, or a to toggle availability.",
        );
        return;
    }
    let wide = a.width >= 76;
    let widths = if wide {
        vec![
            Constraint::Min(16),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(14),
            Constraint::Length(9),
            Constraint::Length(11),
        ]
    } else {
        vec![
            Constraint::Min(13),
            Constraint::Length(7),
            Constraint::Length(10),
            Constraint::Length(8),
        ]
    };
    let headers = if wide {
        vec!["NODE", "STATE", "MODEL", "GPU FREE", "CPU FREE", "RAM FREE"]
    } else {
        vec!["NODE", "STATE", "MODEL", "GPU FREE"]
    };
    let (start, capacity, mut visible_selection) = viewport(&mut app.selected, ids.len(), a.height);
    app.hits.rows = Some(RowHits::new(a, 2, start, capacity.min(ids.len() - start)));
    let rows = ids.iter().skip(start).take(capacity).map(|&i| {
        let n = &app.data.nodes[i];
        let (status, color) = state(n);
        let gpu = if n.gpu_known {
            format!("{}/{}", n.free(), n.total())
        } else {
            format!("?/{}", n.total())
        };
        let mut gpu_line = Line::from(Span::styled(
            format!("{gpu:>4} "),
            style(if n.free() > 0 { GREEN } else { DIM }),
        ));
        if wide && n.gpu_known {
            gpu_line
                .spans
                .extend(meter(n.free().into(), n.total().into(), 7).spans);
        }
        let mut cells = vec![
            Cell::from(n.name.clone()).style(style(FG)),
            Cell::from(status).style(style(color)),
            Cell::from(model_label(n)).style(muted()),
            Cell::from(gpu_line),
        ];
        if wide {
            cells.push(Cell::from(format!("{}/{}", n.cpu_free(), n.cpu_total)).style(muted()));
            cells.push(
                Cell::from(format!(
                    "{}/{}",
                    gb(n.ram_total.saturating_sub(n.ram_used)),
                    gb(n.ram_total)
                ))
                .style(muted()),
            );
        }
        Row::new(cells)
    });
    let table = Table::new(rows, widths)
        .header(
            Row::new(headers)
                .style(muted().add_modifier(Modifier::BOLD))
                .height(2),
        )
        .column_spacing(1)
        .row_highlight_style(selection_style())
        .highlight_symbol("▌ ");
    f.render_stateful_widget(table, a, &mut visible_selection);
}
fn jobs(f: &mut Frame, a: Rect, app: &mut App) {
    let ids = &app.job_view;
    if ids.is_empty() {
        empty(
            f,
            a,
            app,
            "Nothing in this view",
            "No jobs match the selected user, partition and filters.",
        );
        return;
    }
    let wide = a.width >= 85;
    // Every row on My jobs is the current user's, so the owner column only earns space on Queue.
    let show_user = app.tab == 3 && a.width >= 96;
    let mut widths = vec![Constraint::Length(if wide { 12 } else { 10 })];
    let mut headers = vec!["JOB"];
    if show_user {
        widths.push(Constraint::Length(10));
        headers.push("USER");
    }
    widths.extend([
        Constraint::Min(10),
        Constraint::Length(if wide { 13 } else { 11 }),
    ]);
    headers.extend(["NAME", "STATE"]);
    if wide {
        // squeue %b reports the per-node request, not the job-wide total.
        widths.extend([Constraint::Length(9), Constraint::Length(10)]);
        headers.extend(["GPU/NODE", "ELAPSED"]);
    }
    widths.push(Constraint::Length(if wide { 16 } else { 12 }));
    headers.push("NODE / REASON");
    let (start, capacity, mut visible_selection) = viewport(&mut app.selected, ids.len(), a.height);
    app.hits.rows = Some(RowHits::new(a, 2, start, capacity.min(ids.len() - start)));
    let rows = ids.iter().skip(start).take(capacity).map(|&i| {
        let j = &app.jobs()[i];
        let (status, color) = job_state(&j.state);
        let mut cells = vec![Cell::from(j.id.clone()).style(style(FG))];
        if show_user {
            cells.push(Cell::from(j.user.clone()).style(muted()));
        }
        cells.push(Cell::from(j.name.clone()));
        cells.push(Cell::from(status).style(style(color)));
        if wide {
            cells.push(Cell::from(gpu_label(&j.gres)).style(style(FG)));
            cells.push(Cell::from(elapsed_label(&j.elapsed)).style(muted()));
        }
        cells.push(
            Cell::from(if j.running() {
                j.nodes.clone()
            } else {
                j.reason.clone()
            })
            .style(muted()),
        );
        Row::new(cells)
    });
    f.render_stateful_widget(
        Table::new(rows, widths)
            .header(
                Row::new(headers)
                    .style(muted().add_modifier(Modifier::BOLD))
                    .height(2),
            )
            .column_spacing(1)
            .row_highlight_style(selection_style())
            .highlight_symbol("▌ "),
        a,
        &mut visible_selection,
    );
}
fn empty(f: &mut Frame, a: Rect, app: &App, title: &str, detail: &str) {
    let text = if app.data.fetched.is_none() && app.loading {
        vec![
            Line::from(Span::styled("Connecting to Slurm…", bold(ACCENT))),
            label("You can navigate or quit while the controller responds."),
        ]
    } else if app.data.fetched.is_none() && app.error.is_some() {
        vec![
            Line::from(Span::styled("Slurm unavailable", bold(RED))),
            label("Press r to retry."),
        ]
    } else {
        vec![
            Line::from(Span::styled(title.to_string(), bold(FG))),
            Line::default(),
            label(detail),
        ]
    };
    f.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }),
        inset(a, 2, 2),
    );
}
fn node_lines(n: &Node) -> Vec<Line<'static>> {
    let (status, color) = state(n);
    let mut lines = vec![
        Line::from(Span::styled(n.name.clone(), bold(ACCENT))),
        Line::from(vec![
            Span::styled(format!("● {status}"), style(color)),
            Span::styled(
                if n.state.eq_ignore_ascii_case(status) {
                    String::new()
                } else {
                    format!("  {}", n.state.to_lowercase())
                },
                muted(),
            ),
        ]),
        Line::default(),
        section("PARTITIONS"),
        Line::from(n.partitions.join(" · ")),
        Line::default(),
        section("GPUS"),
    ];
    if model_label(n) == "—" {
        lines.push(Line::from(format!("{} × GPU", n.total())));
        lines.push(label("Model not reported by Slurm (untyped GRES)"));
    } else {
        for (model, count) in &n.models {
            lines.push(Line::from(format!("{count} × {}", model.to_uppercase())));
        }
    }
    let ram_free = n.ram_total.saturating_sub(n.ram_used);
    let resource = |name: &str, value: String, free: u64, total: u64| {
        let mut line = Line::from(vec![
            Span::styled(format!("{name:<4}"), muted()),
            Span::styled(format!("{value:<12}"), style(FG)),
        ]);
        line.spans.extend(meter(free, total, 12).spans);
        line
    };
    lines.extend([
        Line::default(),
        section("FREE (UNALLOCATED)"),
        resource(
            "GPU",
            if n.gpu_known {
                format!("{}/{}", n.free(), n.total())
            } else {
                format!("?/{}", n.total())
            },
            n.free().into(),
            n.total().into(),
        ),
        resource(
            "CPU",
            format!("{}/{}", n.cpu_free(), n.cpu_total),
            n.cpu_free().into(),
            n.cpu_total.into(),
        ),
        resource(
            "RAM",
            format!("{}/{}", gb(ram_free), gb(n.ram_total)),
            ram_free,
            n.ram_total,
        ),
        Line::default(),
        section("IN USE"),
        Line::from(format!(
            "{} · {} · {} RAM",
            plural(n.gpu_used, "GPU"),
            plural(n.cpu_used, "CPU"),
            gb(n.ram_used)
        )),
        Line::default(),
    ]);
    if !n.reason.is_empty() && n.reason != "(null)" {
        lines.extend([
            section("NODE NOTE"),
            Line::from(n.reason.clone()),
            Line::default(),
        ]);
    }
    lines.push(label(
        "Whether a job fits also depends on reservations and scheduling policy.",
    ));
    lines
}
fn title_lines(j: &Job) -> Vec<Line<'static>> {
    let (status, color) = job_state(&j.state);
    let mut state_line = vec![Span::styled(status, bold(color))];
    if let Some(code) = notable_exit(&j.exit_code) {
        state_line.push(Span::styled(format!("  exit {code}"), style(RED)));
    }
    vec![
        Line::from(Span::styled(
            format!("#{} · {}", j.id, j.name),
            bold(ACCENT),
        )),
        Line::from(state_line),
    ]
}
fn job_lines(j: &Job, app: &App) -> Vec<Line<'static>> {
    let mut lines = title_lines(j);
    if j.pending() && present(&j.reason) != "—" {
        lines.push(kv_styled("Waiting", j.reason.clone(), style(ACCENT)));
    }
    lines.extend([
        Line::default(),
        section("ALLOCATION"),
        kv("GPU/node", gpu_label(&j.gres)),
        kv("Nodes", present(&j.nodes)),
        kv("Partition", j.partition.clone()),
        kv("Owner", j.user.clone()),
        Line::default(),
        section("TIMING"),
        kv(
            "Runtime",
            format!("{} / {}", elapsed_label(&j.elapsed), present(&j.limit)),
        ),
        kv("Submitted", full_time(&j.submitted)),
    ]);
    if j.pending() {
        let ahead = app
            .data
            .jobs
            .iter()
            .filter(|o| {
                o.pending()
                    && o.id != j.id
                    && o.partition == j.partition
                    && (o.priority > j.priority
                        || (o.priority == j.priority && o.submitted < j.submitted))
            })
            .count();
        lines.extend([
            Line::default(),
            section("QUEUE"),
            kv("Ahead", plural(ahead as u64, "job")),
            label("Same partition, higher or earlier priority. Not a start estimate."),
        ]);
    }
    lines
}
fn selected_detail_lines(app: &App) -> Vec<Line<'static>> {
    let selected = app.selected.selected().unwrap_or(0);
    if app.tab == 1 {
        app.node_view
            .get(selected)
            .map(|&i| node_lines(&app.data.nodes[i]))
            .unwrap_or_default()
    } else {
        app.job_view
            .get(selected)
            .map(|&i| {
                if app.tab == 4 {
                    history_lines(&app.history[i], app)
                } else {
                    job_lines(&app.jobs()[i], app)
                }
            })
            .unwrap_or_default()
    }
}
fn inspector(f: &mut Frame, area: Rect, app: &App) {
    let title = if app.tab == 1 {
        " NODE DETAILS "
    } else {
        " JOB DETAILS "
    };
    draw_inspector(
        f,
        area,
        title,
        selected_detail_lines(app),
        app.history_detail_scroll,
    );
}
fn draw_inspector(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>, scroll: u16) {
    let mut block = Block::default()
        .borders(Borders::LEFT | Borders::TOP)
        .title(Span::styled(
            title.to_string(),
            muted().add_modifier(Modifier::BOLD),
        ))
        .border_style(style(EDGE))
        .style(style(FG).bg(PANEL));
    let inner = inset(block.inner(area), 1, 0);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    if paragraph.line_count(inner.width) > usize::from(inner.height) {
        block = block.title_top(Line::styled(" ↕ J/K ", style(ACCENT)).right_aligned());
    }
    f.render_widget(block, area);
    f.render_widget(paragraph.scroll((scroll, 0)), inner);
}
/// Maximum detail scroll for the full terminal dimensions, matching the main layout.
pub fn detail_max_scroll(app: &App, width: u16, height: u16) -> u16 {
    if width < 48 || height < 14 || (!app.detail && width < 112) {
        return 0;
    }
    let pane_width = if app.detail {
        width.saturating_sub(4)
    } else if app.tab == 4 {
        42
    } else {
        33
    };
    let content_width = pane_width.saturating_sub(3);
    let (header, metrics, toolbar, footer) = chrome_rows(height);
    let content_height = height
        .saturating_sub(header + metrics + toolbar + footer)
        .saturating_sub(1);
    Paragraph::new(selected_detail_lines(app))
        .wrap(Wrap { trim: false })
        .line_count(content_width)
        .saturating_sub(usize::from(content_height))
        .min(usize::from(u16::MAX)) as u16
}
fn history_lines(j: &Job, app: &App) -> Vec<Line<'static>> {
    let gpus = crate::top::data::gpu_counts(&j.gres);
    let gpu = if gpus.is_empty() {
        "No GPU allocation recorded".into()
    } else {
        gpus.iter()
            .map(|(model, count)| {
                if model == "gpu" {
                    format!("{count} GPU (model not recorded)")
                } else {
                    format!("{count} × {}", model.to_uppercase())
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = title_lines(j);
    if j.pending() && present(&j.reason) != "—" {
        lines.push(kv_styled("Waiting", j.reason.clone(), style(ACCENT)));
    }
    lines.extend([
        Line::default(),
        section("ALLOCATION"),
        kv("GPUs", gpu),
        kv("Node", present(&j.nodes)),
    ]);
    if gpus.contains_key("gpu")
        && let Some(node) = app.data.nodes.iter().find(|n| n.name == j.nodes)
    {
        lines.push(kv_styled(
            "Node now",
            format!("{} (current, not historical)", node.models_label()),
            muted(),
        ));
    }
    let mem = j
        .gres
        .split(',')
        .find_map(|t| t.strip_prefix("mem="))
        .unwrap_or("—");
    let requested_mem = match j.req_mem.strip_suffix('c') {
        Some(value) => format!("{value}/CPU"),
        None => match j.req_mem.strip_suffix('n') {
            Some(value) => format!("{value}/node"),
            None => present(&j.req_mem).to_string(),
        },
    };
    lines.extend([
        kv("CPUs", present(&j.cpus)),
        kv(
            "Memory",
            if requested_mem == "—" {
                mem.to_string()
            } else {
                format!("{mem} · requested {requested_mem}")
            },
        ),
        Line::default(),
        section("TIMING"),
        kv(
            "Runtime",
            format!("{} / {}", elapsed_label(&j.elapsed), present(&j.limit)),
        ),
        kv("Submitted", full_time(&j.submitted)),
    ]);
    for (key, value) in [
        ("Eligible", &j.eligible),
        ("Started", &j.start),
        ("Ended", &j.end),
    ] {
        if present(value) != "—" {
            lines.push(kv(key, full_time(value)));
        }
    }
    let finished = !(j.running() || j.pending());
    if finished && notable_exit(&j.exit_code).is_none() && present(&j.exit_code) != "—" {
        lines.push(kv_styled("Exit", j.exit_code.clone(), muted()));
    }
    lines.extend([
        Line::default(),
        section("ACCOUNTING"),
        kv("Owner", j.user.clone()),
        kv("Partition", j.partition.clone()),
    ]);
    for (key, value) in [("Account", &j.account), ("QoS", &j.qos)] {
        if present(value) != "—" {
            lines.push(kv(key, value.clone()));
        }
    }
    if !j.pending() && present(&j.reason) != "—" {
        lines.push(kv("Reason", j.reason.clone()));
    }
    // Long paths and raw TRES strings wrap badly in the side pane; `i` shows them.
    if app.detail {
        lines.extend([
            Line::default(),
            section("RAW"),
            label("Working directory"),
            Line::from(present(&j.work_dir).to_string()),
            label("Allocated TRES"),
            Line::from(present(&j.gres).to_string()),
            label("Requested TRES"),
            Line::from(present(&j.req_tres).to_string()),
        ]);
    } else {
        lines.extend([Line::default(), label("i expands paths and raw TRES")]);
    }
    lines
}
fn history_inspector(f: &mut Frame, area: Rect, app: &App) {
    draw_inspector(
        f,
        area,
        " JOB DETAILS ",
        selected_detail_lines(app),
        app.history_detail_scroll,
    );
}
fn footer(f: &mut Frame, area: Rect, app: &App) {
    let selected = app
        .selected
        .selected()
        .map(|i| i + 1)
        .unwrap_or(0)
        .min(app.count());
    let (fetched, error, loading) = if app.tab == 4 {
        (
            app.history_fetched,
            app.history_error.as_ref(),
            app.history_loading,
        )
    } else {
        (app.data.fetched, app.error.as_ref(), app.loading)
    };
    let age = fetched
        .map(|t| format!("updated {}s ago", t.elapsed().as_secs()))
        .unwrap_or_else(|| {
            if loading {
                format!("loading {}s", app.started.elapsed().as_secs())
            } else {
                "no snapshot".into()
            }
        });
    let state = if app.paused && app.tab != 4 {
        " · PAUSED"
    } else {
        ""
    };
    let unsupported = app.tab == 4 && app.history_unsupported;
    let error = error.filter(|_| !unsupported);
    let status = if unsupported {
        format!(
            "No job accounting on this cluster · live tabs 1–3 still update · user {}",
            app.args.user
        )
    } else if let Some(error) = error {
        if app.tab == 4 {
            format!("History refresh failed · {error}")
        } else {
            format!("STALE · {age} · {error}")
        }
    } else if let Some(notice) = &app.notice {
        format!("{notice}{state}")
    } else {
        format!(
            "{selected}/{} · {age}{state} · user {}",
            app.count(),
            app.args.user
        )
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(style(EDGE));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(status).style(style(if error.is_some() { RED } else { DIM })),
        Rect::new(inner.x, inner.y, inner.width, inner.height.min(1)),
    );
    if inner.height > 1 {
        let wide = inner.width >= 85;
        let keys: &[(&str, &str)] = if app.filter_open {
            &[
                ("j/k", "move"),
                ("Space", "toggle"),
                ("Esc", "close"),
                ("?", "keys"),
            ]
        } else if app.editing {
            &[("type", "to search"), ("Enter/Esc", "finish")]
        } else if app.detail {
            if wide {
                &[
                    ("j/k", "scroll"),
                    ("^D/^U", "half page"),
                    ("g/G", "top/end"),
                    ("i/Esc", "back"),
                    ("?", "keys"),
                ]
            } else {
                &[("j/k", "scroll"), ("i/Esc", "back"), ("?", "keys")]
            }
        } else if wide {
            if app.tab == 1 {
                &[
                    ("j/k", "move"),
                    ("↵", "details"),
                    ("/", "search"),
                    ("f", "filter"),
                    ("a", "free GPUs"),
                    ("s", "sort"),
                    ("c", "clusters"),
                    ("?", "keys"),
                    ("q", "quit"),
                ]
            } else {
                &[
                    ("j/k", "move"),
                    ("↵", "logs"),
                    ("i", "details"),
                    ("/", "search"),
                    ("f", "filter"),
                    ("s", "sort"),
                    ("c", "clusters"),
                    ("?", "keys"),
                    ("q", "quit"),
                ]
            }
        } else if app.tab == 1 {
            &[
                ("j/k", "move"),
                ("↵", "details"),
                ("?", "keys"),
                ("q", "quit"),
            ]
        } else {
            &[("j/k", "move"), ("↵", "logs"), ("?", "keys"), ("q", "quit")]
        };
        f.render_widget(
            Paragraph::new(hints(keys)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}
fn filter_panel(f: &mut Frame, area: Rect, app: &App) -> (Rect, Rect) {
    let options = app.filter_options();
    let width = 46.min(area.width);
    let height = (options.len() as u16 + 7).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let mut lines = vec![
        label("j/k move · Space or click toggles"),
        label("Tab / Shift-Tab move · ? help"),
    ];
    for (index, &option) in options.iter().enumerate() {
        let chosen = app.category_selected(index);
        let focused = index == app.filter_cursors[usize::from(app.tab - 1)];
        lines.push(Line::from(Span::styled(
            format!(
                "{} [{}] {}",
                if focused { "›" } else { " " },
                if chosen { "x" } else { " " },
                option
            ),
            if focused {
                focus_style()
            } else if chosen {
                bold(GREEN)
            } else {
                muted()
            },
        )));
    }
    lines.push(Line::default());
    lines.push(label("Enter / Esc / q close · / text search"));
    lines.push(label("All or no checks = no state filter"));
    let title = match app.tab {
        1 => " Fleet filters ",
        2 => " My jobs filters ",
        4 => " History filters ",
        _ => " Queue filters ",
    };
    // Options follow the border and the two hint lines.
    let options_area = Rect::new(
        rect.x + 1,
        rect.y + 3,
        rect.width.saturating_sub(2),
        (options.len() as u16).min(rect.bottom().saturating_sub(rect.y + 4)),
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).style(style(FG).bg(PANEL)).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(style(ACCENT)),
        ),
        rect,
    );
    (rect, options_area)
}
fn help(f: &mut Frame, area: Rect, app: &App) {
    let (context, keys): (&str, Vec<(&str, &str)>) = if app.log.is_some() {
        (
            "Log",
            vec![
                ("j/k · ↑/↓", "Move / extend selection"),
                ("^D / ^U", "Half page down / up"),
                ("^F / ^B", "Full page down / up"),
                ("g / G · h/l", "Top / bottom · sideways"),
                ("/ · n / N", "Search · next / previous"),
                ("v / V · y", "Select · yank selection/file"),
                ("r", "Reload log"),
                ("c", "Choose / add / remove cluster"),
                ("q / Esc", "Cancel selection / return"),
                ("Q / Ctrl-C", "Quit monitor"),
            ],
        )
    } else if app.filter_open {
        (
            "Filters",
            vec![
                ("j/k · ↑/↓", "Move between options"),
                ("Tab / ⇧Tab", "Next / previous option"),
                ("Space", "Toggle selected option"),
                ("Enter / q / Esc", "Close filters"),
                ("/", "Search rows"),
                ("All / no checks", "Show all states"),
                ("Q / Ctrl-C", "Quit monitor"),
            ],
        )
    } else {
        let mut keys = vec![
            (
                "j/k · ↑/↓",
                if app.detail {
                    "Scroll expanded details"
                } else {
                    "Select a row"
                },
            ),
            ("^D / ^U", "Half page down / up"),
            ("^F/^B · PgDn/Up", "Full page down / up"),
            ("g / G", "First / last"),
            ("Tab / ⇧Tab · 1–4", "Next / previous tab · jump"),
            ("c", "Choose / add / remove cluster"),
            ("i · J / K", "Expand · scroll details"),
            (
                "Enter",
                if app.tab == 1 {
                    "Expand node details"
                } else {
                    "Open job log"
                },
            ),
            ("/ · f · x", "Search / filter / clear"),
            ("s · p · r", "Sort / pause / refresh"),
            ("Click · wheel", "Select row or tab · scroll"),
            (
                "Double-click",
                if app.tab == 1 {
                    "Expand node details"
                } else {
                    "Open job log"
                },
            ),
        ];
        if app.tab == 1 {
            keys.push(("a", "Show nodes with free GPUs"));
        } else {
            keys.push(("o", "Open job log (also in details)"));
        }
        keys.extend([
            ("q / Esc", "Back · q quits from table"),
            ("Q / Ctrl-C", "Quit monitor anywhere"),
        ]);
        (
            if app.detail {
                "Details"
            } else if app.tab == 1 {
                "Fleet"
            } else {
                "Jobs"
            },
            keys,
        )
    };
    let width = 66.min(area.width);
    let height = (keys.len() as u16 + 4).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, rect);
    let block = Block::default()
        .title(format!(" {context} · keys "))
        .title_bottom(" Any key closes help ")
        .borders(Borders::ALL)
        .border_style(style(ACCENT))
        .style(style(FG).bg(PANEL));
    let inner = inset(block.inner(rect), 1, 0);
    f.render_widget(block, rect);
    let key_width = if inner.width >= 58 { 20 } else { 16 };
    let rows = keys.into_iter().map(|(key, action)| {
        Row::new([
            Cell::from(key).style(bold(ACCENT)),
            Cell::from(action).style(style(FG)),
        ])
    });
    f.render_widget(
        Table::new(rows, [Constraint::Length(key_width), Constraint::Min(1)]).column_spacing(1),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use ratatui::{Terminal, backend::TestBackend};
    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
    #[test]
    fn cluster_picker_scrolls_and_renders_over_logs_at_small_sizes() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop"]));
        app.picker.open = true;
        app.picker.hosts.hosts = (0..40).map(|i| format!("cluster-{i:02}")).collect();
        app.picker.selected = 39;
        for (w, h) in [(48, 14), (80, 24), (120, 36)] {
            assert!(screen(&mut app, w, h).contains("cluster-39"));
        }
        app.log = Some(crate::top::history::LogView::loading("42"));
        assert!(screen(&mut app, 80, 24).contains("Clusters"));
    }
    #[test]
    fn help_uses_the_current_context_and_renders_over_logs_and_filters() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop"]));
        app.help = true;
        assert!(screen(&mut app, 80, 24).contains("Fleet · keys"));
        app.filter_open = true;
        let filter_help = screen(&mut app, 80, 24);
        assert!(filter_help.contains("Filters · keys"));
        assert!(filter_help.contains("Toggle selected option"));
        let mut log = crate::top::history::LogView::loading("42");
        log.lines = vec!["example output".into()];
        app.log = Some(log);
        let log_help = screen(&mut app, 80, 24);
        assert!(log_help.contains("Log · keys"));
        assert!(log_help.contains("yank selection/file"));
        assert!(!log_help.contains("Toggle selected option"));
    }
    #[test]
    fn history_status_uses_its_own_snapshot_and_error() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop", "--tab", "4"]));
        app.error = Some("live controller unavailable".into());
        app.history_fetched = Some(std::time::Instant::now() - std::time::Duration::from_secs(6));
        let contents = screen(&mut app, 100, 24);
        assert!(contents.contains("updated 6s ago"));
        assert!(!contents.contains("live controller unavailable"));
        app.history_error = Some("accounting unavailable".into());
        assert!(screen(&mut app, 100, 24).contains("accounting unavailable"));
    }
    #[test]
    fn disabled_accounting_is_explained_without_stale_status() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop", "--tab", "4"]));
        app.args.target = Some(crate::top::hosts::Target::Local);
        app.history_loaded = true;
        app.history_unsupported = true;
        app.history_error = Some("Slurm accounting storage is disabled".into());
        let contents = screen(&mut app, 120, 30);
        assert!(contents.contains("Job history isn't available on this cluster"));
        assert!(contents.contains("NO ACCOUNTING"));
        assert!(!contents.contains("STALE"));
        assert!(!contents.contains("retry"));
    }
    #[test]
    fn untyped_gpus_show_no_model_and_counts_are_pluralized() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop"]));
        app.data.nodes = crate::top::data::parse_nodes(
            "NodeName=solo State=IDLE Partitions=gpu Gres=gpu:4 CPUTot=104 RealMemory=385024 AllocTRES=",
            "gpu",
        );
        app.rebuild_views();
        let contents = screen(&mut app, 140, 36);
        assert!(contents.contains("4 / 4 GPUs"));
        assert!(contents.contains("1 / 1 node "));
        assert!(contents.contains("Model not reported"));
        assert!(!contents.contains("4 × GPU models"));
    }
    #[test]
    fn backgrounds_follow_the_terminal_in_views_and_overlays() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop"]));
        for overlay in 0..3 {
            app.help = overlay == 1;
            app.filter_open = overlay == 2;
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .all(|cell| cell.bg == Color::Reset)
            );
        }
    }
    #[test]
    fn detail_scroll_limit_keeps_the_last_wrapped_line_visible() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop", "--tab", "4"]));
        app.history = vec![Job {
            id: "42".into(),
            user: app.args.user.clone(),
            partition: app.args.partition.clone(),
            work_dir: "/a/long/path/with spaces that wrap over multiple display lines".repeat(4),
            req_tres: "FINAL RESOURCE MARKER".into(),
            ..Default::default()
        }];
        app.reset_selection();
        for (width, height, expanded) in [(48, 14, true), (80, 24, true), (112, 36, false)] {
            app.detail = expanded;
            app.history_detail_scroll = detail_max_scroll(&app, width, height);
            let contents = screen(&mut app, width, height);
            // Raw TRES is the last field when expanded; the side pane ends with the expand hint.
            assert!(contents.contains(if expanded {
                "FINAL RESOURCE MARKER"
            } else {
                "i expands paths"
            }));
        }
    }
    #[test]
    fn history_details_follow_selection_and_render_at_all_sizes() {
        let mut app = App::new(crate::top::Args::parse_from([
            "mytop", "--tab", "4", "--user", "me",
        ]));
        app.history = ["first", "second"]
            .iter()
            .enumerate()
            .map(|(i, name)| Job {
                id: i.to_string(),
                name: name.to_string(),
                user: "me".into(),
                partition: "gpu".into(),
                gres: "gres/gpu=1,mem=32G".into(),
                nodes: "node".into(),
                start: "2026-10-18T02:14:42".into(),
                cpus: "8".into(),
                ..Default::default()
            })
            .collect();
        app.data.nodes = crate::top::data::parse_nodes(
            "NodeName=node Partitions=gpu Gres=gpu:titanrtx:8 AllocTRES=",
            "gpu",
        );
        app.reset_selection();
        let first = app.identity();
        let content = |app: &App| {
            history_lines(
                &app.history[app.job_view[app.selected.selected().unwrap()]],
                app,
            )
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
        };
        assert!(content(&app).contains("1 GPU (model not recorded)"));
        assert!(content(&app).contains("current, not historical"));
        assert!(content(&app).contains("TITANRTX"));
        assert!(content(&app).contains("CPUs      8"));
        assert!(content(&app).contains("Memory    32G"));
        app.move_by(1);
        assert_ne!(app.identity(), first);
        for (w, h) in [(48, 14), (80, 24), (112, 36), (160, 50)] {
            for detail in [false, true] {
                app.detail = detail;
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal.draw(|f| draw(f, &mut app)).unwrap();
                let screen = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>();
                if detail || w >= 112 {
                    assert!(screen.contains("JOB DETAILS"));
                    if h >= 24 {
                        assert!(screen.contains("ALLOCATION"));
                    }
                }
            }
        }
    }
    #[test]
    fn category_panel_shows_context_and_keyboard_hints() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop"]));
        app.begin_filter();
        for (tab, expected) in [(1, "offline"), (2, "pending"), (3, "suspended")] {
            app.tab = tab;
            app.rebuild_views();
            let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let contents = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(contents.contains(expected));
            assert!(contents.contains("j/k move"));
            assert!(contents.contains("Enter / Esc"));
        }
    }
    #[test]
    fn job_values_hide_slurm_placeholders_and_keep_exceptions() {
        assert_eq!(short_time("2026-11-22T15:28:10"), "11-22 15:28");
        assert_eq!(short_time("2026-11-22"), "11-22");
        assert_eq!(short_time("Unknown"), "—");
        assert_eq!(full_time("2026-11-22T15:28:10"), "2026-11-22 15:28:10");
        for zero in ["00:00:00", "0:00", "0-00:00:00"] {
            assert_eq!(elapsed_label(zero), "—");
        }
        assert_eq!(elapsed_label("1-02:03:04"), "1-02:03:04");
        assert_eq!(notable_exit("0:0"), None);
        assert_eq!(notable_exit("0:9"), Some("0:9"));
        assert_eq!(notable_exit("1:0"), Some("1:0"));
        assert_eq!(job_state("CANCELLED by 123").0, "– cancelled");
        assert_eq!(job_state("OUT_OF_MEMORY"), ("✗ oom".into(), RED));
        assert_eq!(gpu_label("gres/gpu=2,gres/gpu:a100=2"), "2×A100");
        assert_eq!(gpu_label("gpu:1"), "1 GPU");
        assert_eq!(gpu_label(""), "—");
    }
    fn history_fixture() -> App {
        let mut app = App::new(crate::top::Args::parse_from([
            "mytop", "--tab", "4", "--user", "me",
        ]));
        let job = |id: &str, name: &str, state: &str, elapsed: &str, exit: &str, end: &str| Job {
            id: id.into(),
            name: name.into(),
            user: "me".into(),
            partition: "gpu".into(),
            state: state.into(),
            elapsed: elapsed.into(),
            exit_code: exit.into(),
            end: end.into(),
            submitted: format!("2026-11-21T02:{}:00", &id[2..]),
            limit: "1-22:00:00".into(),
            gres: "gres/gpu=1,mem=40G".into(),
            cpus: "8".into(),
            nodes: "gpu03".into(),
            ..Default::default()
        };
        app.history = vec![
            job(
                "7146",
                "ae95-swin-fine-hf-25k",
                "RUNNING",
                "00:06:12",
                "0:0",
                "Unknown",
            ),
            job(
                "7144",
                "ae95-swin-fine-smoke",
                "COMPLETED",
                "00:01:25",
                "0:0",
                "2026-11-21T09:36:18",
            ),
            job(
                "7132",
                "ae95-swin-coarse-hf-100k",
                "PENDING",
                "00:00:00",
                "0:0",
                "Unknown",
            ),
            job(
                "7130",
                "ae95-swin-coarse-hf-75k",
                "FAILED",
                "01:52:50",
                "1:0",
                "2026-11-22T11:04:14",
            ),
            job(
                "7129",
                "ae95-swin-coarse-lf-75k",
                "OUT_OF_MEMORY",
                "03:05:19",
                "0:125",
                "2026-11-22T11:46:06",
            ),
            job(
                "7101",
                "ae95-direct-fine-hf-25k",
                "CANCELLED by 1",
                "02:58:57",
                "0:15",
                "2026-11-22T12:00:00",
            ),
        ];
        app.reset_selection();
        app
    }
    #[test]
    fn history_table_fits_beside_the_inspector() {
        let mut app = history_fixture();
        let contents = screen(&mut app, 112, 30);
        assert!(contents.contains("JOB DETAILS"));
        assert!(contents.contains("ENDED"));
        assert!(contents.contains("11-22 11:04"));
        assert!(contents.contains("✗ failed 1:0"));
        assert!(contents.contains("◌ pending"));
        assert!(!contents.contains("Unknown"));
        assert!(contents.contains("6 jobs · 7d"));
        if std::env::var_os("MYTOP_SNAPSHOT").is_some() {
            for w in [112, 160] {
                let mut terminal = Terminal::new(TestBackend::new(w, 30)).unwrap();
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                for y in 0..buffer.area.height {
                    let line: String = (0..w).map(|x| buffer[(x, y)].symbol()).collect();
                    println!("{}", line.trim_end());
                }
            }
        }
    }
    #[test]
    fn history_sorts_missing_end_last_and_filters_live_states() {
        let mut app = history_fixture();
        app.job_sort = "ended".into();
        app.reset_selection();
        let ids: Vec<_> = app
            .job_view
            .iter()
            .map(|&i| app.history[i].id.as_str())
            .collect();
        assert_eq!(ids, ["7101", "7129", "7130", "7144", "7146", "7132"]);
        let running = app
            .filter_options()
            .iter()
            .position(|o| *o == "running")
            .unwrap();
        let pending = app
            .filter_options()
            .iter()
            .position(|o| *o == "pending")
            .unwrap();
        app.category_masks[3] = (1 << running) | (1 << pending);
        app.reset_selection();
        assert_eq!(app.count(), 2);
        assert_eq!(app.category(), "running + pending");
    }
    #[test]
    fn viewport_follows_selection_and_resize() {
        let mut state = TableState::default().with_selected(9999);
        let (start, capacity, local) = viewport(&mut state, 10000, 20);
        assert_eq!((start, capacity, local.selected()), (9982, 18, Some(17)));
        let (start, capacity, local) = viewport(&mut state, 10000, 40);
        assert_eq!((start, capacity, local.selected()), (9962, 38, Some(37)));
        state.select(Some(0));
        assert_eq!(viewport(&mut state, 10000, 20).0, 0);
    }
    #[test]
    fn large_queue_last_row_renders() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop", "--tab", "3"]));
        app.data.jobs = (0..10000)
            .map(|i| Job {
                id: format!("{i:05}"),
                name: format!("job-{i:05}"),
                partition: "gpu".into(),
                state: "RUNNING".into(),
                ..Default::default()
            })
            .collect();
        app.rebuild_views();
        app.move_by(10000);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let contents = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(contents.contains("job-09999"));
        assert_eq!(app.selected.selected(), Some(9999));
    }
    #[test]
    fn layouts_render_without_overflow() {
        let mut app = App::new(crate::top::Args::parse_from(["mytop"]));
        app.loading = false;
        app.data.nodes = crate::top::data::parse_nodes(
            "NodeName=testgpu01 State=MIXED Partitions=gpu Gres=gpu:a100:4 CPUAlloc=12 CPUTot=64 RealMemory=64000 AllocMem=12000 AllocTRES=gres/gpu=2,gres/gpu:a100=2",
            "gpu",
        );
        for (w, h) in [(40, 10), (60, 20), (80, 24), (120, 36), (180, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for tab in 1..=4 {
                app.tab = tab;
                app.rebuild_views();
                terminal.draw(|f| draw(f, &mut app)).unwrap();
            }
            app.tab = 1;
            app.rebuild_views();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let contents = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            if w >= 48 {
                assert!(contents.contains("testgpu01"));
            } else {
                assert!(contents.contains("Resize"));
            }
        }
    }
}
