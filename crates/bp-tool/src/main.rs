//! The `bp` binary: parses the verb, runs it from the working directory, exits with its code.

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    // Clap exits 2 on a usage error; the contract reserves 2 for a refusal and gives usage 4.
    let cli = match bp_tool::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { 4 } else { 0 });
        }
    };
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("bp: cannot read the working directory: {e}");
            return ExitCode::from(1);
        }
    };
    bp_tool::run(&cli, &cwd, &mut std::io::stdout().lock())
}
