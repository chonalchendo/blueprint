//! `bp check <id> [--name <n>]`: runs the owner's check blocks from the code repository and writes one run record per
//! block in one commit (15, check; 25, design changes 4 and 5). It exits 0 when every `error` block passes, else 2
//! with `check_failed` naming the first failed run; the records are committed either way.

use std::path::Path;

use bp_vault::{Code, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::node_id;

/// `bp check <id> [--name <n>]`.
#[derive(Args)]
pub struct Check {
    /// The owner of the blocks: a design, an issue or a milestone.
    pub id: String,
    /// Only the block with this name.
    #[arg(long)]
    pub name: Option<String>,
}

pub(crate) fn check(cwd: &Path, c: &Check) -> Result<Value, Refusal> {
    let owner = node_id(&c.id)?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    // A bad `BP_NOW` refuses here, before any block runs; after that the clock cannot fail.
    let start = crate::clock::now()?;
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(start));
    let checked = bp_check::check(&vault, &owner, cwd, c.name.as_deref(), role.as_str(), &clock)?;
    if let Some(run) = checked.failed() {
        return Err(Refusal::new(
            Code::CheckFailed,
            run.id.as_str(),
            "every error-severity check block passes (25, design change 4)",
            format!(
                "{} {}: {}; read its output with bp show {}, fix the cause, then rerun bp check {owner} --name {}",
                run.name, run.verdict, run.note, run.id, run.name
            ),
        ));
    }
    let runs: Vec<Value> = checked
        .runs
        .iter()
        .map(|r| json!({ "name": r.name, "verdict": r.verdict, "id": r.id, "note": r.note }))
        .collect();
    Ok(json!({ "id": owner, "commit": checked.commit, "runs": runs, "findings": checked.findings }))
}
