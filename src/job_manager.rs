//! Job Manager for SLURM job lifecycle management.

use crate::utils::{parse_job_id, parse_sacct_output, run_slurm_command, JobId, JobStatus};

/// Write debug message to file
fn debug_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/slurm_monitor_debug.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Information about a SLURM job
#[derive(Debug, Clone, Default)]
pub struct JobInfo {
    #[allow(dead_code)]
    pub job_id: JobId,
    pub job_name: String,
    pub state: String,
    pub start_time: String,
    pub end_time: String,
    pub elapsed: String,
    pub work_dir: PathBuf,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

/// Manages SLURM job submission, tracking, and status retrieval.
#[derive(Debug, Default)]
pub struct JobManager {
    tracked_jobs: HashMap<JobId, HashMap<String, String>>,
}

impl JobManager {
    /// Create a new JobManager instance.
    pub fn new() -> Self {
        Self {
            tracked_jobs: HashMap::new(),
        }
    }

    /// Submit a job using sbatch and return the job ID.
    ///
    /// # Arguments
    /// * `sbatch_script` - Path to the SLURM batch script
    /// * `extra_args` - Additional arguments to pass to sbatch
    pub fn submit_job(&mut self, sbatch_script: &Path, extra_args: &[String]) -> Result<u64> {
        if !sbatch_script.exists() {
            anyhow::bail!("Script not found: {}", sbatch_script.display());
        }

        let mut cmd_args = vec!["sbatch"];
        let extra_args_refs: Vec<&str> = extra_args.iter().map(|s| s.as_str()).collect();
        cmd_args.extend(extra_args_refs);
        cmd_args.push(sbatch_script.to_str().unwrap_or(""));

        let result = run_slurm_command(&cmd_args, true)
            .with_context(|| format!("Failed to submit job: {}", sbatch_script.display()))?;

        let job_id = parse_job_id(&result.stdout)
            .ok_or_else(|| anyhow::anyhow!("Could not parse job ID from sbatch output"))?;

        // Track the job
        let mut metadata = HashMap::new();
        metadata.insert(
            "script".to_string(),
            sbatch_script.to_string_lossy().to_string(),
        );
        metadata.insert("submitted".to_string(), "true".to_string());
        self.tracked_jobs.insert(JobId::from(job_id), metadata);

        Ok(job_id)
    }

    /// Get the current status of a job.
    pub fn get_job_status(&self, job_id: JobId) -> JobStatus {
        let id_str = job_id.to_string();

        // First try squeue for active jobs
        let result = run_slurm_command(
            &["squeue", "-j", &id_str, "-h", "-o", "%T"],
            false,
        );

        if let Ok(cmd_result) = result {
            if cmd_result.return_code == 0 && !cmd_result.stdout.trim().is_empty() {
                let state = cmd_result.stdout.trim().to_uppercase();
                return JobStatus::from_slurm_state(&state);
            }
        }

        // If not in squeue, check sacct for completed/failed jobs
        let result = run_slurm_command(
            &[
                "sacct",
                "-j",
                &id_str,
                "--format=State",
                "--noheader",
                "--parsable2",
            ],
            false,
        );

        if let Ok(cmd_result) = result {
            if cmd_result.return_code == 0 && !cmd_result.stdout.trim().is_empty() {
                let state = cmd_result
                    .stdout
                    .trim()
                    .split('|')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_uppercase();
                return JobStatus::from_slurm_state(&state);
            }
        }

        JobStatus::Unknown
    }

    /// Get detailed information about a job including output paths.
    pub fn get_job_info(&self, job_id: JobId) -> JobInfo {
        let mut info = JobInfo {
            job_id,
            ..Default::default()
        };

        let id_str = job_id.to_string();

        // Use sacct to get comprehensive job information
        let result = run_slurm_command(
            &[
                "sacct",
                "-j",
                &id_str,
                "--format=JobID,JobName,State,Start,End,Elapsed,WorkDir,StdOut,StdErr",
                "--parsable2",
            ],
            false,
        );

        if let Ok(cmd_result) = result {
            if cmd_result.return_code == 0 && !cmd_result.stdout.trim().is_empty() {
                let parsed = parse_sacct_output(&cmd_result.stdout);

                info.job_name = parsed.get("JobName").cloned().unwrap_or_default();
                info.state = parsed.get("State").cloned().unwrap_or_else(|| "UNKNOWN".to_string());
                info.start_time = parsed.get("Start").cloned().unwrap_or_default();
                info.end_time = parsed.get("End").cloned().unwrap_or_default();
                info.elapsed = parsed.get("Elapsed").cloned().unwrap_or_default();

                let work_dir = parsed.get("WorkDir").cloned().unwrap_or_default();
                info.work_dir = PathBuf::from(&work_dir);

                // Get and process stdout path
                let stdout_path = parsed.get("StdOut").cloned().unwrap_or_default();
                info.stdout_path = self.resolve_output_path(&stdout_path, job_id, &work_dir);

                // Get and process stderr path
                let stderr_path = parsed.get("StdErr").cloned().unwrap_or_default();
                info.stderr_path = self.resolve_output_path(&stderr_path, job_id, &work_dir);

                debug_log(&format!("get_job_info: job_id={} stdout={} stderr={}", job_id, info.stdout_path.display(), info.stderr_path.display()));

                return info;
            }
        }

        // Fallback: try to construct paths from common patterns
        let cwd = std::env::current_dir().unwrap_or_default();
        info.work_dir = cwd.clone();
        info.stdout_path = self.find_output_file(&cwd, job_id, "out");
        info.stderr_path = self.find_output_file(&cwd, job_id, "err");

        info
    }

    /// Resolve output path, replacing SLURM placeholders.
    fn resolve_output_path(&self, path: &str, job_id: JobId, work_dir: &str) -> PathBuf {
        if path.is_empty() {
            return PathBuf::new();
        }

        // Replace SLURM placeholders
        let array_index_str = job_id
            .array_index
            .map(|i| i.to_string())
            .unwrap_or_else(|| "0".to_string());

        let resolved = path
            .replace("%j", &job_id.to_string())
            .replace("%A", &job_id.base_id.to_string())
            .replace("%a", &array_index_str);

        let path = PathBuf::from(&resolved);

        // Make path absolute if relative
        if path.is_absolute() {
            path
        } else if !work_dir.is_empty() {
            PathBuf::from(work_dir).join(&path)
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(&path)
        }
    }

    /// Find output file using common naming patterns.
    fn find_output_file(&self, dir: &Path, job_id: JobId, ext: &str) -> PathBuf {
        // Try exact match with full job ID (e.g. slurm-8322_5.out or slurm-8322.out)
        let exact = dir.join(format!("slurm-{}.{}", job_id, ext));
        if exact.exists() {
            return exact;
        }

        // For array jobs, also try base_id pattern
        if job_id.array_index.is_some() {
            let base_pattern = dir.join(format!("slurm-{}.{}", job_id.base_id, ext));
            if base_pattern.exists() {
                return base_pattern;
            }
        }

        // For non-array jobs, try array pattern with index 0
        if job_id.array_index.is_none() {
            let array = dir.join(format!("slurm-{}_{}.{}", job_id.base_id, 0, ext));
            if array.exists() {
                return array;
            }
        }

        // Return exact pattern even if it doesn't exist yet
        exact
    }

    /// List all currently tracked job IDs.
    #[allow(dead_code)]
    pub fn list_tracked_jobs(&self) -> Vec<JobId> {
        self.tracked_jobs.keys().copied().collect()
    }

    /// Add a job to the tracking list.
    pub fn add_tracked_job(&mut self, job_id: JobId) {
        self.tracked_jobs.entry(job_id).or_insert_with(HashMap::new);
    }

    /// Remove a job from the tracking list.
    #[allow(dead_code)]
    pub fn remove_tracked_job(&mut self, job_id: JobId) {
        self.tracked_jobs.remove(&job_id);
    }

    /// Check if a job is being tracked.
    #[allow(dead_code)]
    pub fn is_tracking(&self, job_id: JobId) -> bool {
        self.tracked_jobs.contains_key(&job_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_manager_new() {
        let manager = JobManager::new();
        assert!(manager.list_tracked_jobs().is_empty());
    }

    #[test]
    fn test_add_remove_tracked_job() {
        let mut manager = JobManager::new();
        let job_id = JobId::from(12345);

        manager.add_tracked_job(job_id);
        assert!(manager.is_tracking(job_id));
        assert_eq!(manager.list_tracked_jobs().len(), 1);

        manager.remove_tracked_job(job_id);
        assert!(!manager.is_tracking(job_id));
        assert!(manager.list_tracked_jobs().is_empty());
    }

    #[test]
    fn test_add_remove_array_job() {
        let mut manager = JobManager::new();
        let job_id = JobId::new(8322, Some(5));

        manager.add_tracked_job(job_id);
        assert!(manager.is_tracking(job_id));
        // A different array index should not be tracked
        assert!(!manager.is_tracking(JobId::new(8322, Some(6))));
    }

    #[test]
    fn test_resolve_output_path() {
        let manager = JobManager::new();

        // Test placeholder replacement for non-array job
        let resolved = manager.resolve_output_path("slurm-%j.out", JobId::from(12345), "/home/user");
        assert!(resolved.to_string_lossy().contains("slurm-12345.out"));

        // Test placeholder replacement for array job
        let resolved = manager.resolve_output_path(
            "slurm-%A_%a.out",
            JobId::new(8322, Some(5)),
            "/home/user",
        );
        assert!(resolved.to_string_lossy().contains("slurm-8322_5.out"));
    }
}
