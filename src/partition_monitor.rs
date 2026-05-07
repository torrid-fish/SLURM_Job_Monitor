//! Background poller that summarises SLURM partition usage and the minimum
//! wait time before resources free up. Mirrors the logic of the standalone
//! `partition-wait` script but emits structured updates over an mpsc channel.

use crate::utils::run_slurm_command;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// One pending job on a partition (drill-down view).
#[derive(Debug, Clone, Default)]
pub struct PendingJob {
    pub job_id: String,
    pub user: String,
    pub name: String,
    pub priority: String,
    pub time_limit: String,
    #[allow(dead_code)]
    pub submit_time: String,
    pub reason: String,
    pub nodes: String,
    pub cpus: String,
}

/// Per-partition snapshot.
#[derive(Debug, Clone, Default)]
pub struct PartitionInfo {
    pub name: String,
    pub total_nodes: usize,
    pub free_nodes: usize,
    pub pending_jobs: usize,
    /// Minimum wait in seconds; `None` means unknown (down/drain only).
    pub min_wait_secs: Option<i64>,
    /// True if the partition is administratively down.
    pub is_down: bool,
    /// State counts e.g. "idle:3, mix:1".
    pub states: String,
}

/// One push from the background thread to the UI.
#[derive(Debug, Clone, Default)]
pub struct PartitionUpdate {
    pub partitions: Vec<PartitionInfo>,
    pub total_running: usize,
    pub total_pending: usize,
}

pub struct PartitionMonitor {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    interval: Duration,
}

impl PartitionMonitor {
    pub fn new(interval_secs: f64) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            handle: None,
            interval: Duration::from_secs_f64(interval_secs.max(1.0)),
        }
    }

    pub fn start(&mut self, tx: Sender<PartitionUpdate>) {
        let stop = Arc::clone(&self.stop);
        let interval = self.interval;
        let handle = thread::spawn(move || {
            // First sample immediately so the UI isn't empty for `interval` seconds.
            if let Some(update) = collect_partitions() {
                let _ = tx.send(update);
            }
            while !stop.load(Ordering::Relaxed) {
                // Sleep in small slices so stop is responsive.
                let mut slept = Duration::ZERO;
                while slept < interval && !stop.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(200));
                    slept += Duration::from_millis(200);
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(update) = collect_partitions() {
                    if tx.send(update).is_err() {
                        break;
                    }
                }
            }
        });
        self.handle = Some(handle);
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for PartitionMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Collect a snapshot. Returns `None` if `sinfo` is missing or failed entirely.
fn collect_partitions() -> Option<PartitionUpdate> {
    // Per-partition node info.
    let sinfo = run_slurm_command(
        &["sinfo", "-h", "-N", "-o", "%P|%T|%C|%G|%n|%a"],
        false,
    )
    .ok()?;
    if sinfo.return_code != 0 {
        return None;
    }

    struct PartAcc {
        total: usize,
        free: usize,
        states: BTreeMap<String, usize>,
        nodes: Vec<String>,
        avail: String,
    }
    let mut parts: BTreeMap<String, PartAcc> = BTreeMap::new();

    for line in sinfo.stdout.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 6 {
            continue;
        }
        let part = f[0].trim_end_matches('*').to_string();
        let state_raw = f[1];
        let cpus = f[2];
        let _gres = f[3];
        let node = f[4].to_string();
        let avail = f[5].trim().to_string();

        // CPU column: A/I/O/T (alloc/idle/other/total)
        let mut idle_cpus: u32 = 0;
        if let Some((a, rest)) = cpus.split_once('/') {
            let _ = a;
            if let Some((i, _)) = rest.split_once('/') {
                idle_cpus = i.parse().unwrap_or(0);
            }
        }

        let state = state_raw
            .trim_end_matches(|c: char| matches!(c, '*' | '$' | '~' | '#' | '@' | '+'))
            .to_lowercase();

        let entry = parts.entry(part).or_insert_with(|| PartAcc {
            total: 0,
            free: 0,
            states: BTreeMap::new(),
            nodes: Vec::new(),
            avail: avail.clone(),
        });
        entry.avail = avail;
        entry.total += 1;
        *entry.states.entry(state.clone()).or_insert(0) += 1;
        entry.nodes.push(node);
        let is_free = state == "idle" || (state == "mix" && idle_cpus > 0);
        if is_free {
            entry.free += 1;
        }
    }

    // Per-node time-left (for partitions with no free nodes).
    let mut jobs_by_node: HashMap<String, Vec<i64>> = HashMap::new();
    let mut total_running = 0usize;
    if let Ok(sq) = run_slurm_command(
        &["squeue", "-a", "-h", "-t", "RUNNING", "-o", "%L|%N"],
        false,
    ) {
        for line in sq.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            total_running += 1;
            let (tleft, nl) = match line.split_once('|') {
                Some(p) => p,
                None => continue,
            };
            let secs = match parse_time_left(tleft) {
                Some(s) => s,
                None => continue,
            };
            for n in expand_nodelist(nl) {
                jobs_by_node.entry(n).or_default().push(secs);
            }
        }
    }

    // Per-partition pending counts.
    let mut pending: HashMap<String, usize> = HashMap::new();
    let mut total_pending = 0usize;
    if let Ok(sq) = run_slurm_command(
        &["squeue", "-h", "-t", "PENDING", "-o", "%P"],
        false,
    ) {
        for line in sq.stdout.lines() {
            let p = line.trim();
            if p.is_empty() {
                continue;
            }
            *pending.entry(p.to_string()).or_insert(0) += 1;
            total_pending += 1;
        }
    }

    let mut out: Vec<PartitionInfo> = Vec::new();
    for (name, acc) in parts {
        let min_wait_secs = if acc.free > 0 {
            Some(0)
        } else {
            let mut tl: Vec<i64> = Vec::new();
            for n in &acc.nodes {
                if let Some(v) = jobs_by_node.get(n) {
                    tl.extend(v.iter().copied());
                }
            }
            if let Some(&m) = tl.iter().min() {
                Some(m)
            } else {
                let up_states = ["idle", "mix", "alloc", "allocated", "mixed"];
                if acc.states.keys().any(|s| up_states.contains(&s.as_str())) {
                    None
                } else {
                    None
                }
            }
        };

        let is_down = !acc.avail.eq_ignore_ascii_case("up");

        let states = acc
            .states
            .iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect::<Vec<_>>()
            .join(",");

        let pending_jobs = pending.get(&name).copied().unwrap_or(0);

        out.push(PartitionInfo {
            name,
            total_nodes: acc.total,
            free_nodes: acc.free,
            pending_jobs,
            min_wait_secs,
            is_down,
            states,
        });
    }

    // The UI applies its own (clickable) sort — we just leave the natural
    // BTreeMap-by-name order here.

    Some(PartitionUpdate {
        partitions: out,
        total_running,
        total_pending,
    })
}

/// Parse SLURM duration strings: D-HH:MM:SS, HH:MM:SS, MM:SS, "INVALID", "UNLIMITED".
/// Returns `None` for unknown/invalid.
fn parse_time_left(s: &str) -> Option<i64> {
    let s = s.trim();
    if matches!(s, "INVALID" | "UNLIMITED" | "NOT_SET" | "N/A" | "") {
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

fn expand_nodelist(nl: &str) -> Vec<String> {
    let nl = nl.trim();
    if nl.is_empty() || nl.starts_with('(') {
        return Vec::new();
    }
    match run_slurm_command(&["scontrol", "show", "hostnames", nl], false) {
        Ok(r) if r.return_code == 0 => r
            .stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// Fetch pending jobs queued on a specific partition. Sorted by priority desc,
/// then submit-time asc.
pub fn fetch_partition_queue(partition: &str) -> Vec<PendingJob> {
    // squeue fields: jobid|user|name|priority|timelimit|submit|reason|nodes|cpus
    let fmt = "%i|%u|%j|%Q|%l|%V|%r|%D|%C";
    let r = run_slurm_command(
        &[
            "squeue",
            "-h",
            "-t",
            "PENDING",
            "-p",
            partition,
            "-o",
            fmt,
        ],
        false,
    );
    let stdout = match r {
        Ok(c) if c.return_code == 0 => c.stdout,
        _ => return Vec::new(),
    };
    let out: Vec<PendingJob> = stdout
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('|').collect();
            if f.len() < 9 {
                return None;
            }
            Some(PendingJob {
                job_id: f[0].trim().to_string(),
                user: f[1].trim().to_string(),
                name: f[2].trim().to_string(),
                priority: f[3].trim().to_string(),
                time_limit: f[4].trim().to_string(),
                submit_time: f[5].trim().to_string(),
                reason: f[6].trim().to_string(),
                nodes: f[7].trim().to_string(),
                cpus: f[8].trim().to_string(),
            })
        })
        .collect();
    out
}

/// Format a duration in seconds for display.
pub fn fmt_secs(sec: Option<i64>, is_down: bool) -> String {
    if is_down {
        return "DOWN".to_string();
    }
    let sec = match sec {
        None => return "unknown".to_string(),
        Some(s) if s < 0 => return "unknown".to_string(),
        Some(0) => return "now (free)".to_string(),
        Some(s) => s,
    };
    let (d, r) = (sec / 86400, sec % 86400);
    let (h, r) = (r / 3600, r % 3600);
    let (m, s) = (r / 60, r % 60);
    let mut out = Vec::new();
    if d > 0 {
        out.push(format!("{}d", d));
    }
    if h > 0 {
        out.push(format!("{}h", h));
    }
    if m > 0 {
        out.push(format!("{}m", m));
    }
    if s > 0 && d == 0 {
        out.push(format!("{}s", s));
    }
    if out.is_empty() {
        "0s".to_string()
    } else {
        out.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_time_left_variants() {
        assert_eq!(parse_time_left("1-02:03:04"), Some(86400 + 7384));
        assert_eq!(parse_time_left("02:03:04"), Some(7384));
        assert_eq!(parse_time_left("03:04"), Some(184));
        assert_eq!(parse_time_left("UNLIMITED"), None);
        assert_eq!(parse_time_left(""), None);
    }

    #[test]
    fn fmt_secs_cases() {
        assert_eq!(fmt_secs(Some(0), false), "now (free)");
        assert_eq!(fmt_secs(None, false), "unknown");
        assert_eq!(fmt_secs(Some(7384), false), "2h 3m 4s");
        assert_eq!(fmt_secs(Some(90000), false), "1d 1h");
        assert_eq!(fmt_secs(Some(0), true), "DOWN");
    }
}
