//! The `bp` binary: parses the verb, runs it from the working directory, exits with its code.

use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    // Clap exits 2 on a usage error; the contract reserves 2 for a refusal and gives usage 4, with the envelope
    // when the caller asked for JSON.
    let cli = match bp_tool::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if !e.use_stderr() => {
            let _ = e.print();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            if std::env::args().any(|a| a == "--json") {
                let refusal = bp_vault::Refusal::usage(e.kind().to_string());
                println!("{}", serde_json::to_string(&refusal).expect("a plain struct serialises"));
            } else {
                let _ = e.print();
            }
            return ExitCode::from(4);
        }
    };
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            let refusal = bp_vault::Refusal::failed("the working directory", e.to_string());
            println!("{}", serde_json::to_string(&refusal).expect("a plain struct serialises"));
            return ExitCode::from(1);
        }
    };
    bp_tool::run(&cli, &cwd, &mut std::io::stdout().lock())
}
