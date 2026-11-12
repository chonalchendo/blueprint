//! `bp risk <issue>`: the risk level in force and the rules that set it (37, design changes 1 and 2). The level is the
//! highest of the issue's `risk` label (absent reads `medium`), the level the claim's diff computes, each review that
//! holds a `risk` row, and a pending delta the issue caused. The diff is the pull request's head after a fetch: the
//! higher of the local branch and `origin/<branch>`, against the base branch its pull request targets. The rules are
//! `.bp/risk.yml` on that base commit, so a branch that edits them is judged by the base's rules, and its edit is
//! high. The answer names each head it judged and the base. It writes nothing to the vault and calls no forge.

use std::path::Path;

use bp_change::intents::Base;
use bp_change::risk::{self, Computed, Config, Level, Pattern, RISK_FILE};
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

/// The computed level, the base commit whose rules judged it, and each head it judged.
struct Judged {
    computed: Computed,
    base: String,
    heads: Vec<String>,
}

pub(crate) fn risk(cwd: &Path, r: &Risk) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &r.issue)?;
    let Some(header) = index.header(id.as_str())? else {
        return Err(vault.unparsed(&id).unwrap_or_else(|| {
            Refusal::new(Code::NoNode, id.as_str(), "bp risk reads one issue (37, design change 1)", "bp list issue")
        }));
    };
    if header.get("kind").and_then(Value::as_str) != Some("issue") {
        // `cut` refuses a node of another kind so too: only an issue moves, and only an issue has a level.
        return Err(Refusal::new(
            Code::IllegalTransition,
            id.as_str(),
            "bp risk reads one issue (37, design change 1)",
            "bp list issue",
        ));
    }
    let declared = match header.get("risk") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().and_then(|s| s.parse().ok()).ok_or_else(|| {
            Refusal::new(
                Code::UnknownValue,
                format!("{id} risk {v}"),
                "a label with a closed set takes one of its values (13)",
                format!("bp set {id} risk <low|medium|high>"),
            )
        })?),
    };
    // A pending delta is vault data: it raises the level with or without a clone in view (37, design change 1).
    let delta = !bp_read::carried_deltas(&index, &id)?.is_empty();
    let raised_by = bp_read::inputs::raises(&index, id.as_str())?;
    let judged = judged(cwd, &index, &id)?;
    let computed = judged.as_ref().map(|j| j.computed.level);
    let level = risk::level(declared.unwrap_or(Level::Medium), computed, raised_by.len() + usize::from(delta));
    let mut reasons = Vec::new();
    match declared {
        Some(d) if d == level => reasons.push(format!("the issue declares {d}")),
        None if level == Level::Medium => reasons.push("the issue declares no risk, which reads medium".into()),
        _ => {}
    }
    if let Some(j) = judged.as_ref().filter(|j| j.computed.level == level) {
        reasons.extend(j.computed.reasons.iter().cloned());
    }
    if level == Level::High {
        if delta {
            reasons.push("the issue caused a pending delta, and a design change is high".into());
        }
        reasons.extend(raised_by.iter().map(|r| format!("review {r} holds a risk row")));
    }
    Ok(json!({
        "level": level,
        "declared": declared.unwrap_or(Level::Medium),
        "computed": computed,
        "head": judged.as_ref().map_or(Vec::new(), |j| j.heads.clone()),
        "base": judged.as_ref().map(|j| j.base.clone()),
        "raised_by": raised_by,
        "reasons": reasons,
    }))
}

/// The level of the held claim's diff after a fetch; `None` with no held claim, or when the working directory is in
/// no clone of the project's repository. A gate reads `None` as not computed, never as low.
fn judged(cwd: &Path, index: &Index, id: &Id) -> Result<Option<Judged>, Refusal> {
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
    // Old refs would judge another head by stale rules, so a failed fetch refuses.
    git(&top, &["fetch", "-q", "origin"])?;
    let sha = |r: &str| git(&top, &["rev-parse", "--verify", "-q", &format!("{r}^{{commit}}")]).ok();
    let Some(base) = sha(&format!("refs/remotes/origin/{target}")) else {
        return Err(Refusal::failed(format!("origin/{target}"), "origin holds no base branch of the claim"));
    };
    let name = branch(id);
    let mut heads: Vec<String> =
        [format!("refs/heads/{name}"), format!("refs/remotes/origin/{name}")].iter().filter_map(|r| sha(r)).collect();
    heads.dedup();
    if heads.is_empty() {
        return Err(Refusal::failed(name, format!("the clone holds no branch of the claim; bp cut {id} puts it back")));
    }
    let config = rules(&top, &base, &target)?;
    let mut best: Option<Computed> = None;
    for head in &heads {
        let range = format!("{base}...{head}");
        let args = ["-c", "core.quotePath=false", "diff", "--no-ext-diff", "--numstat", "-z", "--find-renames", &range];
        let c = risk::computed(&risk::numstat(&git(&top, &args)?), &config);
        if best.as_ref().is_none_or(|b| c.level > b.level) {
            best = Some(c);
        }
    }
    let computed = best.expect("one head at least");
    Ok(Some(Judged { computed, base, heads }))
}

/// `.bp/risk.yml` on the base commit; the default when it holds none. An exact pattern that names a directory of the
/// base tree matches no path under it, so it refuses as a parse failure does.
fn rules(top: &Path, base: &str, target: &str) -> Result<Config, Refusal> {
    let at = format!("{base}:{RISK_FILE}");
    let invalid = |why: String| {
        Refusal::new(
            Code::RiskFileInvalid,
            format!("origin/{target}:{RISK_FILE}"),
            "the risk file holds `high` and `low` lists of path patterns, `limits` and `signer` (37, design change 1)",
            format!("{why}; edit {RISK_FILE} on {target} by hand, then git commit it"),
        )
    };
    if git(top, &["cat-file", "-e", &at]).is_err() {
        return Ok(Config::default());
    }
    let config = Config::parse(&git(top, &["show", &at])?).map_err(invalid)?;
    let exact = config.high.iter().chain(&config.low).filter_map(|p| match p {
        Pattern::Exact(e) => Some(e.as_str()),
        _ => None,
    });
    // One path at a time: `ls-tree` given `src` and `src/a.rs` together walks into `src` and never prints it.
    for path in exact {
        let entry = git(top, &["--literal-pathspecs", "ls-tree", "-z", base, "--", path])?;
        if entry.split_once('\t').is_some_and(|(meta, at)| meta.contains(" tree ") && at.trim_end_matches('\0') == path)
        {
            return Err(invalid(format!(
                "the pattern {path:?} names a directory, so it matches no path: write {path}/"
            )));
        }
    }
    Ok(config)
}
