//! `bp risk <issue>`: the risk level in force and the rules that set it (37, design changes 1 and 2). The level is the
//! highest of the issue's declared level (its `risk` label, absent reads `medium`; a label lowered by hand once locked
//! reads as the one it replaced), the level the claim's diff computes, each review that holds a `risk` row as first
//! committed, and a pending delta the issue caused; then the round caps (design change 6).
//! The diff is the higher of the local branch and the sha that origin's server names for the branch, against the sha
//! it names for the base branch the pull request targets; each sha is fetched. The rules are `.bp/risk.yml` on that
//! base commit, so a branch that edits them is judged by the base's rules, and its edit is high. A lowering that the
//! human signed for the one head judged sets the level there. The answer names each head it judged and the base. It
//! writes nothing to the vault and calls no `gh`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bp_change::intents::Base;
use bp_change::risk::{self, Computed, Config, Level, Pattern, RISK_FILE};
use bp_change::rounds;
use bp_read::inputs::{field, held};
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Kind, Record, Refusal, Vault};
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

/// The computed level, the base commit whose rules judged it, each head it judged, and the clone it read.
pub(crate) struct Judged {
    pub(crate) computed: Computed,
    pub(crate) base: String,
    pub(crate) heads: Vec<String>,
    pub(crate) top: PathBuf,
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
    let label = match header.get("risk") {
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
    // A label lowered by hand once it was locked reads as the higher one it replaced (37, design change 1).
    let declared = bp_read::inputs::declared(&vault, &index, id.as_str())?;
    // A pending delta is vault data: it raises the level with or without a clone in view (37, design change 1).
    let delta = !bp_read::carried_deltas(&index, &id)?.is_empty();
    // Each review counts as its first commit wrote it, so an edit cannot drop a raise or an error row (37, design
    // change 2).
    let reviews = bp_read::inputs::reviews(&vault, &index, id.as_str())?;
    let raised_by: Vec<&str> = reviews.iter().filter(|r| r.raises()).map(|r| r.id.as_str()).collect();
    let judged = judged(cwd, &index, &id)?;
    let computed = judged.as_ref().map(|j| j.computed.level);
    let base = risk::level(declared.unwrap_or(Level::Medium), computed, raised_by.len() + usize::from(delta));
    let (level, capped) = rounds::capped(base, &reviews);
    let lowered = match &judged {
        Some(j) => lowering(&index, &id, j)?.filter(|l| *l < level),
        None => None,
    };
    let (level, capped) = lowered.map_or((level, capped), |l| (l, None));
    let mut reasons = Vec::new();
    if let Some(l) = lowered {
        reasons.push(format!("the human's signed lowering of this head sets {l}"));
    }
    if declared != label {
        let label = label.map_or("no risk label".into(), |l| format!("risk: {l}"));
        reasons.push(format!(
            "the issue reads {label}, written after its milestone's approval or its claim: it declares the {} it \
             held before",
            declared.unwrap_or(Level::Medium)
        ));
    }
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
    if let Some((round, from)) = capped {
        reasons.push(format!("round cap: the review of round {round} holds an error row at or past the cap of {from}"));
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

/// The level of the held claim's diff; `None` with no held claim, or when the working directory is in no clone of
/// the project's repository. A gate reads `None` as not computed, never as low. The heads are the higher of the local
/// branch and the one origin names.
fn judged(cwd: &Path, index: &Index, id: &Id) -> Result<Option<Judged>, Refusal> {
    let Some(claim) = held(index, id)? else { return Ok(None) };
    let top = match clone_of(cwd, index, id.project().unwrap_or_default()) {
        Ok(top) => top,
        Err(e) if e.code == Code::NoClone => return Ok(None),
        Err(e) => return Err(e),
    };
    let target = target(index, &claim, &top)?;
    let name = branch(id);
    let local = git(&top, &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}^{{commit}}")]).ok();
    let mut heads: Vec<String> = local.into_iter().chain(at_origin(&top, &name)?).collect();
    heads.dedup();
    if heads.is_empty() {
        return Err(Refusal::failed(name, format!("the clone holds no branch of the claim; bp cut {id} puts it back")));
    }
    judge(&top, &target, heads).map(Some)
}

/// The branch the pull request of the held `claim` targets, as `land` finds it: its base branch, origin's default
/// branch for a claim that records none, or a layer's own `base_branch` over an open dependency.
pub(crate) fn target(index: &Index, claim: &Record, top: &Path) -> Result<String, Refusal> {
    Ok(match bp_read::inputs::base(index, claim)? {
        Base::Branch(b) => b,
        Base::Unknown => default_branch(top)?,
        Base::Unmerged(_) => field(claim, "base_branch").unwrap_or_default(),
    })
}

/// The level of the human's signed lowering of the head judged (37, design change 2; review of #95, row 23). A lowering
/// binds one head, so with a local branch past origin's two heads are judged and none counts. `bp risk` calls no `gh`,
/// so it reads the record's own pull request and base; `land` holds them to the forge's. The signer is read only when
/// a lowering record names the head.
fn lowering(index: &Index, id: &Id, j: &Judged) -> Result<Option<Level>, Refusal> {
    let [head] = j.heads.as_slice() else { return Ok(None) };
    let at = |r: &Record| r.header.get("head").and_then(Value::as_str) == Some(head.as_str());
    if !index.records(id.as_str(), Kind::Lowering)?.iter().any(at) {
        return Ok(None);
    }
    let (signer, _) = crate::signed::signer(&j.top)?;
    crate::approve::lowered(index, id, head, &signer, None)
}

/// The level of each head's diff against the base that origin names for `target`, by the rules on that base; the
/// highest wins, and a tie keeps the first head's reasons. `pr` calls it with the head it pushes.
pub(crate) fn judge(top: &Path, target: &str, heads: Vec<String>) -> Result<Judged, Refusal> {
    let Some(base) = at_origin(top, target)? else {
        return Err(Refusal::failed(format!("origin/{target}"), "origin holds no base branch of the claim"));
    };
    judge_at(top, base, target, heads)
}

/// [`judge`] against the fetched commit `base` of `target`: `land` passes the base that the forge names.
pub(crate) fn judge_at(top: &Path, base: String, target: &str, heads: Vec<String>) -> Result<Judged, Refusal> {
    let config = rules(top, &base, target)?;
    let mut best: Option<Computed> = None;
    for head in &heads {
        let range = format!("{base}...{head}");
        let args = ["-c", "core.quotePath=false", "diff", "--no-ext-diff", "--numstat", "-z", "--find-renames", &range];
        let c = risk::computed(&risk::numstat(&git(top, &args)?), &config);
        if best.as_ref().is_none_or(|b| c.level > b.level) {
            best = Some(c);
        }
    }
    let computed = best.expect("one head at least");
    Ok(Judged { computed, base, heads, top: top.to_path_buf() })
}

/// The sha origin's server names for `refs/heads/<branch>`, fetched; `None` when it holds no such branch. A failed
/// call refuses: old refs would judge another head by stale rules. Nothing of the clone's own names the sha: a
/// `remote.origin.fetch` refspec, `git update-ref` or `git remote set-head` moves each local ref with no push.
pub(crate) fn at_origin(top: &Path, branch: &str) -> Result<Option<String>, Refusal> {
    let want = format!("refs/heads/{branch}");
    let listed = git(top, &["ls-remote", "origin", &want])?;
    let sha = listed.lines().find_map(|l| l.split_once('\t').filter(|(_, r)| *r == want).map(|(sha, _)| sha));
    let Some(sha) = sha.filter(|s| s.len() >= 40 && s.bytes().all(|b| b.is_ascii_hexdigit())) else { return Ok(None) };
    git(top, &["fetch", "-q", "origin", sha])
        .map_err(|e| Refusal::failed(e.subject, format!("git cannot fetch {sha}, origin's {branch}: {}", e.fix)))?;
    Ok(Some(sha.to_string()))
}

/// `.bp/risk.yml` on the base commit; the default when it holds none. An exact pattern that names a directory of the
/// base tree, in any ASCII case, refuses as a parse failure does: the human wrote a directory without its `/`. A
/// directory only a head adds is the branch's, not the file's: a high exact pattern also holds each path under it, so
/// the head reads high and the refusal never blames the file on the base (review of #95, row 26).
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
    let listed = git(top, &["ls-tree", "-r", "-d", "-z", "--name-only", base])?;
    let dirs: BTreeSet<String> = listed.split('\0').map(str::to_ascii_lowercase).collect();
    let exact = config.high.iter().chain(&config.low).filter_map(|p| match p {
        Pattern::Exact(e) => Some(e.as_str()),
        _ => None,
    });
    if let Some(path) = exact.into_iter().find(|p| dirs.contains(&p.to_ascii_lowercase())) {
        return Err(invalid(format!("the pattern {path:?} names a directory, so it matches no path: write {path}/")));
    }
    Ok(config)
}
