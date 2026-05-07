//! Rendering logic using Ratatui.

use super::app::{wrap_lines, App, FocusBlock, LayoutMode, RightTab};
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

    match app.layout {
        LayoutMode::Horizontal => render_horizontal(frame, app, body_area),
        LayoutMode::Vertical => render_vertical(frame, app, body_area),
    }

    render_brand(frame, main_chunks[1]);
}

fn render_brand(frame: &mut Frame, area: Rect) {
    let text = format!("lazyslurm v{}", env!("CARGO_PKG_VERSION"));
    let p = Paragraph::new(Span::styled(
        text,
        Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM),
    ))
    .alignment(Alignment::Right);
    frame.render_widget(p, area);
}

fn render_horizontal(frame: &mut Frame, app: &mut App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_status_panel(frame, app, body_chunks[0]);
    render_right_panel(frame, app, body_chunks[1], Direction::Vertical);
}

fn render_vertical(frame: &mut Frame, app: &mut App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_status_panel(frame, app, body_chunks[0]);
    render_right_panel(frame, app, body_chunks[1], Direction::Horizontal);
}

fn render_right_panel(frame: &mut Frame, app: &mut App, area: Rect, output_dir: Direction) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    let tab_strip = chunks[0];
    let content = chunks[1];

    render_tab_strip(frame, app, tab_strip);

    match app.focused_panel.right_tab() {
        RightTab::Details => render_details_tab(frame, app, content),
        RightTab::Output => render_output_tab(frame, app, content, output_dir),
    }
}

fn render_tab_strip(frame: &mut Frame, app: &App, area: Rect) {
    let active = app.focused_panel.right_tab();
    let make_span = |label: &str, is_active: bool| {
        let style = if is_active {
            Style::default().fg(FOCUS_COLOR).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(UNFOCUS_COLOR)
        };
        Span::styled(format!("  {}  ", label), style)
    };
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let details = Paragraph::new(Line::from(make_span("Details", active == RightTab::Details)))
        .alignment(Alignment::Center);
    let output = Paragraph::new(Line::from(make_span("Output", active == RightTab::Output)))
        .alignment(Alignment::Center);

    frame.render_widget(details, chunks[0]);
    frame.render_widget(output, chunks[1]);
}

fn render_details_tab(frame: &mut Frame, app: &App, area: Rect) {
    let label_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    let placeholder = "—".to_string();

    let lines: Vec<Line> = match app.current_job_id.and_then(|id| app.jobs.get(&id)) {
        Some(job) => {
            let pick = |s: &str| if s.is_empty() { placeholder.clone() } else { s.to_string() };
            let work_dir = job.info.work_dir.display().to_string();
            let stdout = job.info.stdout_path.display().to_string();
            let stderr = job.info.stderr_path.display().to_string();

            vec![
                Line::from(vec![
                    Span::styled("Job ID:   ", label_style),
                    Span::raw(format!("{}", job.info.job_id)),
                ]),
                Line::from(vec![
                    Span::styled("Name:     ", label_style),
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
                ]),
                Line::from(vec![
                    Span::styled("End:      ", label_style),
                    Span::raw(pick(&job.info.end_time)),
                ]),
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

    let focused = app.focused_panel == FocusBlock::Details;
    let paragraph = Paragraph::new(lines).block(block_for("Details", focused));
    frame.render_widget(paragraph, area);
}

fn render_output_tab(frame: &mut Frame, app: &mut App, area: Rect, output_dir: Direction) {
    if app.current_job_id.is_none() {
        let focused = matches!(app.focused_panel, FocusBlock::Stdout | FocusBlock::Stderr);
        let empty = Paragraph::new("Select a job to view output")
            .block(block_for("Output", focused));
        frame.render_widget(empty, area);
        return;
    }

    let chunks = Layout::default()
        .direction(output_dir)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_stdout_panel(frame, app, chunks[0]);
    render_stderr_panel(frame, app, chunks[1]);
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

    // Dynamic name truncation: use available width instead of hardcoded 20
    // area.width - 2 (borders) - 12*3 (fixed cols) - 3 (column gaps) = area.width - 41
    let name_max_len = (table_area.width as usize).saturating_sub(41).max(10);

    let sorted_ids = app.get_sorted_job_ids();

    // Sync table_state selection with current_job_id
    let selected_index = app
        .current_job_id
        .and_then(|cid| sorted_ids.iter().position(|&id| id == cid));
    app.table_state.select(selected_index);

    // Create table header
    let header_cells = ["Job ID", "Status", "Runtime", "Name"]
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

            let name = if job.info.job_name.is_empty() {
                format!("Job {}", job_id)
            } else if job.info.job_name.len() > name_max_len {
                format!("{}...", &job.info.job_name[..name_max_len.saturating_sub(3)])
            } else {
                job.info.job_name.clone()
            };

            Some(
                Row::new(vec![
                    Cell::from(job_id.to_string()).style(Style::default().fg(Color::Cyan)),
                    Cell::from(job.status.as_str()).style(Style::default().fg(status_color)),
                    Cell::from(runtime),
                    Cell::from(name),
                ])
                .height(1),
            )
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .row_highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
    .highlight_symbol("▶ ")
    .block(block_for(panel_title, focused));

    frame.render_stateful_widget(table, table_area, &mut app.table_state);
}

fn render_stdout_panel(frame: &mut Frame, app: &App, area: Rect) {
    let job_id = match app.current_job_id {
        Some(id) => id,
        None => return,
    };

    let job = match app.jobs.get(&job_id) {
        Some(j) => j,
        None => return,
    };

    let is_focused = app.focused_panel == FocusBlock::Stdout;

    let scroll_indicator = if job.stdout_scroll_mode {
        " [SCROLL]"
    } else {
        ""
    };

    let title = format!("STDOUT (Job {}){}", job_id, scroll_indicator);

    // Calculate visible lines
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let wrapped_lines = wrap_lines(&job.stdout_lines, inner_width);
    let visible_lines = get_visible_lines(&wrapped_lines, job.stdout_scroll, inner_height);

    let content = if visible_lines.is_empty() {
        "[No output yet - waiting for file updates...]".to_string()
    } else {
        visible_lines.join("\n")
    };

    let paragraph = Paragraph::new(content).block(block_for(&title, is_focused));

    frame.render_widget(paragraph, area);
}

fn render_stderr_panel(frame: &mut Frame, app: &App, area: Rect) {
    let job_id = match app.current_job_id {
        Some(id) => id,
        None => return,
    };

    let job = match app.jobs.get(&job_id) {
        Some(j) => j,
        None => return,
    };

    let is_focused = app.focused_panel == FocusBlock::Stderr;

    let scroll_indicator = if job.stderr_scroll_mode {
        " [SCROLL]"
    } else {
        ""
    };

    let title = format!("STDERR (Job {}){}", job_id, scroll_indicator);

    // Calculate visible lines
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let wrapped_lines = wrap_lines(&job.stderr_lines, inner_width);
    let visible_lines = get_visible_lines(&wrapped_lines, job.stderr_scroll, inner_height);

    let content = if visible_lines.is_empty() {
        "[No output yet - waiting for file updates...]".to_string()
    } else {
        visible_lines.join("\n")
    };

    let paragraph = Paragraph::new(content).block(block_for(&title, is_focused));

    frame.render_widget(paragraph, area);
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
