//! `bp check <id> [--name <n>]`: runs the owner's check blocks from the code repository and writes one run record per
//! block in one commit (15, check; 25, design changes 4 and 5). It exits 0 when no `error` block fails or times out,
//! else 2 with `check_failed` naming the first failed run. That is the one refusal that follows a commit, so its
//! envelope adds the commit and every run. An owner with no named block exits 3: nothing ran, nothing was written.

use std::path::Path;
use std::process::ExitCode;

use bp_vault::{Code, Refusal, Vault};
use clap::Args;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{CONTRACT, Envelope, emit, finish};

/// `bp check <id> [--name <n>]`.
#[derive(Args)]
pub struct Check {
    /// The owner of the blocks: a design, an issue or a milestone.
    pub id: String,
    /// Only the block with this name.
    #[arg(long)]
    pub name: Option<String>,
}

/// `check_failed`: the five fields of every refusal, then what the commit holds.
#[derive(Serialize)]
struct Failed {
    #[serde(flatten)]
    refusal: Refusal,
    commit: String,
    runs: Vec<Value>,
}

pub(crate) fn run(cwd: &Path, c: &Check, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    let (owner, checked) = match check(cwd, c) {
        Ok(v) => v,
        Err(refusal) => return finish::<Value>(Err(refusal), json, out),
    };
    let Some(checked) = checked else {
        emit(out, &Envelope { result: json!({ "id": owner }), contract_version: CONTRACT }, json);
        return ExitCode::from(3);
    };
    let runs: Vec<Value> = checked
        .runs
        .iter()
        .map(|r| json!({ "name": r.name, "verdict": r.verdict, "id": r.id, "note": r.note }))
        .collect();
    if let Some(run) = checked.failed() {
        let refusal = Refusal::new(
            Code::CheckFailed,
            run.id.as_str(),
            "every error-severity check block passes (25, design change 4)",
            format!(
                "{} {}: {}; read its output with bp show {}, fix the cause, then rerun bp check {owner} --name {}",
                run.name, run.verdict, run.note, run.id, run.name
            ),
        );
        let exit = refusal.exit;
        emit(out, &Failed { refusal, commit: checked.commit, runs }, json);
        return ExitCode::from(exit);
    }
    let result = json!({ "id": owner, "commit": checked.commit, "runs": runs, "findings": checked.findings });
    finish(Ok(result), json, out)
}

fn check(cwd: &Path, c: &Check) -> Result<(bp_vault::Id, Option<bp_check::Checked>), Refusal> {
    let vault = Vault::discover(cwd)?;
    let owner = crate::subject(&bp_vault::Index::open(&vault)?, &c.id)?;
    let role = crate::set::role()?;
    // A bad `BP_NOW` refuses here, before any block runs; after that the clock cannot fail.
    let start = crate::clock::now()?;
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(start));
    let checked = bp_check::check(&vault, &owner, cwd, c.name.as_deref(), role.as_str(), &clock)?;
    Ok((owner, checked))
}
