//! Rendering logic using Ratatui.

use super::app::{wrap_lines, App, FocusBlock, RightTab};
use crate::utils::JobStatus;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

/// Color used for the focused block's border and title.
const FOCUS_COLOR: Color = Color::Green;
/// Color used for unfocused block borders and titles.
const UNFOCUS_COLOR: Color = Color::White;

fn block_for(title: &str, focused: bool) -> Block<'_> {
    let border_color = if focused { FOCUS_COLOR } else { UNFOCUS_COLOR };
    let title_style = if focused {
        Style::default().fg(FOCUS_COLOR).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(UNFOCUS_COLOR)
    };
    Block::default()
        .title(Span::styled(title.to_string(), title_style))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
}

/// Render the entire UI.
pub fn render(frame: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let body_area = main_chunks[0];

    render_body(frame, app, body_area);

    render_brand(frame, main_chunks[1]);
}

fn render_brand(frame: &mut Frame, area: Rect) {
    let text = format!(" lazyslurm v{} ", env!("CARGO_PKG_VERSION"));
    let p = Paragraph::new(Span::styled(
        text,
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    ))
    .alignment(Alignment::Right);
    frame.render_widget(p, area);
}

fn render_body(frame: &mut Frame, app: &mut App, area: Rect) {
    // Fixed layout: JobList on top, info panel below. When zoomed, only the
    // panel containing the focused block is shown — the App computed its rect
    // and zeroed the other side.
    if app.zoomed {
        if matches!(app.focused_panel, FocusBlock::JobList) {
            render_status_panel(frame, app, area);
        } else {
            render_right_panel(frame, app, area, Direction::Vertical);
        }
        return;
    }

    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_status_panel(frame, app, body_chunks[0]);
    render_right_panel(frame, app, body_chunks[1], Direction::Vertical);
}

fn render_right_panel(frame: &mut Frame, app: &mut App, area: Rect, _output_dir: Direction) {
    let active = app.focused_panel.right_tab();
    // The right panel is "active" whenever focus isn't on the JobList — i.e.
    // it's on Details, Stdout, or Stderr.
    let focused = !matches!(app.focused_panel, FocusBlock::JobList);
    let border_color = if focused { FOCUS_COLOR } else { UNFOCUS_COLOR };

    // Outer block — borders only, no built-in title (we draw clickable tabs
    // on the top border ourselves so each tab gets its own click rect).
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Tabs live on the top border, between the rounded corners.
    let style_for = |is_active: bool| -> Style {
        if is_active {
            Style::default().fg(FOCUS_COLOR).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(UNFOCUS_COLOR)
        }
    };
    let details_label = " Details ";
    let output_label = " Output ";
    let details_rect = app.tab_details_rect;
    let output_rect = app.tab_output_rect;
    let sep_rect = Rect {
        x: details_rect.x + details_rect.width,
        y: details_rect.y,
        width: 1,
        height: 1,
    };

    frame.render_widget(
        Paragraph::new(Span::styled(
            details_label.to_string(),
            style_for(active == RightTab::Details),
        )),
        details_rect,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "│".to_string(),
            Style::default().fg(border_color),
        )),
        sep_rect,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            output_label.to_string(),
            style_for(active == RightTab::Output),
        )),
        output_rect,
    );

    match active {
        RightTab::Details => render_details_tab(frame, app, inner),
        RightTab::Output => render_output_tab(frame, app, inner),
    }
}

fn render_details_tab(frame: &mut Frame, app: &mut App, area: Rect) {
    let label_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    let section_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let placeholder = "—".to_string();

    let lines: Vec<Line> = match app.current_job_id.and_then(|id| app.jobs.get(&id)) {
        Some(job) => {
            let pick = |s: &str| if s.is_empty() { placeholder.clone() } else { s.to_string() };
            let work_dir = job.info.work_dir.display().to_string();
            let stdout = job.info.stdout_path.display().to_string();
            let stderr = job.info.stderr_path.display().to_string();
            let gpu_count = parse_tres_field(&job.info.alloc_tres, "gres/gpu")
                .or_else(|| parse_tres_field(&job.info.req_tres, "gres/gpu"))
                .unwrap_or_else(|| "0".to_string());
            let mem_alloc = parse_tres_field(&job.info.alloc_tres, "mem")
                .unwrap_or_else(|| placeholder.clone());

            vec![
                Line::from(Span::styled("── Identity ──", section_style)),
                Line::from(vec![
                    Span::styled("Job ID:   ", label_style),
                    Span::raw(format!("{}", job.info.job_id)),
                    Span::raw("    "),
                    Span::styled("Name: ", label_style),
                    Span::raw(pick(&job.info.job_name)),
                ]),
                Line::from(vec![
                    Span::styled("State:    ", label_style),
                    Span::raw(format!("{}", job.status)),
                    Span::raw("    "),
                    Span::styled("Raw: ", label_style),
                    Span::raw(pick(&job.info.state)),
                ]),
                Line::from(vec![
                    Span::styled("Node:     ", label_style),
                    Span::raw(pick(&job.info.node_list)),
                ]),
                Line::from(vec![
                    Span::styled("Limit:    ", label_style),
                    Span::raw(pick(&job.info.time_limit)),
                    Span::raw("    "),
                    Span::styled("Elapsed: ", label_style),
                    Span::raw(pick(&job.info.elapsed)),
                ]),
                Line::from(vec![
                    Span::styled("Start:    ", label_style),
                    Span::raw(pick(&job.info.start_time)),
                    Span::raw("    "),
                    Span::styled("End: ", label_style),
                    Span::raw(pick(&job.info.end_time)),
                ]),
                Line::from(""),
                Line::from(Span::styled("── Resources ──", section_style)),
                Line::from(vec![
                    Span::styled("CPUs:     ", label_style),
                    Span::raw(pick(&job.info.num_cpus)),
                    Span::raw("    "),
                    Span::styled("GPUs: ", label_style),
                    Span::raw(gpu_count),
                    Span::raw("    "),
                    Span::styled("Mem: ", label_style),
                    Span::raw(mem_alloc),
                ]),
                Line::from(vec![
                    Span::styled("AllocTRES: ", label_style),
                    Span::raw(pick(&job.info.alloc_tres)),
                ]),
                Line::from(vec![
                    Span::styled("ReqTRES:   ", label_style),
                    Span::raw(pick(&job.info.req_tres)),
                ]),
                Line::from(""),
                Line::from(Span::styled("── Live Usage (sstat) ──", section_style)),
                Line::from(vec![
                    Span::styled("AveCPU:   ", label_style),
                    Span::raw(pick(&job.info.ave_cpu)),
                    Span::raw("    "),
                    Span::styled("MaxRSS: ", label_style),
                    Span::raw(pick(&job.info.max_rss)),
                    Span::raw("    "),
                    Span::styled("AveRSS: ", label_style),
                    Span::raw(pick(&job.info.ave_rss)),
                ]),
                Line::from(""),
                Line::from(Span::styled("── Paths ──", section_style)),
                Line::from(vec![
                    Span::styled("WorkDir:  ", label_style),
                    Span::raw(if work_dir.is_empty() { placeholder.clone() } else { work_dir }),
                ]),
                Line::from(vec![
                    Span::styled("StdOut:   ", label_style),
                    Span::raw(if stdout.is_empty() { placeholder.clone() } else { stdout }),
                ]),
                Line::from(vec![
                    Span::styled("StdErr:   ", label_style),
                    Span::raw(if stderr.is_empty() { placeholder.clone() } else { stderr }),
                ]),
            ]
        }
        None => vec![Line::from(Span::raw("No job selected"))],
    };

    let total_lines = lines.len() as u16;
    let viewport = area.height;
    let max_scroll = total_lines.saturating_sub(viewport);
    app.details_scroll_max = max_scroll;
    let scroll = app.details_scroll.min(max_scroll);
    app.details_scroll = scroll;

    let paragraph = Paragraph::new(lines).scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Extract a `key=value` field from a SLURM TRES string (e.g.
/// `cpu=4,mem=16G,gres/gpu=2` -> `parse_tres_field("...", "gres/gpu") = Some("2")`).
fn parse_tres_field(tres: &str, key: &str) -> Option<String> {
    if tres.is_empty() {
        return None;
    }
    for part in tres.split(',') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn render_output_tab(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.current_job_id.is_none() {
        let empty = Paragraph::new("Select a job to view output");
        frame.render_widget(empty, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(area);

    render_inner_log(frame, app, chunks[0], LogKind::Stdout);
    render_separator(frame, chunks[1]);
    render_inner_log(frame, app, chunks[2], LogKind::Stderr);
}

#[derive(Clone, Copy)]
enum LogKind {
    Stdout,
    Stderr,
}

fn render_separator(frame: &mut Frame, area: Rect) {
    let line: String = "─".repeat(area.width as usize);
    let p = Paragraph::new(Span::styled(
        line,
        Style::default().fg(UNFOCUS_COLOR),
    ));
    frame.render_widget(p, area);
}

fn render_inner_log(frame: &mut Frame, app: &App, area: Rect, kind: LogKind) {
    if area.height == 0 {
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    let header_area = chunks[0];
    let content_area = chunks[1];

    let job_id = match app.current_job_id {
        Some(id) => id,
        None => return,
    };
    let job = match app.jobs.get(&job_id) {
        Some(j) => j,
        None => return,
    };

    let (label, focused, lines, scroll, scroll_mode) = match kind {
        LogKind::Stdout => (
            "STDOUT",
            app.focused_panel == FocusBlock::Stdout,
            &job.stdout_lines,
            job.stdout_scroll,
            job.stdout_scroll_mode,
        ),
        LogKind::Stderr => (
            "STDERR",
            app.focused_panel == FocusBlock::Stderr,
            &job.stderr_lines,
            job.stderr_scroll,
            job.stderr_scroll_mode,
        ),
    };

    let scroll_indicator = if scroll_mode { " [SCROLL]" } else { "" };
    let header_style = if focused {
        Style::default().fg(FOCUS_COLOR).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(UNFOCUS_COLOR).add_modifier(Modifier::BOLD)
    };
    let header = Paragraph::new(Span::styled(
        format!("▌ {}{}", label, scroll_indicator),
        header_style,
    ));
    frame.render_widget(header, header_area);

    let wrapped = wrap_lines(lines, content_area.width as usize);
    let visible = get_visible_lines(&wrapped, scroll, content_area.height as usize);
    let content = if visible.is_empty() {
        "[No output yet - waiting for file updates...]".to_string()
    } else {
        visible.join("\n")
    };
    frame.render_widget(Paragraph::new(content), content_area);
}

fn render_status_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focused_panel == FocusBlock::JobList;
    let panel_title = "Jobs (Tab: switch, ↑/↓: select, d: delete)";

    if app.jobs.is_empty() {
        let empty = Paragraph::new("No jobs").block(block_for(panel_title, focused));
        frame.render_widget(empty, area);
        return;
    }

    let table_area = area;

    // Six fixed-width columns (Job ID, Status, Runtime, Limit, Node) take up
    // 10+10+10+10+15 = 55 cols, plus 5 gaps × 3 cells + 2 borders = 72.
    // Whatever's left goes to the Name column, with a 10-char minimum.
    const FIXED_COLS_WIDTH: usize = 10 + 10 + 10 + 10 + 15 + (5 * 3) + 2;
    let name_max_len = (table_area.width as usize)
        .saturating_sub(FIXED_COLS_WIDTH)
        .max(10);

    let sorted_ids = app.get_sorted_job_ids();

    // Sync table_state selection with current_job_id
    let selected_index = app
        .current_job_id
        .and_then(|cid| sorted_ids.iter().position(|&id| id == cid));
    app.table_state.select(selected_index);

    // Create table header
    let header_cells = ["Job ID", "Status", "Runtime", "Limit", "Node", "Name"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).height(1);

    // Create table rows
    let rows: Vec<Row> = sorted_ids
        .iter()
        .filter_map(|&job_id| {
            let job = app.jobs.get(&job_id)?;

            let status_color = match job.status {
                JobStatus::Queued => Color::Yellow,
                JobStatus::Running => Color::Green,
                JobStatus::Completed => Color::Blue,
                JobStatus::Failed => Color::Red,
                JobStatus::Unknown => Color::White,
            };

            let runtime = if job.info.elapsed.is_empty() {
                "N/A".to_string()
            } else {
                job.info.elapsed.clone()
            };

            let limit = if job.info.time_limit.is_empty() {
                "—".to_string()
            } else {
                job.info.time_limit.clone()
            };

            let node = if job.info.node_list.is_empty() {
                "—".to_string()
            } else {
                truncate_with_ellipsis(&job.info.node_list, 15)
            };

            let name = if job.info.job_name.is_empty() {
                format!("Job {}", job_id)
            } else {
                truncate_with_ellipsis(&job.info.job_name, name_max_len)
            };

            Some(
                Row::new(vec![
                    Cell::from(job_id.to_string()).style(Style::default().fg(Color::Cyan)),
                    Cell::from(job.status.as_str()).style(Style::default().fg(status_color)),
                    Cell::from(runtime),
                    Cell::from(limit),
                    Cell::from(node),
                    Cell::from(name),
                ])
                .height(1),
            )
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(15),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .column_spacing(3)
    .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ")
    .block(block_for(panel_title, focused));

    frame.render_stateful_widget(table, table_area, &mut app.table_state);
}

fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        return s.to_string();
    }
    if max_len <= 3 {
        return s.chars().take(max_len).collect();
    }
    let head: String = s.chars().take(max_len - 3).collect();
    format!("{}...", head)
}

fn get_visible_lines(lines: &[String], scroll_pos: usize, max_height: usize) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    let total = lines.len();
    let max_scroll = total.saturating_sub(max_height);
    let scroll = scroll_pos.min(max_scroll);

    let end = (scroll + max_height).min(total);
    lines[scroll..end].to_vec()
}
