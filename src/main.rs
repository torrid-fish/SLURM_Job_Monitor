mod cli;
mod job_manager;
mod log_tailer;
mod status_monitor;
mod ui;
mod utils;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Submit { script, no_watch, editor } => {
            cli::handle_submit(&script, no_watch, editor.as_deref())?;
        }
        Commands::Watch { job_ids, editor } => {
            cli::handle_watch(job_ids, editor.as_deref())?;
        }
        Commands::List => {
            cli::handle_list()?;
        }
        Commands::Stop { job_id } => {
            cli::handle_stop(&job_id)?;
        }
    }

    Ok(())
}
