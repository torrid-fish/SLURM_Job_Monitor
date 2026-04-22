//! Rendering logic using Ratatui.

use super::app::{App, FocusedPanel, LayoutMode};
use crate::utils::JobStatus;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

/// Render the entire UI.
pub fn render(frame: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(frame.area());

    render_header(frame, app, main_chunks[0]);

    let body_area = main_chunks[1];

    match app.layout {
        LayoutMode::Horizontal => render_horizontal(frame, app, body_area),
        LayoutMode::Vertical => render_vertical(frame, app, body_area),
        LayoutMode::Stacked => render_stacked(frame, app, body_area),
        LayoutMode::FullLog => render_full_log(frame, app, body_area),
    }
}

fn render_horizontal(frame: &mut Frame, app: &mut App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    render_status_panel(frame, app, body_chunks[0]);
    render_output_panel_vertical(frame, app, body_chunks[1]);
}

fn render_vertical(frame: &mut Frame, app: &mut App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(area);

    render_status_panel(frame, app, body_chunks[0]);
    render_output_panel_horizontal(frame, app, body_chunks[1]);
}

fn render_stacked(frame: &mut Frame, app: &mut App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(40), Constraint::Percentage(40)])
        .split(area);

    render_status_panel(frame, app, body_chunks[0]);
    render_stdout_panel(frame, app, body_chunks[1]);
    render_stderr_panel(frame, app, body_chunks[2]);
}

fn render_full_log(frame: &mut Frame, app: &mut App, area: Rect) {
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_stdout_panel(frame, app, body_chunks[0]);
    render_stderr_panel(frame, app, body_chunks[1]);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let job_count = app.jobs.len();
    let mut title = format!(
        "SLURM Job Monitor - {} job{} [{}]",
        job_count,
        if job_count == 1 { "" } else { "s" },
        app.layout.name(),
    );

    if let Some(job_id) = app.current_job_id {
        if let Some(job) = app.jobs.get(&job_id) {
            let name = if job.info.job_name.is_empty() {
                format!("Job {}", job_id)
            } else {
                job.info.job_name.clone()
            };
            title.push_str(&format!(" | Current: {} (ID: {})", name, job_id));
        }
    }

    let help_text = "Ctrl+C: exit | Scroll: arrows | Tab: switch panels | l: layout | Enter: editor";

    let header_text = vec![
        Line::from(Span::styled(title, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(help_text, Style::default().fg(Color::DarkGray))),
    ];

    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Blue)));

    frame.render_widget(header, area);
}

fn render_status_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let panel_title = "Job Status (n: prev, p: next, d: delete)";

    if app.jobs.is_empty() {
        let empty = Paragraph::new("No jobs")
            .block(Block::default().title(panel_title).borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
        frame.render_widget(empty, area);
        return;
    }

    // Dynamic name truncation: use available width instead of hardcoded 20
    // area.width - 2 (borders) - 12*3 (fixed cols) - 3 (column gaps) = area.width - 41
    let name_max_len = (area.width as usize).saturating_sub(41).max(10);

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
    .block(
        Block::default()
            .title(panel_title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn render_output_panel_vertical(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.current_job_id.is_none() {
        let empty = Paragraph::new("Select a job to view output")
            .block(Block::default().title("Output").borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
        frame.render_widget(empty, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_stdout_panel(frame, app, chunks[0]);
    render_stderr_panel(frame, app, chunks[1]);
}

fn render_output_panel_horizontal(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.current_job_id.is_none() {
        let empty = Paragraph::new("Select a job to view output")
            .block(Block::default().title("Output").borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
        frame.render_widget(empty, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_stdout_panel(frame, app, chunks[0]);
    render_stderr_panel(frame, app, chunks[1]);
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

    let is_focused = app.focused_panel == FocusedPanel::Stdout;
    let border_color = if is_focused {
        Color::LightGreen
    } else {
        Color::DarkGray
    };

    let focus_indicator = if is_focused {
        " [FOCUSED]"
    } else {
        " [Press Tab to focus]"
    };

    let scroll_indicator = if job.stdout_scroll_mode {
        " [SCROLL MODE - Press 'q' to exit]"
    } else {
        ""
    };

    let title = format!(
        "STDOUT (Job {}){}{}",
        job_id, focus_indicator, scroll_indicator
    );

    let title_style = if is_focused {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Calculate visible lines
    let inner_height = area.height.saturating_sub(2) as usize;
    let visible_lines = get_visible_lines(&job.stdout_lines, job.stdout_scroll, inner_height);

    let content = if visible_lines.is_empty() {
        "[No output yet - waiting for file updates...]".to_string()
    } else {
        visible_lines.join("\n")
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(Span::styled(title, title_style))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

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

    let is_focused = app.focused_panel == FocusedPanel::Stderr;
    let border_color = if is_focused {
        Color::LightRed
    } else {
        Color::DarkGray
    };

    let focus_indicator = if is_focused {
        " [FOCUSED]"
    } else {
        " [Press Tab to focus]"
    };

    let scroll_indicator = if job.stderr_scroll_mode {
        " [SCROLL MODE - Press 'q' to exit]"
    } else {
        ""
    };

    let title = format!(
        "STDERR (Job {}){}{}",
        job_id, focus_indicator, scroll_indicator
    );

    let title_style = if is_focused {
        Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Calculate visible lines
    let inner_height = area.height.saturating_sub(2) as usize;
    let visible_lines = get_visible_lines(&job.stderr_lines, job.stderr_scroll, inner_height);

    let content = if visible_lines.is_empty() {
        "[No output yet - waiting for file updates...]".to_string()
    } else {
        visible_lines.join("\n")
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .title(Span::styled(title, title_style))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );

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
