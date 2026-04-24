//! Utility functions for SLURM command execution and output parsing.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt;
use std::process::Command;
use std::str::FromStr;

/// A SLURM job identifier, supporting both regular jobs (e.g. `8322`) and
/// array job tasks (e.g. `8322_5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId {
    pub base_id: u64,
    pub array_index: Option<u32>,
}

impl JobId {
    pub fn new(base_id: u64, array_index: Option<u32>) -> Self {
        Self {
            base_id,
            array_index,
        }
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.array_index {
            Some(idx) => write!(f, "{}_{}", self.base_id, idx),
            None => write!(f, "{}", self.base_id),
        }
    }
}

impl FromStr for JobId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        // Strip .batch / .extern / .0 suffixes (step identifiers)
        let base = s.split('.').next().unwrap_or(s);

        if let Some((left, right)) = base.split_once('_') {
            let base_id: u64 = left
                .parse()
                .with_context(|| format!("Invalid base job ID: {}", left))?;
            let array_index: u32 = right
                .parse()
                .with_context(|| format!("Invalid array index: {}", right))?;
            Ok(JobId::new(base_id, Some(array_index)))
        } else {
            let base_id: u64 = base
                .parse()
                .with_context(|| format!("Invalid job ID: {}", base))?;
            Ok(JobId::new(base_id, None))
        }
    }
}

impl Ord for JobId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.base_id
            .cmp(&other.base_id)
            .then(self.array_index.cmp(&other.array_index))
    }
}

impl PartialOrd for JobId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<u64> for JobId {
    fn from(id: u64) -> Self {
        Self {
            base_id: id,
            array_index: None,
        }
    }
}

/// Expand an array job into all its subtask `JobId`s by querying sacct.
///
/// If `base_id` is not an array job (or sacct fails), returns `vec![JobId::from(base_id)]`.
pub fn expand_array_job(base_id: u64) -> Vec<JobId> {
    let result = run_slurm_command(
        &[
            "sacct",
            "-j",
            &base_id.to_string(),
            "--format=JobID",
            "--noheader",
            "--parsable2",
        ],
        false,
    );

    match result {
        Ok(cmd_result) if cmd_result.return_code == 0 && !cmd_result.stdout.trim().is_empty() => {
            let mut ids: Vec<JobId> = cmd_result
                .stdout
                .trim()
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() {
                        return None;
                    }
                    // Strip step suffixes (.batch, .extern, .0)
                    let base = line.split('.').next()?;
                    // Only keep entries that have an array index
                    if base.contains('_') {
                        base.parse::<JobId>().ok()
                    } else {
                        None
                    }
                })
                .collect();

            ids.sort_unstable();
            ids.dedup();

            if ids.is_empty() {
                // Not an array job
                vec![JobId::from(base_id)]
            } else {
                ids
            }
        }
        _ => vec![JobId::from(base_id)],
    }
}

/// Result of running a SLURM command
#[derive(Debug)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub return_code: i32,
}

/// Execute a SLURM command and return stdout, stderr, and return code.
///
/// # Arguments
/// * `cmd` - Command and arguments as a slice
/// * `check` - If true, return error on non-zero return code
/// * `timeout_secs` - Command timeout in seconds (default: 30)
pub fn run_slurm_command(cmd: &[&str], check: bool) -> Result<CommandResult> {
    run_slurm_command_with_timeout(cmd, check, 30)
}

/// Execute a SLURM command with custom timeout.
pub fn run_slurm_command_with_timeout(
    cmd: &[&str],
    check: bool,
    _timeout_secs: u64,
) -> Result<CommandResult> {
    if cmd.is_empty() {
        anyhow::bail!("Empty command");
    }

    let output = Command::new(cmd[0])
        .args(&cmd[1..])
        .output()
        .with_context(|| format!("Failed to execute command: {}", cmd[0]))?;

    let result = CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        return_code: output.status.code().unwrap_or(-1),
    };

    if check && result.return_code != 0 {
        anyhow::bail!(
            "Command {:?} failed with code {}: {}",
            cmd,
            result.return_code,
            result.stderr
        );
    }

    Ok(result)
}

/// Parse squeue output for a single job.
#[allow(dead_code)]
pub fn parse_squeue_output(output: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let lines: Vec<&str> = output.trim().lines().collect();

    if lines.len() < 2 {
        return result;
    }

    // Skip header line
    let data_line = lines[1].trim();
    let parts: Vec<&str> = data_line.split_whitespace().collect();

    if parts.len() >= 4 {
        result.insert("job_id".to_string(), parts[0].to_string());
        result.insert("state".to_string(), parts.get(1).unwrap_or(&"UNKNOWN").to_string());
        result.insert("time".to_string(), parts.get(2).unwrap_or(&"").to_string());
        result.insert("nodes".to_string(), parts.get(3).unwrap_or(&"").to_string());
    }

    result
}

/// Parse sacct output for job information.
/// Handles multiple rows (main job + batch step) by merging data,
/// preferring non-empty values from batch steps for StdOut/StdErr.
pub fn parse_sacct_output(output: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let lines: Vec<&str> = output
        .trim()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if lines.len() < 2 {
        return result;
    }

    // Parse header
    let header: Vec<&str> = lines[0].split('|').collect();
    
    // First pass: populate from main job line (lines[1])
    let data: Vec<&str> = lines[1].split('|').collect();
    for (i, field) in header.iter().enumerate() {
        if i < data.len() {
            result.insert(field.trim().to_string(), data[i].trim().to_string());
        }
    }

    // Second pass: look for StdOut/StdErr in subsequent lines (batch steps like .0, .batch)
    // These fields are often only populated on the batch step, not the main job
    for line in lines.iter().skip(2) {
        let data: Vec<&str> = line.split('|').collect();
        for (i, field) in header.iter().enumerate() {
            let field_name = field.trim();
            if (field_name == "StdOut" || field_name == "StdErr") && i < data.len() {
                let value = data[i].trim();
                if !value.is_empty() {
                    // Found a non-empty StdOut/StdErr, use it
                    result.insert(field_name.to_string(), value.to_string());
                }
            }
        }
    }

    result
}

/// Parse sacct output for multiple jobs.
#[allow(dead_code)]
pub fn parse_sacct_multiple_output(output: &str) -> Vec<HashMap<String, String>> {
    let lines: Vec<&str> = output
        .trim()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    if lines.len() < 2 {
        return Vec::new();
    }

    // Parse header
    let header: Vec<&str> = lines[0].split('|').collect();

    // Parse all data lines
    lines[1..]
        .iter()
        .map(|line| {
            let data: Vec<&str> = line.split('|').collect();
            let mut result = HashMap::new();
            for (i, field) in header.iter().enumerate() {
                if i < data.len() {
                    result.insert(field.trim().to_string(), data[i].trim().to_string());
                }
            }
            result
        })
        .filter(|m| !m.is_empty())
        .collect()
}

/// Get all job IDs from sacct (recent jobs visible to the user).
///
/// Returns a vector of `JobId`s sorted in descending order, preserving array indices.
pub fn get_all_job_ids_from_sacct() -> Vec<JobId> {
    let result = run_slurm_command(
        &["sacct", "--format=JobID", "--noheader", "--parsable2"],
        false,
    );

    match result {
        Ok(cmd_result) if cmd_result.return_code == 0 && !cmd_result.stdout.trim().is_empty() => {
            let mut job_ids: Vec<JobId> = cmd_result
                .stdout
                .trim()
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() {
                        return None;
                    }
                    // Strip pipe-delimited fields (take first)
                    let id_str = line.split('|').next()?;
                    // Strip step suffixes (.batch, .extern)
                    let base = id_str.split('.').next()?;
                    // Skip bare step entries like "12345.batch" that resolve to just "12345"
                    // when we already have "12345" or "12345_N"
                    base.parse::<JobId>().ok()
                })
                .collect();

            // Remove duplicates and sort in descending order
            job_ids.sort_unstable();
            job_ids.dedup();
            job_ids.reverse();
            job_ids
        }
        _ => Vec::new(),
    }
}

/// Job status enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Unknown,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "QUEUED",
            JobStatus::Running => "RUNNING",
            JobStatus::Completed => "COMPLETED",
            JobStatus::Failed => "FAILED",
            JobStatus::Unknown => "UNKNOWN",
        }
    }

    pub fn from_slurm_state(state: &str) -> Self {
        let state_upper = state.to_uppercase();
        match state_upper.as_str() {
            "PENDING" | "CONFIGURING" => JobStatus::Queued,
            "RUNNING" | "COMPLETING" => JobStatus::Running,
            "COMPLETED" => JobStatus::Completed,
            "FAILED" | "CANCELLED" | "TIMEOUT" | "NODE_FAIL" | "PREEMPTED" | "OUT_OF_MEMORY" => {
                JobStatus::Failed
            }
            _ => {
                if state_upper.contains("COMPLETED") {
                    JobStatus::Completed
                } else if state_upper.contains("FAILED")
                    || state_upper.contains("CANCELLED")
                    || state_upper.contains("TIMEOUT")
                {
                    JobStatus::Failed
                } else if state_upper.contains("RUNNING") {
                    JobStatus::Running
                } else {
                    JobStatus::Unknown
                }
            }
        }
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sacct_output() {
        let output = "JobID|JobName|State\n12345|test_job|RUNNING\n";
        let result = parse_sacct_output(output);
        assert_eq!(result.get("JobID"), Some(&"12345".to_string()));
        assert_eq!(result.get("JobName"), Some(&"test_job".to_string()));
        assert_eq!(result.get("State"), Some(&"RUNNING".to_string()));
    }

    #[test]
    fn test_job_status_from_slurm_state() {
        assert_eq!(JobStatus::from_slurm_state("PENDING"), JobStatus::Queued);
        assert_eq!(JobStatus::from_slurm_state("RUNNING"), JobStatus::Running);
        assert_eq!(JobStatus::from_slurm_state("COMPLETED"), JobStatus::Completed);
        assert_eq!(JobStatus::from_slurm_state("FAILED"), JobStatus::Failed);
        assert_eq!(JobStatus::from_slurm_state("CANCELLED"), JobStatus::Failed);
    }

    #[test]
    fn test_job_id_display() {
        assert_eq!(JobId::from(8322).to_string(), "8322");
        assert_eq!(JobId::new(8322, Some(5)).to_string(), "8322_5");
        assert_eq!(JobId::new(8322, Some(0)).to_string(), "8322_0");
    }

    #[test]
    fn test_job_id_from_str() {
        assert_eq!("8322".parse::<JobId>().unwrap(), JobId::from(8322));
        assert_eq!(
            "8322_5".parse::<JobId>().unwrap(),
            JobId::new(8322, Some(5))
        );
        assert_eq!(
            "8322_0.batch".parse::<JobId>().unwrap(),
            JobId::new(8322, Some(0))
        );
        assert_eq!(
            "8322.extern".parse::<JobId>().unwrap(),
            JobId::from(8322)
        );
        assert!("abc".parse::<JobId>().is_err());
    }

    #[test]
    fn test_job_id_ordering() {
        let mut ids = vec![
            JobId::new(8322, Some(5)),
            JobId::from(8322),
            JobId::new(8322, Some(0)),
            JobId::from(1000),
            JobId::new(8322, Some(21)),
        ];
        ids.sort();
        assert_eq!(
            ids,
            vec![
                JobId::from(1000),
                JobId::from(8322),
                JobId::new(8322, Some(0)),
                JobId::new(8322, Some(5)),
                JobId::new(8322, Some(21)),
            ]
        );
    }
}
