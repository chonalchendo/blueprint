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
                let refusal = bp_vault::Refusal::usage(bad_argument(&e), help());
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

/// The argument clap could not place, else the kind of error.
fn bad_argument(e: &clap::Error) -> String {
    use clap::error::{ContextKind, ContextValue};
    let context = [ContextKind::InvalidArg, ContextKind::InvalidSubcommand, ContextKind::InvalidValue];
    match context.iter().find_map(|k| e.get(*k)) {
        Some(ContextValue::String(arg)) => format!("{}: {arg}", e.kind()),
        Some(ContextValue::Strings(args)) => format!("{}: {}", e.kind(), args.join(" ")),
        _ => e.kind().to_string(),
    }
}

/// `bp <verb> --help` for the verb on the command line, else `bp --help`.
fn help() -> String {
    use clap::CommandFactory;
    let verb = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    match verb.filter(|v| bp_tool::Cli::command().find_subcommand(v).is_some()) {
        Some(v) => format!("bp {v} --help"),
        None => "bp --help".into(),
    }
}
