//! `bp risk <issue>`: the risk level in force and the rules that set it (37, design changes 1 and 2). The level is the
//! highest of the issue's `risk` label (absent reads `medium`), the level the claim's diff computes, and each review
//! that holds a `risk` row. The diff is the held claim's branch against the base branch its pull request targets, as
//! the clone's remote-tracking ref holds it; the rules are `.bp/risk.yml` on that base branch, so a branch that edits
//! them is judged by the base's rules, and its edit is high. A read: it writes nothing and calls no forge.

use std::path::Path;

use bp_change::intents::Base;
use bp_change::risk::{self, Computed, Config, Level, RISK_FILE};
use bp_read::inputs::{field, held};
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::claim::{branch, default_branch, git};
use crate::subject;

/// `bp risk <issue>`.
#[derive(Args)]
pub struct Risk {
    /// The issue whose risk level prints.
    pub issue: String,
}

pub(crate) fn risk(cwd: &Path, r: &Risk) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &r.issue)?;
    let header = index.header(id.as_str())?.filter(|h| h.get("kind").and_then(Value::as_str) == Some("issue"));
    let Some(header) = header else {
        return Err(vault.unparsed(&id).unwrap_or_else(|| {
            Refusal::new(Code::NoNode, id.as_str(), "bp risk reads one issue (37, design change 1)", "bp list issue")
        }));
    };
    let declared = match header.get("risk") {
        None | Some(Value::Null) => Level::Medium,
        Some(v) => v.as_str().and_then(|s| s.parse().ok()).ok_or_else(|| {
            Refusal::new(
                Code::UnknownValue,
                format!("{id} risk {v}"),
                "a label with a closed set takes one of its values (13)",
                format!("bp set {id} risk <low|medium|high>"),
            )
        })?,
    };
    let raised_by = bp_read::inputs::raises(&index, id.as_str())?;
    let computed = computed(cwd, &index, &id)?;
    let level = risk::level(declared, computed.as_ref().map(|c| c.level), raised_by.len());
    let mut reasons = Vec::new();
    if declared == level {
        reasons.push(match header.get("risk") {
            Some(_) => format!("the issue declares {declared}"),
            None => "the issue declares no risk, which reads medium".into(),
        });
    }
    if let Some(c) = computed.as_ref().filter(|c| c.level == level) {
        reasons.extend(c.reasons.iter().cloned());
    }
    if level == Level::High {
        reasons.extend(raised_by.iter().map(|r| format!("review {r} holds a risk row")));
    }
    Ok(json!({
        "level": level,
        "declared": declared,
        "computed": computed.map(|c| c.level),
        "raised_by": raised_by,
        "reasons": reasons,
    }))
}

/// The level of the held claim's diff; `None` with no held claim, or when the working directory is in no clone of the
/// project's repository.
fn computed(cwd: &Path, index: &Index, id: &Id) -> Result<Option<Computed>, Refusal> {
    let Some(claim) = held(index, id)? else { return Ok(None) };
    let top = match clone_of(cwd, index, id.project().unwrap_or_default()) {
        Ok(top) => top,
        Err(e) if e.code == Code::NoClone => return Ok(None),
        Err(e) => return Err(e),
    };
    // The base branch the pull request targets, as `land` finds it: a layer over an open dependency targets its branch.
    let target = match bp_read::inputs::base(index, &claim)? {
        Base::Branch(b) => b,
        Base::Unknown => default_branch(&top)?,
        Base::Unmerged(_) => field(&claim, "base_branch").unwrap_or_default(),
    };
    let base = format!("refs/remotes/origin/{target}");
    let name = branch(id);
    let exists = |r: &str| git(&top, &["rev-parse", "--verify", "-q", r]).is_ok();
    let Some(head) =
        [format!("refs/heads/{name}"), format!("refs/remotes/origin/{name}")].into_iter().find(|r| exists(r))
    else {
        return Err(Refusal::failed(name, format!("the clone holds no branch of the claim; bp cut {id} puts it back")));
    };
    if !exists(&base) {
        return Err(Refusal::failed(base, "the clone has no copy of the base branch: git fetch origin, then rerun"));
    }
    let range = format!("{base}...{head}");
    let args = ["-c", "core.quotePath=false", "diff", "--no-ext-diff", "--numstat", "-z", "--find-renames", &range];
    let diff = risk::numstat(&git(&top, &args)?);
    let rules = format!("{base}:{RISK_FILE}");
    let config = if exists(&rules) {
        Config::parse(&git(&top, &["show", &rules])?).map_err(|why| {
            Refusal::new(
                Code::SchemaInvalid,
                rules.clone(),
                "the risk file holds `high` and `low` lists of path patterns, `limits` and `signer` (37, design change 1)",
                format!("{why}; edit {RISK_FILE} on {target} by hand, then git commit it"),
            )
        })?
    } else {
        Config::default()
    };
    let delta = !bp_read::carried_deltas(index, id)?.is_empty();
    Ok(Some(risk::computed(&diff, &config, delta)))
}
