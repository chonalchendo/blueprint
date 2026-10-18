//! The tool: the 18 verbs, the write path composition, the envelope. This milestone ships `where`; the other read verbs
//! land issue by issue. Every verb prints one JSON object on stdout with `--json`; diagnostics go to stderr.

// The permitted edges of the crate rule, re-exported so the dependency graph is real before the verbs use them.
pub use bp_change;
pub use bp_check;
pub use bp_design;
pub use bp_forge;
pub use bp_install;
pub use bp_operation;
pub use bp_reasoning;

use std::path::Path;
use std::process::ExitCode;

use bp_vault::{CONTRACT, Refusal, Vault};
use clap::{Parser, Subcommand};
use serde::Serialize;

/// Blueprint: one evolving blueprint per project. The agent's hands.
#[derive(Parser)]
#[command(name = "bp", version, about)]
pub struct Cli {
    /// Print the result as one JSON object. Every verb supports it; the agent always passes it.
    #[arg(long, global = true)]
    pub json: bool,
    /// The verb to run.
    #[command(subcommand)]
    pub verb: Verb,
}

/// The verbs. Read verbs first; the rest land with their milestones.
#[derive(Subcommand)]
pub enum Verb {
    /// Vault, project, worktree, contract version.
    Where,
}

/// The result of `where`.
#[derive(Serialize)]
pub struct Where {
    /// The vault root.
    pub vault: String,
    /// The project the working directory sits under, if any.
    pub project: Option<String>,
    /// The git worktree, once the repo verbs land.
    pub worktree: Option<String>,
    /// The contract version.
    pub contract: u32,
    /// The binary version.
    pub version: &'static str,
}

/// Runs one verb from `cwd` and returns the exit code: 0 ok, 1 error, 2 refused, 3 no-op, 4 usage.
pub fn run(cli: &Cli, cwd: &Path, out: &mut impl std::io::Write) -> ExitCode {
    let result = match cli.verb {
        Verb::Where => where_(cwd),
    };
    match result {
        Ok(value) => {
            emit(out, &value, cli.json);
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            emit(out, &refusal, cli.json);
            ExitCode::from(refusal.exit)
        }
    }
}

fn where_(cwd: &Path) -> Result<serde_json::Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let w = Where {
        project: vault.project_of(cwd),
        vault: vault.root.display().to_string(),
        worktree: None,
        contract: CONTRACT,
        version: env!("CARGO_PKG_VERSION"),
    };
    Ok(serde_json::to_value(w).expect("a plain struct serialises"))
}

fn emit(out: &mut impl std::io::Write, value: &impl Serialize, json: bool) {
    let text = if json {
        serde_json::to_string(value).expect("a plain struct serialises")
    } else {
        serde_json::to_string_pretty(value).expect("a plain struct serialises")
    };
    // A closed stdout (a pipe that went away) is not an error the tool can act on.
    let _ = writeln!(out, "{text}");
}
