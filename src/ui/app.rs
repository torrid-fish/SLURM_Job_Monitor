//! Application state management for the TUI.

use crate::job_manager::JobInfo;
use crate::partition_monitor::{PartitionInfo, PendingJob};
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
    /// System-wide partition / queue overview panel (third panel).
    Partitions,
}

impl FocusBlock {
    /// Which tab in the right panel this focus implies.
    pub fn right_tab(self) -> RightTab {
        match self {
            FocusBlock::Details => RightTab::Details,
            FocusBlock::Stdout | FocusBlock::Stderr => RightTab::Output,
            FocusBlock::JobList | FocusBlock::Partitions => RightTab::Output,
        }
    }
}

/// Tabs shown in the right-hand panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightTab {
    Details,
    Output,
}

/// Sort state for a clickable table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortState {
    pub column: usize,
    pub ascending: bool,
}

/// Backwards-compatible alias for the legacy name.
pub type FocusedPanel = FocusBlock;


impl FocusBlock {
    /// Cycle Tab key forward.
    /// JobList → Details → Stdout → Stderr → Partitions → JobList.
    pub fn next(self) -> Self {
        match self {
            FocusBlock::JobList => FocusBlock::Details,
            FocusBlock::Details => FocusBlock::Stdout,
            FocusBlock::Stdout => FocusBlock::Stderr,
            FocusBlock::Stderr => FocusBlock::Partitions,
            FocusBlock::Partitions => FocusBlock::JobList,
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
    pub zoomed: bool,
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

    // Partitions panel (third panel) — system-wide overview.
    pub partitions: Vec<PartitionInfo>,
    pub partition_total_running: usize,
    pub partition_total_pending: usize,
    pub partition_table_state: TableState,
    pub partitions_panel_rect: Rect,
    /// True when the panel is hidden because the terminal is too short.
    pub partitions_collapsed: bool,

    /// Drill-down: when `Some`, the panel shows pending jobs on this partition
    /// instead of the partition list. Esc/Backspace returns to the list.
    pub partition_queue_for: Option<String>,
    pub partition_queue: Vec<PendingJob>,
    pub partition_queue_state: TableState,

    // Sort state + click-rects (per column header) for the clickable tables.
    pub partition_sort: SortState,
    pub partition_queue_sort: SortState,
    pub joblist_sort: SortState,
    pub partition_header_rects: Vec<Rect>,
    pub partition_queue_header_rects: Vec<Rect>,
    pub joblist_header_rects: Vec<Rect>,
}

impl App {
    pub fn new(editor: String) -> Self {
        Self {
            jobs: HashMap::new(),
            current_job_id: None,
            focused_panel: FocusBlock::JobList,
            zoomed: false,
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
            partitions: Vec::new(),
            partition_total_running: 0,
            partition_total_pending: 0,
            partition_table_state: TableState::default(),
            partitions_panel_rect: Rect::default(),
            partitions_collapsed: false,
            partition_queue_for: None,
            partition_queue: Vec::new(),
            partition_queue_state: TableState::default(),
            // Defaults match the original sort orders so behavior doesn't
            // change until the user clicks a different header.
            partition_sort: SortState { column: 3, ascending: true }, // Min-Wait asc
            partition_queue_sort: SortState { column: 3, ascending: false }, // Prio desc
            joblist_sort: SortState { column: 0, ascending: false }, // Job ID desc (newest first)
            partition_header_rects: Vec::new(),
            partition_queue_header_rects: Vec::new(),
            joblist_header_rects: Vec::new(),
        }
    }

    /// Click on a partition table header: toggle direction if same column,
    /// otherwise switch to that column with a sensible default direction.
    pub fn cycle_partition_sort(&mut self, column: usize) {
        if self.partition_sort.column == column {
            self.partition_sort.ascending = !self.partition_sort.ascending;
        } else {
            // Numeric / wait columns default desc-feeling, but Min-Wait wants asc.
            let asc = matches!(column, 0 | 3 | 4); // Partition, Min-Wait, States
            self.partition_sort = SortState { column, ascending: asc };
        }
        self.resort_partitions();
    }

    pub fn cycle_partition_queue_sort(&mut self, column: usize) {
        if self.partition_queue_sort.column == column {
            self.partition_queue_sort.ascending = !self.partition_queue_sort.ascending;
        } else {
            // Priority/Nodes/CPUs default descending, others ascending.
            let desc = matches!(column, 3 | 5 | 6);
            self.partition_queue_sort = SortState {
                column,
                ascending: !desc,
            };
        }
        self.resort_partition_queue();
    }

    pub fn resort_partitions(&mut self) {
        let s = self.partition_sort;
        self.partitions.sort_by(|a, b| {
            let ord = match s.column {
                0 => a.name.cmp(&b.name),
                1 => a
                    .free_nodes
                    .cmp(&b.free_nodes)
                    .then(a.total_nodes.cmp(&b.total_nodes)),
                2 => a.pending_jobs.cmp(&b.pending_jobs),
                3 => {
                    fn rank(p: &PartitionInfo) -> (u8, i64) {
                        if p.is_down {
                            (3, 0)
                        } else {
                            match p.min_wait_secs {
                                Some(0) => (0, 0),
                                Some(s) => (1, s),
                                None => (2, 0),
                            }
                        }
                    }
                    rank(a).cmp(&rank(b))
                }
                4 => a.states.cmp(&b.states),
                _ => std::cmp::Ordering::Equal,
            };
            // Tie-break by name for stability.
            let ord = ord.then_with(|| a.name.cmp(&b.name));
            if s.ascending {
                ord
            } else {
                ord.reverse()
            }
        });
    }

    pub fn resort_partition_queue(&mut self) {
        let s = self.partition_queue_sort;
        self.partition_queue.sort_by(|a, b| {
            let ord = match s.column {
                0 => parse_int_prefix(&a.job_id).cmp(&parse_int_prefix(&b.job_id)),
                1 => a.user.cmp(&b.user),
                2 => a.name.cmp(&b.name),
                3 => {
                    let pa: i64 = a.priority.parse().unwrap_or(0);
                    let pb: i64 = b.priority.parse().unwrap_or(0);
                    pa.cmp(&pb)
                }
                4 => a.time_limit.cmp(&b.time_limit),
                5 => {
                    let na: i64 = a.nodes.parse().unwrap_or(0);
                    let nb: i64 = b.nodes.parse().unwrap_or(0);
                    na.cmp(&nb)
                }
                6 => {
                    let ca: i64 = a.cpus.parse().unwrap_or(0);
                    let cb: i64 = b.cpus.parse().unwrap_or(0);
                    ca.cmp(&cb)
                }
                7 => a.reason.cmp(&b.reason),
                _ => std::cmp::Ordering::Equal,
            };
            let ord = ord.then_with(|| a.job_id.cmp(&b.job_id));
            if s.ascending {
                ord
            } else {
                ord.reverse()
            }
        });
    }

    /// Hit-test the partition / queue header row. Returns the column index that
    /// was clicked, if any.
    pub fn hit_test_partition_header(&self, col: u16, row: u16) -> Option<usize> {
        use ratatui::layout::Position;
        let pos = Position::new(col, row);
        let rects = if self.in_partition_queue_mode() {
            &self.partition_queue_header_rects
        } else {
            &self.partition_header_rects
        };
        rects.iter().position(|r| r.contains(pos))
    }

    /// Name of the currently-highlighted partition row, if any.
    pub fn selected_partition_name(&self) -> Option<&str> {
        let idx = self.partition_table_state.selected()?;
        self.partitions.get(idx).map(|p| p.name.as_str())
    }

    /// Whether the partitions panel is showing the pending-job drill-down.
    pub fn in_partition_queue_mode(&self) -> bool {
        self.partition_queue_for.is_some()
    }

    /// Enter drill-down for a partition (replaces the list with pending jobs).
    pub fn open_partition_queue(&mut self, partition: String, jobs: Vec<PendingJob>) {
        self.partition_queue_for = Some(partition);
        self.partition_queue = jobs;
        self.partition_queue_state.select(if self.partition_queue.is_empty() {
            None
        } else {
            Some(0)
        });
    }

    /// Replace the queue contents in place (used by background refresh).
    pub fn refresh_partition_queue(&mut self, jobs: Vec<PendingJob>) {
        let prev_sel = self.partition_queue_state.selected();
        self.partition_queue = jobs;
        let len = self.partition_queue.len();
        let new_sel = match (prev_sel, len) {
            (_, 0) => None,
            (Some(i), n) => Some(i.min(n - 1)),
            (None, _) => Some(0),
        };
        self.partition_queue_state.select(new_sel);
    }

    pub fn close_partition_queue(&mut self) {
        self.partition_queue_for = None;
        self.partition_queue.clear();
        self.partition_queue_state.select(None);
    }

    pub fn partition_scroll_up(&mut self, lines: usize) {
        if self.in_partition_queue_mode() {
            let len = self.partition_queue.len();
            if len == 0 {
                return;
            }
            let cur = self.partition_queue_state.selected().unwrap_or(0);
            let new = cur.saturating_sub(lines);
            self.partition_queue_state.select(Some(new.min(len - 1)));
            return;
        }
        let len = self.partitions.len();
        if len == 0 {
            return;
        }
        let cur = self.partition_table_state.selected().unwrap_or(0);
        let new = cur.saturating_sub(lines);
        self.partition_table_state.select(Some(new.min(len - 1)));
    }

    pub fn partition_scroll_down(&mut self, lines: usize) {
        if self.in_partition_queue_mode() {
            let len = self.partition_queue.len();
            if len == 0 {
                return;
            }
            let cur = self.partition_queue_state.selected().unwrap_or(0);
            let new = (cur + lines).min(len - 1);
            self.partition_queue_state.select(Some(new));
            return;
        }
        let len = self.partitions.len();
        if len == 0 {
            return;
        }
        let cur = self.partition_table_state.selected().unwrap_or(0);
        let new = (cur + lines).min(len - 1);
        self.partition_table_state.select(Some(new));
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

    /// Get sorted job IDs according to the current `joblist_sort`.
    /// Columns: 0=Job ID, 1=Status, 2=Runtime, 3=Limit, 4=Node, 5=Name.
    pub fn get_sorted_job_ids(&self) -> Vec<JobId> {
        let mut ids: Vec<JobId> = self.jobs.keys().copied().collect();
        let s = self.joblist_sort;
        ids.sort_by(|a, b| {
            let ja = self.jobs.get(a);
            let jb = self.jobs.get(b);
            let ord = match s.column {
                0 => a
                    .base_id
                    .cmp(&b.base_id)
                    .then(a.array_index.cmp(&b.array_index)),
                1 => {
                    let sa = ja.map(|j| j.status.as_str()).unwrap_or("");
                    let sb = jb.map(|j| j.status.as_str()).unwrap_or("");
                    sa.cmp(sb)
                }
                2 => {
                    let ea = ja
                        .and_then(|j| parse_slurm_duration(&j.info.elapsed))
                        .unwrap_or(-1);
                    let eb = jb
                        .and_then(|j| parse_slurm_duration(&j.info.elapsed))
                        .unwrap_or(-1);
                    ea.cmp(&eb)
                }
                3 => {
                    let la = ja
                        .and_then(|j| parse_slurm_duration(&j.info.time_limit))
                        .unwrap_or(-1);
                    let lb = jb
                        .and_then(|j| parse_slurm_duration(&j.info.time_limit))
                        .unwrap_or(-1);
                    la.cmp(&lb)
                }
                4 => {
                    let na = ja.map(|j| j.info.node_list.as_str()).unwrap_or("");
                    let nb = jb.map(|j| j.info.node_list.as_str()).unwrap_or("");
                    na.cmp(nb)
                }
                5 => {
                    let na = ja.map(|j| j.info.job_name.as_str()).unwrap_or("");
                    let nb = jb.map(|j| j.info.job_name.as_str()).unwrap_or("");
                    na.cmp(nb)
                }
                _ => std::cmp::Ordering::Equal,
            };
            // Tie-break by Job ID for stability.
            let ord = ord.then_with(|| {
                a.base_id
                    .cmp(&b.base_id)
                    .then(a.array_index.cmp(&b.array_index))
            });
            if s.ascending {
                ord
            } else {
                ord.reverse()
            }
        });
        ids
    }

    pub fn cycle_joblist_sort(&mut self, column: usize) {
        if self.joblist_sort.column == column {
            self.joblist_sort.ascending = !self.joblist_sort.ascending;
        } else {
            // Numeric/time columns default desc; textual columns default asc.
            let desc = matches!(column, 0 | 2 | 3);
            self.joblist_sort = SortState {
                column,
                ascending: !desc,
            };
        }
    }

    pub fn hit_test_joblist_header(&self, col: u16, row: u16) -> Option<usize> {
        use ratatui::layout::Position;
        let pos = Position::new(col, row);
        self.joblist_header_rects
            .iter()
            .position(|r| r.contains(pos))
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
            FocusBlock::JobList | FocusBlock::Details | FocusBlock::Partitions => return None,
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
        if self.focused_panel == FocusBlock::Partitions {
            self.partition_scroll_up(lines);
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details | FocusBlock::Partitions => {}
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
        if self.focused_panel == FocusBlock::Partitions {
            self.partition_scroll_down(lines);
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details | FocusBlock::Partitions => {}
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
        if self.focused_panel == FocusBlock::Partitions {
            self.partition_table_state.select(if self.partitions.is_empty() {
                None
            } else {
                Some(0)
            });
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details | FocusBlock::Partitions => {}
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
        if self.focused_panel == FocusBlock::Partitions {
            if !self.partitions.is_empty() {
                self.partition_table_state
                    .select(Some(self.partitions.len() - 1));
            }
            return;
        }
        if let Some(job_id) = self.current_job_id {
            if let Some(job) = self.jobs.get_mut(&job_id) {
                match self.focused_panel {
                    FocusBlock::JobList | FocusBlock::Details | FocusBlock::Partitions => {}
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
        // Reserve 1 row at the bottom for the brand mark.
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(frame_area);

        let body_area = main_chunks[0];

        // Decide partitions-panel height (auto-collapse when terminal is short).
        // Need at least 20 rows of body before reserving anything for partitions.
        // Otherwise the per-job panels become unusable.
        const COLLAPSE_BELOW: u16 = 20;
        let part_panel_h: u16 = if body_area.height < COLLAPSE_BELOW {
            0
        } else if self.in_partition_queue_mode() {
            // Queue drill-down wants more vertical space.
            let data_rows = self.partition_queue.len() as u16;
            let desired = (data_rows + 3).clamp(8, 20);
            desired.min(body_area.height / 2)
        } else {
            // border (top+bot) + header + N data rows; cap at 10.
            let data_rows = self.partitions.len() as u16;
            let desired = (data_rows + 3).clamp(5, 10);
            // Don't take more than ~⅓ of the body.
            desired.min(body_area.height / 3)
        };
        self.partitions_collapsed = part_panel_h == 0;

        // Layout: JobList(top) + Right(middle) + Partitions(bottom, optional).
        // When zoomed, only the panel containing the focused block fills the body.
        let (joblist_rect, right_rect, partitions_rect) = if self.zoomed {
            match self.focused_panel {
                FocusBlock::JobList => (body_area, Rect::default(), Rect::default()),
                FocusBlock::Partitions if part_panel_h > 0 => {
                    (Rect::default(), Rect::default(), body_area)
                }
                FocusBlock::Partitions => {
                    // Collapsed but focused — show it anyway when zoomed so the
                    // user has a way to actually see it on tiny terminals.
                    (Rect::default(), Rect::default(), body_area)
                }
                _ => (Rect::default(), body_area, Rect::default()),
            }
        } else if part_panel_h == 0 {
            // Collapsed: original two-panel layout (JobList on top, info below).
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                .split(body_area);
            (chunks[0], chunks[1], Rect::default())
        } else {
            // Three-panel layout: Partitions on top, then JobList, then Right.
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(part_panel_h),
                    Constraint::Percentage(25),
                    Constraint::Min(0),
                ])
                .split(body_area);
            (chunks[1], chunks[2], chunks[0])
        };

        self.joblist_panel_rect = joblist_rect;
        self.right_panel_rect = right_rect;
        self.partitions_panel_rect = partitions_rect;

        // If the right panel is collapsed (zoomed JobList), skip its layout.
        if right_rect.width == 0 || right_rect.height == 0 {
            self.tab_details_rect = Rect::default();
            self.tab_output_rect = Rect::default();
            self.details_panel_rect = Rect::default();
            self.stdout_panel_rect = Rect::default();
            self.stderr_panel_rect = Rect::default();
            self.stdout_panel_height = 1;
            self.stderr_panel_height = 1;
            self.stdout_panel_width = 1;
            self.stderr_panel_width = 1;
            self.max_visible_lines = 1;
            return;
        }

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
        } else if self.partitions_panel_rect.contains(pos) {
            Some(FocusBlock::Partitions)
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
                    FocusBlock::JobList | FocusBlock::Details | FocusBlock::Partitions => false,
                };
            }
        }
        false
    }
}

/// Parse a SLURM duration string (`D-HH:MM:SS`, `HH:MM:SS`, `MM:SS`).
/// Returns `None` for empty / unparseable values (e.g. `UNLIMITED`).
fn parse_slurm_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || matches!(s, "UNLIMITED" | "INVALID" | "N/A" | "Partition_Limit") {
        return None;
    }
    let (days, rest) = if let Some((d, r)) = s.split_once('-') {
        (d.parse::<i64>().ok()?, r)
    } else {
        (0, s)
    };
    let parts: Vec<i64> = rest.split(':').filter_map(|p| p.parse().ok()).collect();
    let (h, m, sec) = match parts.len() {
        3 => (parts[0], parts[1], parts[2]),
        2 => (0, parts[0], parts[1]),
        1 => (0, parts[0], 0),
        _ => return None,
    };
    Some(days * 86400 + h * 3600 + m * 60 + sec)
}

/// Parse the leading integer of a string. Used to sort job IDs numerically
/// while preserving the array-task suffix order (`8322_1` < `8322_10`).
fn parse_int_prefix(s: &str) -> (u64, String) {
    let mut n = 0u64;
    let mut i = 0;
    for ch in s.chars() {
        if let Some(d) = ch.to_digit(10) {
            n = n * 10 + d as u64;
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    (n, s[i..].to_string())
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
