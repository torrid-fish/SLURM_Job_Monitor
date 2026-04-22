# SLURM Job Monitor — Agent Notes

## Build & Run

```bash
cargo build --release          # LTO + strip enabled in release profile
cargo run -- watch             # auto-discover and monitor all jobs from sacct
cargo run -- watch 12345 12346 # monitor specific jobs (expands array jobs)
cargo run -- submit script.sh  # submit and watch
cargo run -- submit script.sh --no-watch  # submit without monitoring
cargo run -- list              # list tracked jobs with status
cargo run -- stop 12345        # informational only — prints message, no real effect
```

`watch` and `submit` accept `--editor <cmd>` to override the editor for opening log files (Enter key). Resolution: CLI flag → `$VISUAL` → `$EDITOR` → `vim`.

Binary name: `slurm-monitor` (defined in `[[bin]]`).

## Test

```bash
cargo test
```

- Unit tests live inside each source file under `#[cfg(test)] mod tests`.
- No integration test directory (`tests/`).
- All tests are pure logic/parsing — no SLURM commands called, no TUI rendering.
- `log_tailer` tests use `tempfile` dev-dependency for FileState tests.
- Most runtime code (SLURM commands, TUI, channels) is untested — would require mocking `run_slurm_command`.

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

**Threading**: Two background threads communicate with the main TUI loop via `mpsc` channels. `StatusMonitor` sends `StatusUpdate`; `LogTailer` sends `LogUpdate`. Commands flow back through `TailerCommand` / `MonitorCommand` enums on separate channels. Both threads use 100ms check intervals within their longer poll cycles (3.0s status, 1.0s log).

**Auto-discovery**: When `watch` is called with no job IDs, it queries `sacct` for all visible jobs and enables `auto_discover` mode. Every 10s, the event loop polls for new jobs. User-deleted jobs (via `d` key) are tracked in `deleted_jobs: HashSet<JobId>` to prevent re-adding.

**Editor integration**: Pressing Enter suspends the TUI (`LeaveAlternateScreen` + raw mode off), opens the focused log file in the configured editor, then resumes. The `suspend_and_open_editor` function in `cli.rs` handles the terminal state transitions.

## Key Conventions

- **Key bindings**: `n` = prev job, `p` = next job (vim-style, reversed from numeric intuition). `d` = remove job. `Tab` = switch stdout/stderr focus. `Enter` = open focused log file in editor. Arrow keys = enter scroll mode. `q` = exit scroll mode or quit. `Ctrl+C` = always quit.
- **Array jobs**: `JobId` supports `base_index` format (e.g. `8322_5`). `watch 8322` auto-expands all subtasks via `sacct`. Explicit `8322_5` monitors only that subtask.
- **Debug logging**: Identical `debug_log()` function copy-pasted in `cli.rs`, `job_manager.rs`, and `log_tailer.rs` (writes to `/tmp/slurm_monitor_debug.log`). Not gated behind a feature flag — always active in any build.
- **Synchronized rendering**: Uses `BeginSynchronizedUpdate`/`EndSynchronizedUpdate` to prevent flicker in tmux.
- **Scroll mode**: Per-panel state (`stdout_scroll_mode` / `stderr_scroll_mode`). Arrow keys enter scroll mode (disables auto-scroll). Scrolling to bottom or pressing `q` exits scroll mode. `Ctrl+C` always quits regardless.
- **Error handling**: All functions return `anyhow::Result`. Errors use `.context()` for chaining. No custom error types despite `thiserror` being a dependency.
- **Carriage return handling**: `process_log_content` in `app.rs` simulates terminal `\r` behavior — clearing the current line on `\r` instead of appending. This matters for progress bar output from SLURM jobs.

## SLURM Dependencies

The tool shells out to these commands. They must be in `$PATH`:

| Command | Used by | Notes |
|---------|---------|-------|
| `sbatch` | `job_manager.rs` | Submit jobs |
| `squeue` | `job_manager.rs` | Check active job status (checked first, fallback to sacct) |
| `sacct`  | `job_manager.rs`, `utils.rs` | Completed job info, auto-discovery, array job expansion |

Output file resolution: checks `sacct` `StdOut`/`StdErr` paths first, falls back to `slurm-<id>.out`/`.err` in the working directory. SLURM placeholders (`%j`, `%A`, `%a`) are resolved in `resolve_output_path`. For array jobs, also tries base-id pattern and index-0 pattern.

## Gotchas

- `run_slurm_command_with_timeout` has an `_timeout_secs` parameter — timeout is **not enforced** on process execution. `Command::output()` runs indefinitely. The `_` prefix suppresses the compiler warning.
- `JobId` `Ord` impl sorts ascending (`base_id` then `array_index`). `get_sorted_job_ids()` reverses to show newest first. The `app.rs` sort is **descending** by `base_id`, then **descending** by `array_index` — different from the natural ascending order.
- `parse_sacct_output` merges data from multiple rows (main job + batch step), preferring non-empty `StdOut`/`StdErr` from batch steps.
- `LogTailer.add_file` silently skips duplicate labels — no warning on re-add. `LogTailer.remove_file` exists but is only called from the event loop `d` key handler.
- `handle_stop` is informational only — prints a message, doesn't actually unsubscribe from monitoring.
- `squeue` is queried before `sacct` in `get_job_status` — running jobs use squeue, completed/failed fall back to sacct.
- `thiserror` is declared as a dependency but no custom error types are defined — all errors use `anyhow`.
- No CI, no Makefile, no clippy config, no rustfmt config — this is a bare project with no linting/formatting enforcement.