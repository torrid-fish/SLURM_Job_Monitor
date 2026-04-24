# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Detailed agent notes already live in `AGENTS.md` — read it first. This file summarizes the highlights and points at things that are easy to get wrong.

## Build, Run, Test

```bash
cargo build --release          # LTO + strip enabled
cargo run                      # auto-discover all jobs from sacct
cargo run -- 12345             # specific job (array jobs auto-expand)
cargo test                     # unit tests only, inline #[cfg(test)] modules
```

Binary name is `lazyslurm`. No subcommands — the binary always runs the watch TUI; positional args are job IDs. No CI, no Makefile, no rustfmt/clippy config — nothing enforces lint/format.

## Architecture

Single-crate Rust 2021 TUI (ratatui + crossterm). Three threads communicate through `mpsc` channels:

- **Main/TUI loop** (`src/cli.rs`) — the real `main`; owns the event loop, terminal, and `App` state.
- **StatusMonitor thread** (`src/status_monitor.rs`) — polls SLURM every ~3s, emits `StatusUpdate`. Control via `MonitorCommand`.
- **LogTailer thread** (`src/log_tailer.rs`) — tails stdout/stderr with `notify` + 1s fallback poll, emits `LogUpdate`. Control via `TailerCommand`.

`src/job_manager.rs` shells out to `squeue`/`sacct` (must be on `$PATH`). `squeue` is tried before `sacct` in `get_job_status`. `src/utils.rs` defines `JobId` (supports `base_id` + optional `array_index`, e.g. `8322_5`) and the `run_slurm_command*` helpers. `src/ui/{app,render}.rs` holds App state and ratatui rendering.

Editor integration (Enter key) suspends the TUI via `suspend_and_open_editor` in `cli.rs`: leaves alternate screen + raw mode, runs editor, then resumes. Editor resolution: `--editor` flag → `$VISUAL` → `$EDITOR` → `vim`.

## Non-obvious behavior

- `run_slurm_command_with_timeout` **does not enforce its timeout** — the `_timeout_secs` underscore-prefix is intentional; `Command::output()` runs unbounded.
- `JobId`'s natural `Ord` is ascending, but `app.rs` sorts **descending** by `base_id` then `array_index` to show newest jobs first. `get_sorted_job_ids()` reverses for the same reason — don't "fix" one without the other.
- `parse_sacct_output` merges the main job row with batch-step rows, preferring non-empty `StdOut`/`StdErr` from batch steps.
- `LogTailer.add_file` silently ignores duplicate labels.
- `process_log_content` in `app.rs` simulates terminal `\r` (overwrite current line) — required for progress-bar output to render correctly; do not change to plain append.
- `debug_log()` is copy-pasted in `cli.rs`, `job_manager.rs`, and `log_tailer.rs` and always writes to `/tmp/lazyslurm_debug.log` (not feature-gated).
- `thiserror` is in `Cargo.toml` but unused — all errors go through `anyhow::Result` with `.context()`.
- Auto-discovery (run with no IDs) re-polls `sacct` every 10s; jobs removed with `d` are tracked in `deleted_jobs: HashSet<JobId>` so they aren't re-added.

## Key bindings worth remembering

`n` = previous job, `p` = next job (vim-reversed from numeric intuition). `Tab` toggles stdout/stderr focus. `d` removes the current job from view. `Enter` opens the focused log in the editor. Arrow keys enter per-panel scroll mode (disables auto-scroll); scrolling to the bottom or pressing `q` exits scroll mode. `Ctrl+C` always quits.
