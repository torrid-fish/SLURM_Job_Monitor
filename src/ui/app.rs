//! Application state management for the TUI.

use crate::job_manager::JobInfo;
use crate::utils::{JobId, JobStatus};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::TableState;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Which block is currently focused
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusBlock {
    JobList,
    Details,
    Stdout,
    Stderr,
}

impl FocusBlock {
    /// Which tab in the right panel this focus implies.
    pub fn right_tab(self) -> RightTab {
        match self {
            FocusBlock::Details => RightTab::Details,
            FocusBlock::Stdout | FocusBlock::Stderr => RightTab::Output,
            FocusBlock::JobList => RightTab::Output,
        }
    }
}

/// Tabs shown in the right-hand panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Details,
    Output,
}

/// Backwards-compatible alias for the legacy name.
pub type FocusedPanel = FocusBlock;

/// Layout mode for the TUI. Auto-selected based on terminal aspect ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Wider than tall: JobList left, output right (stdout above stderr).
    Horizontal,
    /// Taller than wide: JobList top, output below (stdout above stderr).
    Vertical,
}

impl LayoutMode {
    /// Pick a layout based on terminal width vs. height.
    ///
    /// Terminal cells are roughly twice as tall as they are wide, so we compare
    /// `width` to `height * 2` to get a perceptually-square threshold.
    pub fn auto(width: u16, height: u16) -> Self {
        if width as u32 >= (height as u32) * 2 {
            LayoutMode::Horizontal
        } else {
            LayoutMode::Vertical
        }
    }
}

impl FocusBlock {
    /// Cycle Tab key forward: JobList → Details → Stdout → Stderr → JobList.
    pub fn next(self) -> Self {
        match self {
            FocusBlock::JobList => FocusBlock::Details,
            FocusBlock::Details => FocusBlock::Stdout,
            FocusBlock::Stdout => FocusBlock::Stderr,
            FocusBlock::Stderr => FocusBlock::JobList,
        }
    }

    pub fn toggle(&mut self) {
        *self = self.next();
    }
}

/// Data for a single job
#[derive(Debug, Clone, Default)]
pub struct JobData {
    pub status: JobStatus,
    pub info: JobInfo,
    pub stdout: String,
    pub stderr: String,
    pub stdout_lines: Vec<String>,
    pub stderr_lines: Vec<String>,
    pub stdout_scroll: usize,
    pub stderr_scroll: usize,
    pub stdout_scroll_mode: bool,
    pub stderr_scroll_mode: bool,
}

impl JobData {
    pub fn new(job_id: JobId) -> Self {
        Self {
            status: JobStatus::Unknown,
            info: JobInfo {
                job_id,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Process log content to handle carriage returns (progress bars).
    /// Simulates terminal behavior: \r returns to line start, overwriting previous content.
    fn process_log_content(content: &str) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut current_line = String::new();

        for ch in content.chars() {
            match ch {
                '\r' => {
                    // Carriage return: reset to beginning of current line (don't push yet)
                    current_line.clear();
                }
                '\n' => {
                    // Newline: push current line and start fresh
                    lines.push(current_line.clone());
                    current_line.clear();
                }
                _ => {
                    current_line.push(ch);
                }
            }
        }

        // Don't forget any trailing content without a newline
        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }

    /// Update stdout content
    pub fn append_stdout(&mut self, content: &str, max_visible_lines: usize, panel_width: usize) {
        self.stdout.push_str(content);
        self.stdout_lines = Self::process_log_content(&self.stdout);

        if !self.stdout_scroll_mode {
            self.scroll_stdout_to_bottom(max_visible_lines, panel_width);
        }
    }

    pub fn append_stderr(&mut self, content: &str, max_visible_lines: usize, panel_width: usize) {
        self.stderr.push_str(content);
        self.stderr_lines = Self::process_log_content(&self.stderr);

        if !self.stderr_scroll_mode {
            self.scroll_stderr_to_bottom(max_visible_lines, panel_width);
        }
    }

    pub fn scroll_stdout_to_bottom(&mut self, max_visible_lines: usize, panel_width: usize) {
        let total = wrap_lines_count(&self.stdout_lines, panel_width);
        self.stdout_scroll = total.saturating_sub(max_visible_lines);
        self.stdout_scroll_mode = false;
    }

    pub fn scroll_stderr_to_bottom(&mut self, max_visible_lines: usize, panel_width: usize) {
        let total = wrap_lines_count(&self.stderr_lines, panel_width);
        self.stderr_scroll = total.saturating_sub(max_visible_lines);
        self.stderr_scroll_mode = false;
    }
}

/// Main application state
pub struct App {
    pub jobs: HashMap<JobId, JobData>,
    pub current_job_id: Option<JobId>,
    pub focused_panel: FocusedPanel,
    pub layout: LayoutMode,
    pub should_quit: bool,
    pub max_visible_lines: usize,
    pub stdout_panel_height: usize,
    pub stderr_panel_height: usize,
    pub stdout_panel_width: usize,
    pub stderr_panel_width: usize,
    pub auto_discover: bool,
    pub deleted_jobs: HashSet<JobId>,
    pub table_state: TableState,
    pub editor: String,
    pub stdout_panel_rect: Rect,
    pub stderr_panel_rect: Rect,
    pub joblist_panel_rect: Rect,
    pub details_panel_rect: Rect,
    pub tab_details_rect: Rect,
    pub tab_output_rect: Rect,
    pub right_panel_rect: Rect,
    pub details_scroll: u16,
    pub details_scroll_max: u16,
}

impl App {
    pub fn new(editor: String) -> Self {
        Self {
            jobs: HashMap::new(),
            current_job_id: None,
            focused_panel: FocusBlock::JobList,
            layout: LayoutMode::Horizontal,
            should_quit: false,
            max_visible_lines: 20,
            stdout_panel_height: 20,
            stderr_panel_height: 20,
            stdout_panel_width: 80,
            stderr_panel_width: 80,
            auto_discover: false,
            deleted_jobs: HashSet::new(),
            table_state: TableState::default(),
            editor,
            stdout_panel_rect: Rect::default(),
            stderr_panel_rect: Rect::default(),
            joblist_panel_rect: Rect::default(),
            details_panel_rect: Rect::default(),
            tab_details_rect: Rect::default(),
            tab_output_rect: Rect::default(),
            right_panel_rect: Rect::default(),
            details_scroll: 0,
            details_scroll_max: 0,
        }
    }

    /// Add a job to track.
    pub fn add_job(&mut self, job_id: JobId) {
        if !self.jobs.contains_key(&job_id) {
            self.jobs.insert(job_id, JobData::new(job_id));
        }
        if self.current_job_id.is_none() {
            self.current_job_id = Some(job_id);
        }
    }

    /// Remove a job from tracking.
    pub fn remove_job(&mut self, job_id: JobId) {
        self.jobs.remove(&job_id);
        // Track deleted jobs to prevent re-adding via auto-discovery
        self.deleted_jobs.insert(job_id);
        if self.current_job_id == Some(job_id) {
            self.current_job_id = self.get_sorted_job_ids().first().copied();
        }
    }

    /// Get sorted job IDs.
    ///
    /// Sorts by base_id descending, then array_index ascending (None before Some).
    /// This groups array tasks together under their parent.
    pub fn get_sorted_job_ids(&self) -> Vec<JobId> {
        let mut ids: Vec<JobId> = self.jobs.keys().copied().collect();
        ids.sort_unstable_by(|a, b| {
            b.base_id
                .cmp(&a.base_id)
                .then(b.array_index.cmp(&a.array_index))
        });
        ids
    }

    /// Update job status.
    pub fn update_job_status(&mut self, job_id: JobId, status: JobStatus, info: JobInfo) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            job.status = status;
            job.info = info;
        } else {
            let mut job_data = JobData::new(job_id);
            job_data.status = status;
            job_data.info = info;
            self.jobs.insert(job_id, job_data);
            if self.current_job_id.is_none() {
                self.current_job_id = Some(job_id);
            }
        }
    }

    /// Update log content.
    pub fn update_log(&mut self, job_id: JobId, log_type: &str, content: &str) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            match log_type {
                "stdout" => job.append_stdout(content, self.stdout_panel_height, self.stdout_panel_width),
                "stderr" => job.append_stderr(content, self.stderr_panel_height, self.stderr_panel_width),
                _ => {}
            }
        }
    }

    /// Switch focus between panels.
    pub fn switch_focus(&mut self) {
        self.focused_panel.toggle();
    }

    /// Get the file path of the currently focused log panel (stdout or stderr).
    pub fn get_focused_file_path(&self) -> Option<PathBuf> {
        let job_id = self.current_job_id?;
        let job = self.jobs.get(&job_id)?;
        let path = match self.focused_panel {
            FocusBlock::Stdout => &job.info.stdout_path,
            FocusBlock::Stderr => &job.info.stderr_path,
            FocusBlock::JobList | FocusBlock::Details => return None,
        };
        if path.as_os_str().is_empty() {
            None
        } else {
            Some(path.clone())
        }
    }

    /// Switch to next job.
    pub fn next_job(&mut self) {
        let ids = self.get_sorted_job_ids();
        if ids.is_empty() {
            return;
        }

        self.current_job_id = match self.current_job_id {
            Some(current) => {
                let idx = ids.iter().position(|&id| id == current).unwrap_or(0);
                Some(ids[(idx + 1) % ids.len()])
            }
            None => Some(ids[0]),
        };
    }

    /// Switch to previous job.
    pub fn prev_job(&mut self) {
        let ids = self.get_sorted_job_ids();
        if ids.is_empty() {
            return;
        }

        self.current_job_id = match self.current_job_id {
            Some(current) => {
                let idx = ids.iter().position(|&id| id == current).unwrap_or(0);
                Some(ids[(idx + ids.len() - 1) % ids.len()])
            }
            None => Some(ids[0]),
        };
    }

    /// Scroll the focused panel up.
    pub fn scroll_up(&mut self, lines: usize) {
        if self.focused_panel == FocusBlock::JobList {
            for _ in 0..lines {
                self.prev_job();
            }
            return;
        }
        if self.focused_panel == FocusBlock::Details {
            self.details_scroll = self.details_scroll.saturating_sub(lines as u16);
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details => {}
                    FocusBlock::Stdout => {
                        let visible_lines = self.stdout_panel_height;
                        let total = wrap_lines_count(&job.stdout_lines, self.stdout_panel_width);
                        let max_scroll = total.saturating_sub(visible_lines);
                        if max_scroll == 0 {
                            return;
                        }
                        let old_scroll = job.stdout_scroll;
                        job.stdout_scroll = job.stdout_scroll.saturating_sub(lines);
                        if job.stdout_scroll != old_scroll {
                            job.stdout_scroll_mode = true;
                        }
                    }
                    FocusBlock::Stderr => {
                        let visible_lines = self.stderr_panel_height;
                        let total = wrap_lines_count(&job.stderr_lines, self.stderr_panel_width);
                        let max_scroll = total.saturating_sub(visible_lines);
                        if max_scroll == 0 {
                            return;
                        }
                        let old_scroll = job.stderr_scroll;
                        job.stderr_scroll = job.stderr_scroll.saturating_sub(lines);
                        if job.stderr_scroll != old_scroll {
                            job.stderr_scroll_mode = true;
                        }
                    }
                }
            }
        }
    }

    /// Scroll the focused panel down.
    pub fn scroll_down(&mut self, lines: usize) {
        if self.focused_panel == FocusBlock::JobList {
            for _ in 0..lines {
                self.next_job();
            }
            return;
        }
        if self.focused_panel == FocusBlock::Details {
            self.details_scroll = (self.details_scroll + lines as u16).min(self.details_scroll_max);
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details => {}
                    FocusBlock::Stdout => {
                        let visible_lines = self.stdout_panel_height;
                        let total = wrap_lines_count(&job.stdout_lines, self.stdout_panel_width);
                        let max_scroll = total.saturating_sub(visible_lines);
                        if max_scroll == 0 {
                            return;
                        }
                        let old_scroll = job.stdout_scroll;
                        job.stdout_scroll = (job.stdout_scroll + lines).min(max_scroll);
                        if job.stdout_scroll != old_scroll {
                            job.stdout_scroll_mode = true;
                        } else if job.stdout_scroll == max_scroll {
                            job.stdout_scroll_mode = false;
                        }
                    }
                    FocusBlock::Stderr => {
                        let visible_lines = self.stderr_panel_height;
                        let total = wrap_lines_count(&job.stderr_lines, self.stderr_panel_width);
                        let max_scroll = total.saturating_sub(visible_lines);
                        if max_scroll == 0 {
                            return;
                        }
                        let old_scroll = job.stderr_scroll;
                        job.stderr_scroll = (job.stderr_scroll + lines).min(max_scroll);
                        if job.stderr_scroll != old_scroll {
                            job.stderr_scroll_mode = true;
                        } else if job.stderr_scroll == max_scroll {
                            job.stderr_scroll_mode = false;
                        }
                    }
                }
            }
        }
    }

    /// Scroll to top.
    pub fn scroll_to_top(&mut self) {
        if self.focused_panel == FocusBlock::Details {
            self.details_scroll = 0;
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details => {}
                    FocusBlock::Stdout => {
                        job.stdout_scroll = 0;
                        job.stdout_scroll_mode = true;
                    }
                    FocusBlock::Stderr => {
                        job.stderr_scroll = 0;
                        job.stderr_scroll_mode = true;
                    }
                }
            }
        }
    }

    /// Scroll to bottom (exit scroll mode).
    pub fn scroll_to_bottom(&mut self) {
        if self.focused_panel == FocusBlock::Details {
            self.details_scroll = self.details_scroll_max;
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details => {}
                    FocusBlock::Stdout => {
                        job.scroll_stdout_to_bottom(self.stdout_panel_height, self.stdout_panel_width);
                    }
                    FocusBlock::Stderr => {
                        job.scroll_stderr_to_bottom(self.stderr_panel_height, self.stderr_panel_width);
                    }
                }
            }
        }
    }

    /// Exit scroll mode for the focused panel.
    pub fn exit_scroll_mode(&mut self) {
        self.scroll_to_bottom();
    }

    /// Remove the current job.
    pub fn remove_current_job(&mut self) {
        if let Some(job_id) = self.current_job_id {
            self.remove_job(job_id);
        }
    }

    pub fn update_panel_heights(&mut self, frame_area: Rect) {
        // Auto-pick layout based on terminal aspect ratio.
        self.layout = LayoutMode::auto(frame_area.width, frame_area.height);

        // Reserve 1 row at the bottom for the brand mark.
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(frame_area);

        let body_area = main_chunks[0];

        let (joblist_rect, right_rect, output_dir) = match self.layout {
            LayoutMode::Horizontal => {
                let body_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                    .split(body_area);
                (body_chunks[0], body_chunks[1], Direction::Vertical)
            }
            LayoutMode::Vertical => {
                let body_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                    .split(body_area);
                (body_chunks[0], body_chunks[1], Direction::Vertical)
            }
        };

        self.joblist_panel_rect = joblist_rect;
        self.right_panel_rect = right_rect;

        // The tab strip lives inside the outer block's top border. Inner is the
        // outer rect shrunk by 1 cell on each side.
        let tab_content = Rect {
            x: right_rect.x.saturating_add(1),
            y: right_rect.y.saturating_add(1),
            width: right_rect.width.saturating_sub(2),
            height: right_rect.height.saturating_sub(2),
        };

        // Tab labels rendered on the top border. Layout: " Details │ Output "
        // starting at column right_rect.x + 2 (after the rounded corner + space).
        let details_label_w = " Details ".chars().count() as u16;
        let output_label_w = " Output ".chars().count() as u16;
        let details_x = right_rect.x.saturating_add(2);
        self.tab_details_rect = Rect {
            x: details_x,
            y: right_rect.y,
            width: details_label_w,
            height: 1,
        };
        self.tab_output_rect = Rect {
            x: details_x + details_label_w + 1, // +1 for the "│" separator
            y: right_rect.y,
            width: output_label_w,
            height: 1,
        };

        // Tab content rects depend on the active tab.
        match self.focused_panel.right_tab() {
            RightTab::Details => {
                self.details_panel_rect = tab_content;
                self.stdout_panel_rect = Rect::default();
                self.stderr_panel_rect = Rect::default();
            }
            RightTab::Output => {
                let _ = output_dir; // stdout/stderr are always stacked vertically now
                self.details_panel_rect = Rect::default();
                // The outer block's border is the right_rect's border itself,
                // so split tab_content directly into stdout / sep / stderr.
                let output_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(50),
                        Constraint::Length(1),
                        Constraint::Percentage(50),
                    ])
                    .split(tab_content);
                self.stdout_panel_rect = output_chunks[0];
                self.stderr_panel_rect = output_chunks[2];
            }
        }

        // Each section reserves 1 row inside for its own header label, so the
        // visible content area is one row shorter. No inner border on either
        // side now that the outer block draws the only border.
        self.stdout_panel_height = self.stdout_panel_rect.height.saturating_sub(1).max(1) as usize;
        self.stderr_panel_height = self.stderr_panel_rect.height.saturating_sub(1).max(1) as usize;
        self.stdout_panel_width = self.stdout_panel_rect.width.max(1) as usize;
        self.stderr_panel_width = self.stderr_panel_rect.width.max(1) as usize;

        self.max_visible_lines = self.stdout_panel_height;
    }

    pub fn hit_test_panel(&self, col: u16, row: u16) -> Option<FocusBlock> {
        use ratatui::layout::Position;
        let pos = Position::new(col, row);
        if self.tab_details_rect.contains(pos) {
            return Some(FocusBlock::Details);
        }
        if self.tab_output_rect.contains(pos) {
            // Default to stdout when clicking the Output tab title.
            return Some(if self.focused_panel == FocusBlock::Stderr {
                FocusBlock::Stderr
            } else {
                FocusBlock::Stdout
            });
        }
        if self.details_panel_rect.contains(pos) {
            Some(FocusBlock::Details)
        } else if self.stdout_panel_rect.contains(pos) {
            Some(FocusBlock::Stdout)
        } else if self.stderr_panel_rect.contains(pos) {
            Some(FocusBlock::Stderr)
        } else if self.joblist_panel_rect.contains(pos) {
            Some(FocusBlock::JobList)
        } else {
            None
        }
    }

    /// Given a click inside the joblist rect, compute which row's job was clicked.
    pub fn joblist_row_to_job(&self, col: u16, row: u16) -> Option<JobId> {
        use ratatui::layout::Position;
        let pos = Position::new(col, row);
        if !self.joblist_panel_rect.contains(pos) {
            return None;
        }
        let inner_top = self.joblist_panel_rect.y + 1; // skip border
        let header_row = inner_top; // header occupies one row
        if row <= header_row {
            return None;
        }
        let row_index = (row - header_row - 1) as usize;
        let ids = self.get_sorted_job_ids();
        ids.get(row_index).copied()
    }

    /// Check if current job is in scroll mode.
    pub fn is_in_scroll_mode(&self) -> bool {
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get(&job_id) {
                return match self.focused_panel {
                    FocusBlock::Stdout => job.stdout_scroll_mode,
                    FocusBlock::Stderr => job.stderr_scroll_mode,
                    FocusBlock::JobList | FocusBlock::Details => false,
                };
            }
        }
        false
    }
}

/// Count total visual lines after hard-wrapping at `max_width`.
pub fn wrap_lines_count(lines: &[String], max_width: usize) -> usize {
    if max_width == 0 {
        return lines.len();
    }
    lines.iter().map(|line| {
        if line.is_empty() {
            1
        } else {
            let char_count = line.chars().count();
            (char_count + max_width - 1) / max_width
        }
    }).sum()
}

/// Hard-wrap lines to fit within `max_width` characters.
pub fn wrap_lines(lines: &[String], max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return lines.to_vec();
    }
    lines.iter().flat_map(|line| {
        if line.is_empty() {
            vec![String::new()]
        } else {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() <= max_width {
                vec![line.clone()]
            } else {
                chars.chunks(max_width)
                    .map(|chunk| chunk.iter().collect())
                    .collect()
            }
        }
    }).collect()
}

impl Default for App {
    fn default() -> Self {
        Self::new(resolve_default_editor())
    }
}

fn resolve_default_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string())
}

impl Default for JobStatus {
    fn default() -> Self {
        JobStatus::Unknown
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self {
            base_id: 0,
            array_index: None,
        }
    }
}
