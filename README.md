# lazyslurm

A real-time TUI for monitoring SLURM jobs -- see status, stdout, and stderr in one place.

![](./images/sample_screen_shot.png)

## Features

- Real-time status monitoring (QUEUED -> RUNNING -> COMPLETED/FAILED)
- Live stdout/stderr tailing with word-wrap
- Multi-job support with easy switching
- Multiple layout modes (Horizontal, Vertical, Stacked, FullLog)
- Auto-discover jobs from `sacct`
- Single binary, no runtime dependencies

## Quick Start

### Install

```bash
cargo install --path .
# or
cargo build --release && cp target/release/lazyslurm ~/.local/bin/
```

### Usage

```bash
# Monitor all your jobs (auto-discovers from sacct)
lazyslurm

# Monitor specific jobs
lazyslurm 12345 12346
```

Pass `--editor <cmd>` to override the editor used when opening log files with Enter. Resolution: `--editor` flag → `$VISUAL` → `$EDITOR` → `vim`.

Run `lazyslurm --help` for detailed options.

## UI Controls

| Key | Action |
|-----|--------|
| Tab | Switch stdout/stderr focus |
| Up/Down | Scroll focused panel |
| PgUp/PgDn | Scroll by page |
| Home/End | Jump to top/bottom |
| n | Previous job (vim-style) |
| p | Next job (vim-style) |
| d | Remove job from view |
| l | Cycle layout mode |
| Enter | Open focused log file in editor |
| q | Exit scroll mode / quit |
| Ctrl+C | Quit |
| Mouse click | Switch stdout/stderr panel focus |
| Mouse scroll | Scroll focused panel |

## Troubleshooting

**Job output files not found** -- Ensure the job has started (files are created at launch), you have read permissions, and output files are in the expected location. The tool checks `sacct` paths, then falls back to `slurm-<job_id>.out` in the current directory.

**Status shows UNKNOWN** -- The job ID may not exist, SLURM commands may not be in PATH, or there may be permission issues.

**UI not displaying correctly** -- Ensure your terminal supports colors and Unicode. If using tmux/screen, check terminal settings.

## License

MIT
