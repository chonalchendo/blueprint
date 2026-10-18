//! The `bp` binary: parses the verb, runs it from the working directory, exits with its code.

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = bp_tool::Cli::parse();
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("bp: cannot read the working directory: {e}");
            return ExitCode::from(1);
        }
    };
    bp_tool::run(&cli, &cwd, &mut std::io::stdout().lock())
}
