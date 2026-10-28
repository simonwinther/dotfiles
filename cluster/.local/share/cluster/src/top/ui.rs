use crate::top::{
    App,
    data::{Job, Node},
    theme::{ACCENT, BG, BLUE, DIM, EDGE, FG, GREEN, PANEL, RED, focus_style, selection_style},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};
fn style(c: Color) -> Style {
    Style::default().fg(c)
}
fn bold(c: Color) -> Style {
    style(c).add_modifier(Modifier::BOLD)
}
fn label(s: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(s.into(), style(DIM)))
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
fn meter(free: u32, total: u32, width: usize) -> Line<'static> {
    let n = if total == 0 {
        0
    } else {
        ((free as f64 / total as f64) * width as f64).round() as usize
    }
    .min(width);
    Line::from(vec![
        Span::styled("▰".repeat(n), style(GREEN)),
        Span::styled("▱".repeat(width - n), style(EDGE)),
    ])
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
            cluster_picker(f, area, app);
        }
        return;
    }
    let a = inset(area, 2, 0);
    let parts = Layout::vertical([
        Constraint::Length(if area.height < 20 { 2 } else { 3 }),
        Constraint::Length(if area.height < 20 { 3 } else { 4 }),
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(a);
    header(f, parts[0], app);
    metrics(f, parts[1], app);
    toolbar(f, parts[2], app);
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
    if app.tab == 4 {
        if app.detail {
            history_inspector(f, parts[3], app);
        } else {
            crate::top::history::draw(f, panes[0], app);
            if wide {
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
        filter_panel(f, area, app);
    }
    if app.help {
        help(f, area, app)
    }
    if app.picker.open {
        cluster_picker(f, area, app);
    }
}
fn cluster_picker(f: &mut Frame, area: Rect, app: &App) {
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
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x, inner.y, inner.width, list_height as u16),
    );
    let mut footer = vec![label(
        "↑/↓ choose · Enter connect · a add · d remove saved host",
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
}
fn header(f: &mut Frame, a: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled(" cluster top ", bold(ACCENT)),
        Span::styled("  SLURM", bold(FG)),
        Span::styled(
            format!(
                "  [{}]",
                app.args
                    .target
                    .as_ref()
                    .map_or("choose cluster", |target| target.label())
            ),
            style(ACCENT),
        ),
        Span::styled("  /  ", style(EDGE)),
        Span::styled(
            if app.args.partition.is_empty() {
                "all partitions"
            } else {
                &app.args.partition
            },
            style(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(line), Rect::new(a.x, a.y + 1, a.width, 1));
    if a.width >= 75 {
        let (error, loading) = if app.tab == 4 {
            (app.history_error.is_some(), app.history_loading)
        } else {
            (app.error.is_some(), app.loading)
        };
        let badge = if app.args.target.is_none() {
            "○ SELECT CLUSTER"
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
                } else if app.paused && app.tab != 4 {
                    ACCENT
                } else {
                    GREEN
                }))
                .alignment(ratatui::layout::Alignment::Right),
            Rect::new(a.right() - 16, a.y + 1, 16, 1),
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
        .filter(|j| j.user == app.args.user)
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
            format!("/ {total} GPU"),
            "UNALLOCATED",
            GREEN,
        ),
        (
            ready.to_string(),
            format!("/ {node_count} nodes"),
            "USABLE NODES",
            BLUE,
        ),
        (
            running.to_string(),
            format!("run · {pending} wait"),
            "YOUR JOBS (ALL)",
            ACCENT,
        ),
    ];
    for (r, (value, detail, title, color)) in cols.iter().zip(specs) {
        let lines = vec![
            Line::from(vec![
                Span::styled(value, bold(color)),
                Span::styled(format!("  {detail}"), style(FG)),
            ]),
            label(title),
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
fn toolbar(f: &mut Frame, a: Rect, app: &App) {
    let mut spans = vec![];
    for (n, title) in [(1, "Fleet"), (2, "My jobs"), (3, "Queue"), (4, "History")] {
        spans.push(Span::styled(
            format!("{n} {title} "),
            if n == app.tab {
                bold(ACCENT)
            } else {
                style(DIM)
            },
        ));
        spans.push(Span::styled(" ", style(DIM)));
    }
    if a.width >= 85 {
        spans.push(Span::styled(
            if app.tab == 1 {
                format!(
                    "sort: {}{}",
                    app.sort,
                    if app.available {
                        " · available only"
                    } else {
                        ""
                    }
                )
            } else {
                format!("{} jobs · sort: {}", app.count(), app.job_sort)
            },
            style(DIM),
        ));
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
    } else {
        f.render_widget(
            Paragraph::new(if app.tab == 1 {
                " GPU / CPU / RAM show unallocated capacity"
            } else if app.tab == 4 {
                " Accounting history · select a job to inspect its allocation"
            } else {
                " Priority order is not a guaranteed start order"
            })
            .style(style(DIM)),
            Rect::new(a.x, a.y + 1, a.width, 1),
        );
    }
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
            Constraint::Length(8),
            Constraint::Length(10),
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
        vec!["NODE", "STATE", "MODEL", "GPU FREE", "CPU", "RAM"]
    } else {
        vec!["NODE", "STATE", "MODEL", "GPU FREE"]
    };
    let (start, capacity, mut visible_selection) = viewport(&mut app.selected, ids.len(), a.height);
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
        if wide {
            gpu_line.spans.extend(meter(n.free(), n.total(), 7).spans);
        }
        let mut cells = vec![
            Cell::from(n.name.clone()).style(style(FG)),
            Cell::from(status).style(style(color)),
            Cell::from(n.models_label()).style(style(DIM)),
            Cell::from(gpu_line),
        ];
        if wide {
            cells.push(Cell::from(format!("{}/{}", n.cpu_free(), n.cpu_total)).style(style(DIM)));
            cells.push(
                Cell::from(format!(
                    "{}/{}",
                    gb(n.ram_total.saturating_sub(n.ram_used)),
                    gb(n.ram_total)
                ))
                .style(style(DIM)),
            );
        }
        Row::new(cells)
    });
    let table = Table::new(rows, widths)
        .header(Row::new(headers).style(style(DIM)).height(2))
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
    let widths = if wide {
        vec![
            Constraint::Length(13),
            Constraint::Length(10),
            Constraint::Min(12),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Length(15),
        ]
    } else {
        vec![
            Constraint::Length(12),
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(12),
        ]
    };
    let headers = if wide {
        vec!["JOB", "USER", "NAME", "STATE", "ELAPSED", "REASON / NODE"]
    } else {
        vec!["JOB", "NAME", "STATE", "REASON"]
    };
    let (start, capacity, mut visible_selection) = viewport(&mut app.selected, ids.len(), a.height);
    let rows = ids.iter().skip(start).take(capacity).map(|&i| {
        let j = &app.jobs()[i];
        let status = if j.running() {
            "running"
        } else if j.pending() {
            "pending"
        } else {
            &j.state
        };
        let color = if j.running() {
            GREEN
        } else if j.pending() {
            ACCENT
        } else {
            DIM
        };
        let mut cells = vec![Cell::from(j.id.clone()).style(style(FG))];
        if wide {
            cells.push(Cell::from(j.user.clone()).style(style(DIM)));
        }
        cells.push(Cell::from(j.name.clone()));
        cells.push(Cell::from(status).style(style(color)));
        if wide {
            cells.push(Cell::from(j.elapsed.clone()).style(style(DIM)));
        }
        cells.push(
            Cell::from(if j.running() {
                j.nodes.clone()
            } else {
                j.reason.clone()
            })
            .style(style(DIM)),
        );
        Row::new(cells)
    });
    f.render_stateful_widget(
        Table::new(rows, widths)
            .header(Row::new(headers).style(style(DIM)).height(2))
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
    let mut lines = vec![
        Line::from(Span::styled(n.name.clone(), bold(ACCENT))),
        Line::from(Span::styled(n.state.clone(), style(state(n).1))),
        label(n.partitions.join(" · ")),
        Line::default(),
        label("GPU MODELS"),
    ];
    for (model, count) in &n.models {
        lines.push(Line::from(format!("{count} × {}", model.to_uppercase())));
    }
    lines.extend([
        Line::default(),
        label("UNALLOCATED GPU / CPU / MEMORY"),
        Line::from(format!(
            "{} / {} GPUs",
            if n.gpu_known {
                n.free().to_string()
            } else {
                "?".into()
            },
            n.total()
        )),
        meter(n.free(), n.total(), 18),
        Line::from(format!("{} / {} CPUs", n.cpu_free(), n.cpu_total)),
        Line::from(format!(
            "{} / {} RAM",
            gb(n.ram_total.saturating_sub(n.ram_used)),
            gb(n.ram_total)
        )),
        Line::default(),
        label("ALLOCATED GPU / CPU / MEMORY"),
        Line::from(format!(
            "{} GPUs · {} CPUs · {} RAM",
            n.gpu_used,
            n.cpu_used,
            gb(n.ram_used)
        )),
        Line::default(),
    ]);
    if !n.reason.is_empty() && n.reason != "(null)" {
        lines.extend([
            label("NODE NOTE"),
            Line::from(n.reason.clone()),
            Line::default(),
        ]);
    }
    lines.push(label(
        "Unallocated capacity depends on CPU, memory, reservations and scheduling policy.",
    ));
    lines
}
fn job_lines(j: &Job, app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("#{} · {}", j.id, j.name),
            bold(ACCENT),
        )),
        Line::from(j.state.clone()),
        label(j.reason.clone()),
        Line::default(),
        label("OWNER / PARTITION"),
        Line::from(format!("{} / {}", j.user, j.partition)),
        Line::default(),
        label("GPU REQUEST"),
        Line::from(j.gres.clone()),
        label("NODE(S)"),
        Line::from(if j.nodes.is_empty() {
            "—".into()
        } else {
            j.nodes.clone()
        }),
        Line::default(),
        label("ELAPSED / LIMIT"),
        Line::from(format!("{} / {}", j.elapsed, j.limit)),
        label("SUBMITTED"),
        Line::from(j.submitted.clone()),
    ];
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
            label(format!("{ahead} higher/earlier priority jobs")),
            label("Same partition; not a start estimate."),
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
    let block = Block::default()
        .borders(Borders::LEFT | Borders::TOP)
        .title(title)
        .border_style(style(EDGE))
        .style(style(FG).bg(PANEL));
    let inner = inset(block.inner(area), 1, 0);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        inner,
    );
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
    let content_height = height
        .saturating_sub(if height < 20 { 10 } else { 12 })
        .saturating_sub(1);
    Paragraph::new(selected_detail_lines(app))
        .wrap(Wrap { trim: false })
        .line_count(content_width)
        .saturating_sub(usize::from(content_height))
        .min(usize::from(u16::MAX)) as u16
}
fn history_lines(j: &Job, app: &App) -> Vec<Line<'static>> {
    let present = |s: &str| {
        if s.is_empty() || s == "Unknown" || s == "None" {
            "—".to_string()
        } else {
            s.to_string()
        }
    };
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
    let mut lines = vec![
        Line::from(Span::styled(
            format!("#{} · {}", j.id, j.name),
            bold(ACCENT),
        )),
        Line::from(format!("{} · exit {}", j.state, present(&j.exit_code))),
        Line::default(),
        label("GPU ALLOCATION"),
        Line::from(gpu),
        label("RAN ON"),
        Line::from(present(&j.nodes)),
    ];
    if gpus.contains_key("gpu")
        && let Some(node) = app.data.nodes.iter().find(|n| n.name == j.nodes)
    {
        lines.push(label("NODE GPU MODELS NOW (not historical)"));
        lines.push(Line::from(node.models_label()));
    }
    let mem = j
        .gres
        .split(',')
        .find_map(|t| t.strip_prefix("mem="))
        .unwrap_or("—");
    let requested_mem = match j.req_mem.strip_suffix('c') {
        Some(value) => format!("{value} per CPU"),
        None => match j.req_mem.strip_suffix('n') {
            Some(value) => format!("{value} per node"),
            None => present(&j.req_mem),
        },
    };
    lines.extend([
        Line::default(),
        label("RUNTIME / TIME LIMIT"),
        Line::from(format!("{} / {}", present(&j.elapsed), present(&j.limit))),
        label("SUBMITTED"),
        Line::from(present(&j.submitted)),
        label("ELIGIBLE"),
        Line::from(present(&j.eligible)),
        label("STARTED"),
        Line::from(present(&j.start)),
        label("ENDED"),
        Line::from(present(&j.end)),
        Line::default(),
        label("ALLOCATED CPU / MEMORY"),
        Line::from(format!("{} CPUs / {mem}", present(&j.cpus))),
        label("REQUESTED MEMORY"),
        Line::from(requested_mem),
        label("OWNER / PARTITION"),
        Line::from(format!("{} / {}", j.user, j.partition)),
        label("ACCOUNT / QOS"),
        Line::from(format!("{} / {}", present(&j.account), present(&j.qos))),
        label("REASON"),
        Line::from(present(&j.reason)),
        label("WORKING DIRECTORY"),
        Line::from(present(&j.work_dir)),
        label("ALLOCATED RESOURCES"),
        Line::from(present(&j.gres)),
        label("REQUESTED RESOURCES"),
        Line::from(present(&j.req_tres)),
    ]);
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
    let status = if let Some(error) = error {
        format!("STALE · {age} · {error}")
    } else if let Some(notice) = &app.notice {
        format!("{notice}{state}")
    } else {
        format!(
            "{selected}/{} · {age}{state} · {}",
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
        let keys = if app.filter_open {
            "j/k move · Space toggle · Enter/q/Esc close · ? keys"
        } else if app.editing {
            "Type to search · Enter/Esc finish"
        } else if app.detail {
            if inner.width >= 85 {
                "j/k scroll · ^D/^U half page · g/G top/end · i/q/Esc back · ? keys"
            } else {
                "j/k scroll · i/q/Esc back · ? keys"
            }
        } else if inner.width >= 85 {
            if app.tab == 1 {
                "j/k move · ↵ details · / search · f filters · c clusters · a free · q quit · ? keys"
            } else {
                "j/k move · ↵ logs · i details · / search · f filters · c clusters · q quit · ? keys"
            }
        } else if app.tab == 1 {
            "j/k move · ↵ details · c clusters · q quit · ? keys"
        } else {
            "j/k · ↵ logs · c clusters · q quit · ? keys"
        };
        f.render_widget(
            Paragraph::new(keys).style(style(ACCENT)),
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}
fn filter_panel(f: &mut Frame, area: Rect, app: &App) {
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
        label("j/k move · Space toggle"),
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
                style(DIM)
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
            assert!(screen(&mut app, width, height).contains("FINAL RESOURCE MARKER"));
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
        assert!(content(&app).contains("NODE GPU MODELS NOW (not historical)"));
        assert!(content(&app).contains("TITANRTX"));
        assert!(content(&app).contains("8 CPUs / 32G"));
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
                        assert!(screen.contains("GPU ALLOCATION"));
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
