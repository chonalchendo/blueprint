//! The `bp-view` binary: serves the human view of one vault on a loopback port until SIGINT or SIGTERM.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// `bp-view --vault <dir> [--port <n>] [--forge off] [--clone <dir>]`.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The vault root.
    #[arg(long)]
    vault: PathBuf,
    /// The loopback port; 0 takes a free one.
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// `off` never calls `gh`: the pages read the forge snapshot that exists.
    #[arg(long, value_parser = ["off"])]
    forge: Option<String>,
    /// The clone of the project's repository the forge is read from; the working directory when absent.
    #[arg(long)]
    clone: Option<PathBuf>,
}

fn main() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let clone = match cli.clone {
        Some(c) => c,
        None => std::env::current_dir()?,
    };
    let view = match bp_view::View::new(&cli.vault, clone, cli.forge.is_none()) {
        Ok(v) => v,
        Err(refusal) => {
            println!("{}", serde_json::to_string(&refusal)?);
            return Ok(ExitCode::from(refusal.exit));
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(bp_view::serve(view, cli.port))?;
    Ok(ExitCode::SUCCESS)
}
