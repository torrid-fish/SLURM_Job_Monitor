# SLURM Job Monitor — Agent Notes

## Build & Run

```bash
cargo build --release          # LTO + strip enabled in release profile
cargo run -- watch             # monitor all jobs from sacct
cargo run -- watch 12345 12346 # monitor specific jobs
cargo run -- submit script.sh  # submit and watch
```

Binary name: `slurm-monitor` (defined in `[[bin]]`).

## Test

```bash
cargo test
```

- Unit tests live inside each source file under `#[cfg(test)] mod tests`.
- No integration test directory (`tests/`).
- Most functionality requires SLURM commands (`sacct`, `squeue`, `sbatch`) in PATH — unit tests only cover parsing/logic, not SLURM interaction.

## Architecture

Single-crate Rust 2021 project. No workspace, no sub-crates.

```
src/main.rs           — entry point, CLI dispatch
src/cli.rs            — clap derive CLI, terminal setup, event loop (the real main)
src/job_manager.rs    — SLURM job lifecycle (submit, status queries via sacct/squeue)
src/status_monitor.rs — background thread: polls SLURM status on interval
src/log_tailer.rs     — background thread: tails stdout/stderr files (notify + fallback poll)
src/utils.rs          — JobId type, SLURM command execution, output parsing
src/ui/app.rs         — App state: jobs map, scroll state, panel focus
src/ui/render.rs      — Ratatui rendering (header, status table, stdout/stderr panels)
src/ui/mod.rs         — re-exports App + render
```

**Threading**: Two background threads communicate with the main TUI loop via `mpsc` channels. `StatusMonitor` sends `StatusUpdate`; `LogTailer` sends `LogUpdate`. Commands flow back through `TailerCommand` / `MonitorCommand` enums on separate channels.

## Key Conventions

- **Key binding quirk**: `n` = prev job, `p` = next job (vim-style navigation, but reversed from numeric intuition).
- **Array jobs**: `JobId` supports `base_index` format (e.g. `8322_5`). `watch 8322` auto-expands all subtasks via `sacct`.
- **Debug logging**: Local `debug_log()` in `cli.rs`, `job_manager.rs`, `log_tailer.rs` writes to `/tmp/slurm_monitor_debug.log`. Not gated behind a feature flag — always active.
- **Synchronized rendering**: Uses `BeginSynchronizedUpdate`/`EndSynchronizedUpdate` to prevent flicker in tmux.
- **Scroll mode**: Arrow keys enter scroll mode (track state per panel). `q` exits scroll mode before quitting. `Ctrl+C` always quits.

## SLURM Dependencies

The tool shells out to these SLURM commands. They must be in `$PATH`:

| Command | Used by |
|---------|---------|
| `sbatch` | `job_manager.rs` — submit jobs |
| `squeue` | `job_manager.rs` — check active job status |
| `sacct`  | `job_manager.rs`, `utils.rs` — completed job info, auto-discovery |

Output file resolution: checks `sacct` `StdOut`/`StdErr` paths first, falls back to `slurm-<id>.out`/`.err` in the working directory. SLURM placeholders (`%j`, `%A`, `%a`) are resolved in `resolve_output_path`.

## Gotchas

- `run_slurm_command_with_timeout` has an unused `_timeout_secs` parameter — timeout is not actually enforced on process execution.
- `JobId` sorts by `base_id` desc, then `array_index` desc (newest jobs first).
- `parse_sacct_output` merges data from multiple rows (main job + batch step), preferring non-empty `StdOut`/`StdErr` from batch steps.
- `LogTailer.add_file` silently skips duplicate labels — no warning on re-add.
- The `handle_stop` command is informational only (prints a message, doesn't actually unsubscribe from monitoring).