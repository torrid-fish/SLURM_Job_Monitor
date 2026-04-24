//! CLI entry point and command definitions.

use crate::job_manager::JobManager;

/// Write debug message to file
fn debug_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/lazyslurm_debug.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}
use crate::log_tailer::{LogTailer, LogUpdate};
use crate::status_monitor::{StatusMonitor, StatusUpdate};
use crate::ui::{self, App};
use crate::utils::{expand_array_job, get_all_job_ids_from_sacct, JobId};
use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute, queue,
    terminal::{
        disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
        EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::prelude::*;
use std::io::{self, stdout};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// lazyslurm - Real-time monitoring tool for SLURM jobs.
#[derive(Parser)]
#[command(name = "lazyslurm")]
#[command(version = "0.1.0")]
#[command(about = "Real-time monitoring tool for SLURM jobs")]
pub struct Cli {
    /// Job IDs to monitor. Supports array jobs: "8322" expands all subtasks,
    /// "8322_5" monitors only that specific subtask. If empty, auto-discovers
    /// all visible jobs from sacct and watches for new jobs.
    pub job_ids: Vec<String>,
    /// Editor to open log files (default: $VISUAL, $EDITOR, or vim)
    #[arg(long)]
    pub editor: Option<String>,
}

/// Resolve editor command: CLI flag > $VISUAL > $EDITOR > "vim"
fn resolve_editor(cli_editor: Option<&str>) -> String {
    if let Some(editor) = cli_editor {
        return editor.to_string();
    }
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string())
}

/// Handle the watch command.
pub fn handle_watch(job_id_strs: Vec<String>, editor: Option<&str>) -> Result<()> {
    let editor = resolve_editor(editor);
    let (job_ids, auto_discover) = if job_id_strs.is_empty() {
        println!("No job IDs provided. Fetching all visible jobs from sacct...");
        let all_jobs = get_all_job_ids_from_sacct();
        if all_jobs.is_empty() {
            println!("No jobs found in sacct. Will monitor for new jobs...");
        } else {
            println!(
                "Found {} job(s): {}",
                all_jobs.len(),
                all_jobs
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!("Auto-discovery enabled: new jobs will be automatically added to monitoring.");
        (all_jobs, true)
    } else {
        // Parse each argument and expand array jobs as needed
        let mut expanded: Vec<JobId> = Vec::new();
        for s in &job_id_strs {
            let parsed: JobId = s
                .parse()
                .with_context(|| format!("Invalid job ID: {}", s))?;

            if parsed.array_index.is_some() {
                // Explicit array index — monitor only this subtask
                expanded.push(parsed);
            } else {
                // No array index — try to expand all subtasks
                let subtasks = expand_array_job(parsed.base_id);
                expanded.extend(subtasks);
            }
        }

        expanded.sort_unstable();
        expanded.dedup();

        if expanded.len() > job_id_strs.len() {
            println!(
                "Expanded to {} subtask(s): {}",
                expanded.len(),
                expanded
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        (expanded, false)
    };

    run_monitor(job_ids, auto_discover, &editor)?;
    Ok(())
}

/// Run the monitor UI.
fn run_monitor(initial_job_ids: Vec<JobId>, auto_discover: bool, editor: &str) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new(editor.to_string());
    app.auto_discover = auto_discover;

    // Initialize jobs
    for &job_id in &initial_job_ids {
        app.add_job(job_id);
    }

    // Create channels for updates
    let (status_tx, status_rx) = mpsc::channel();
    let (log_tx, log_rx) = mpsc::channel();

    // Create job manager
    let job_manager = Arc::new(Mutex::new(JobManager::new()));
    for &job_id in &initial_job_ids {
        job_manager.lock().unwrap().add_tracked_job(job_id);
    }

    // Start status monitor
    let mut status_monitor = StatusMonitor::new(Arc::clone(&job_manager), 3.0);
    status_monitor.start_monitoring(initial_job_ids.clone(), status_tx);

    // Start log tailer
    let mut log_tailer = LogTailer::new(1.0);
    log_tailer.start_monitoring(log_tx.clone());

    // Add initial log files to monitor
    for &job_id in &initial_job_ids {
        let info = job_manager.lock().unwrap().get_job_info(job_id);
        if !info.stdout_path.as_os_str().is_empty() {
            log_tailer.add_file(&format!("stdout_{}", job_id), &info.stdout_path);
        }
        if !info.stderr_path.as_os_str().is_empty() {
            log_tailer.add_file(&format!("stderr_{}", job_id), &info.stderr_path);
        }
    }

    // Run event loop
    let result = run_event_loop(
        &mut terminal,
        &mut app,
        status_rx,
        log_rx,
        &job_manager,
        &log_tailer,
        &status_monitor,
    );

    // Cleanup
    status_monitor.stop_monitoring();
    log_tailer.stop_monitoring();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    )?;

    result
}

fn suspend_and_open_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    editor: &str,
    path: &std::path::Path,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    )?;
    terminal.clear()?;

    // Spawn editor
    let parts: Vec<&str> = editor.split_whitespace().collect();
    let (cmd, args) = match parts.split_first() {
        Some((c, a)) => (*c, a.to_vec()),
        None => anyhow::bail!("Empty editor command"),
    };

    let mut cmd_obj = Command::new(cmd);
    if !args.is_empty() {
        cmd_obj.args(&args);
    }
    cmd_obj.arg(path);

    let status = cmd_obj.status().context("Failed to execute editor")?;

    if !status.success() {
        // Editor exited with error, but still resume TUI
        eprintln!("Editor exited with error (press Enter to continue)");
    }

    // Resume TUI: re-enter alternate screen, hide cursor, enable raw mode
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture,
        Hide
    )?;

    // Clear the terminal to avoid stale content
    terminal.clear()?;

    Ok(())
}

/// Main event loop.
fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    status_rx: Receiver<StatusUpdate>,
    log_rx: Receiver<LogUpdate>,
    job_manager: &Arc<Mutex<JobManager>>,
    log_tailer: &LogTailer,
    status_monitor: &StatusMonitor,
) -> Result<()> {
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();
    let mut last_discovery = Instant::now();
    let discovery_interval = Duration::from_secs(10);

    loop {
        // Update panel heights using actual terminal size and layout calculations
        let size = terminal.size()?;
        let frame_area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
        app.update_panel_heights(frame_area);

        // Draw UI with synchronized update (prevents flicker in tmux and other terminals)
        {
            let backend = terminal.backend_mut();
            queue!(backend, BeginSynchronizedUpdate)?;
        }
        terminal.draw(|frame| {
            ui::render(frame, &mut *app);
        })?;
        {
            let backend = terminal.backend_mut();
            queue!(backend, EndSynchronizedUpdate)?;
            std::io::Write::flush(backend)?;
        }

        // Handle status updates (non-blocking)
        while let Ok(update) = status_rx.try_recv() {
            app.update_job_status(update.job_id, update.status, update.info.clone());

            // Add log files if we have paths now
            if !update.info.stdout_path.as_os_str().is_empty() {
                log_tailer.add_file(
                    &format!("stdout_{}", update.job_id),
                    &update.info.stdout_path,
                );
            }
            if !update.info.stderr_path.as_os_str().is_empty() {
                log_tailer.add_file(
                    &format!("stderr_{}", update.job_id),
                    &update.info.stderr_path,
                );
            }
        }

        // Handle log updates (non-blocking)
        while let Ok(update) = log_rx.try_recv() {
            debug_log(&format!("cli: received LogUpdate label={} content_len={}", update.label, update.content.len()));
            // Parse label to get job_id and log type
            // Label format: "stdout_8322" or "stdout_8322_5"
            // split_once('_') gives ("stdout", "8322") or ("stdout", "8322_5")
            if let Some((log_type, job_id_str)) = update.label.split_once('_') {
                if let Ok(job_id) = job_id_str.parse::<JobId>() {
                    debug_log(&format!("cli: updating log for job {} type {}", job_id, log_type));
                    app.update_log(job_id, log_type, &update.content);
                }
            }
        }

        // Auto-discover new jobs
        if app.auto_discover && last_discovery.elapsed() >= discovery_interval {
            last_discovery = Instant::now();
            let current_jobs: Vec<JobId> = app.jobs.keys().copied().collect();
            let all_jobs = get_all_job_ids_from_sacct();

            for job_id in all_jobs {
                // Skip jobs that are already tracked or were explicitly deleted by user
                if !current_jobs.contains(&job_id) && !app.deleted_jobs.contains(&job_id) {
                    // Fetch status and info immediately instead of waiting for poll cycle
                    let status = job_manager.lock().unwrap().get_job_status(job_id);
                    let info = job_manager.lock().unwrap().get_job_info(job_id);
                    app.update_job_status(job_id, status, info.clone());

                    job_manager.lock().unwrap().add_tracked_job(job_id);
                    status_monitor.add_job_to_monitor(job_id);

                    // Add log files if paths are available
                    if !info.stdout_path.as_os_str().is_empty() {
                        log_tailer.add_file(&format!("stdout_{}", job_id), &info.stdout_path);
                    }
                    if !info.stderr_path.as_os_str().is_empty() {
                        log_tailer.add_file(&format!("stderr_{}", job_id), &info.stderr_path);
                    }
                }
            }
        }

        // Handle input events
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if crossterm::event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') => {
                                if app.is_in_scroll_mode() {
                                    app.exit_scroll_mode();
                                } else {
                                    app.should_quit = true;
                                }
                            }
                            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                                app.should_quit = true;
                            }
                            KeyCode::Tab => {
                                app.switch_focus();
                            }
                            KeyCode::Char('n') => {
                                app.prev_job();
                            }
                            KeyCode::Char('p') => {
                                app.next_job();
                            }
                            KeyCode::Char('d') => {
                                if let Some(job_id) = app.current_job_id {
                                    status_monitor.remove_job_from_monitor(job_id);
                                    log_tailer.remove_file(&format!("stdout_{}", job_id));
                                    log_tailer.remove_file(&format!("stderr_{}", job_id));
                                    app.remove_current_job();
                                }
                            }
                            KeyCode::Char('l') => {
                                app.cycle_layout();
                            }
                            KeyCode::Up => {
                                app.scroll_up(1);
                            }
                            KeyCode::Down => {
                                app.scroll_down(1);
                            }
                            KeyCode::PageUp => {
                                app.scroll_up(10);
                            }
                            KeyCode::PageDown => {
                                app.scroll_down(10);
                            }
                            KeyCode::Home => {
                                app.scroll_to_top();
                            }
                            KeyCode::End => {
                                app.scroll_to_bottom();
                            }
                            KeyCode::Enter => {
                                if let Some(path) = app.get_focused_file_path() {
                                    if path.exists() {
                                        suspend_and_open_editor(terminal, &app.editor, &path)?;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                            if let Some(panel) = app.hit_test_panel(mouse.column, mouse.row) {
                                app.focused_panel = panel;
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            app.scroll_up(3);
                        }
                        MouseEventKind::ScrollDown => {
                            app.scroll_down(3);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
