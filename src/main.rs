mod cli;
mod job_manager;
mod log_tailer;
mod status_monitor;
mod ui;
mod utils;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    cli::handle_watch(cli.job_ids, cli.editor.as_deref())
}
