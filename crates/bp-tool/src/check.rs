//! `bp check <id> [--name <n>]`: runs the owner's check blocks from the code repository and writes one run record per
//! block in one commit (15, check; 25, design changes 4 and 5). It exits 0 when no `error` block fails or times out,
//! else 2 with `check_failed` naming the first failed run. That is the one refusal that follows a commit, so its
//! envelope adds the commit and every run. An owner with no named block exits 3: nothing ran, nothing was written.

use std::path::{Path, PathBuf};
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

/// `check_failed`: the five fields of every refusal, then what the commit holds, and `pr`'s pull request and what it
/// carries.
#[derive(Serialize)]
struct Failed {
    #[serde(flatten)]
    refusal: Refusal,
    commit: String,
    runs: Vec<Value>,
    #[serde(flatten)]
    carries: Option<Value>,
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
    let runs = rows(&checked.runs);
    if let Some(run) = checked.failed() {
        let then = format!("rerun bp check {owner} --name {}", run.name);
        return failed(run, &then, &checked.commit, runs, None, json, out);
    }
    let result = json!({ "id": owner, "commit": checked.commit, "runs": runs, "findings": checked.findings });
    finish(Ok(result), json, out)
}

/// Each run as a result lists it.
pub(crate) fn rows(runs: &[bp_check::Run]) -> Vec<Value> {
    runs.iter().map(|r| json!({ "name": r.name, "verdict": r.verdict, "id": r.id, "note": r.note })).collect()
}

/// `check_failed` for the first failed run, after the commit that holds the runs: the one refusal that follows a
/// commit, so its envelope adds the commit and every run, and `pr`'s pull request, which the push kept, with its
/// `deltas` and `check_changes`. `then` is the command that tries again.
pub(crate) fn failed(
    run: &bp_check::Run,
    then: &str,
    commit: &str,
    runs: Vec<Value>,
    carries: Option<Value>,
    json: bool,
    out: &mut impl std::io::Write,
) -> ExitCode {
    let refusal = Refusal::new(
        Code::CheckFailed,
        run.id.as_str(),
        "every error-severity check block passes (25, design change 4)",
        format!(
            "{} {}: {}; read its output with bp show {}, fix the cause, then {then}",
            run.name, run.verdict, run.note, run.id
        ),
    );
    let exit = refusal.exit;
    emit(out, &Failed { refusal, commit: commit.into(), runs, carries }, json);
    ExitCode::from(exit)
}

fn check(cwd: &Path, c: &Check) -> Result<(bp_vault::Id, Option<bp_check::Checked>), Refusal> {
    let vault = Vault::discover(cwd)?;
    let index = bp_vault::Index::open(&vault)?;
    let owner = crate::subject(&index, &c.id)?;
    // A held claim's blocks run in its worktree, as `pr` runs them: an agent's shell may sit in the clone, whose head
    // is the base (27, issue 8). Else the blocks run where the caller stands.
    let worktree = crate::claim::held(&index, &owner)?
        .and_then(|claim| crate::claim::field(&claim, "worktree"))
        .map(PathBuf::from)
        .filter(|w| w.is_dir());
    drop(index);
    let cwd = worktree.as_deref().unwrap_or(cwd);
    let role = crate::set::role()?;
    // A bad `BP_NOW` refuses here, before any block runs; after that the clock cannot fail.
    let start = crate::clock::now()?;
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(start));
    let checked = bp_check::check(&vault, &owner, cwd, c.name.as_deref(), role.as_str(), &clock)?;
    Ok((owner, checked))
}
