//! `bp pr <issue>` and `bp land <issue>`: the pull request and its merge (15, the repo verbs; 26, design changes 3,
//! 4 and 6). `pr` runs the issue's blocks at the worktree's head with no lock held, pushes that head, opens or
//! retargets the pull request, keeps its body current with what it carries (27, design change 8) and commits the
//! runs; a changed check set first gets a commit that names it. `land` reads the forge with no lock held and refuses
//! on the reviewer, the base, the approval at the head, the forge check, the run evidence and the check set the head
//! names, in that order, for a pull request already merged too. It merges at the head it read and moves each layer
//! cut from it onto the base. Then it removes the worktree and a branch the merged head holds, and commits the claim's
//! release and the issue at `resolved`; the result names each moved layer's next step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bp_change::IssueStatus;
use bp_change::intents::{self, Base, Unmet};
use bp_forge::{Gh, PullRequest};
use bp_read::inputs::{field, held};
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::claim::{default_branch, git};
use crate::{Wrote, finish, node_id, subject};

/// `bp pr <issue>`.
#[derive(Args)]
pub struct Pr {
    /// The claimed issue whose branch goes to the forge.
    pub issue: String,
}

/// `bp land <issue>`.
#[derive(Args)]
pub struct Land {
    /// The claimed issue whose pull request merges.
    pub issue: String,
}

/// What a `pr` did: opened or found its head done, or committed the runs of a failed block and kept its push.
enum Opened {
    Wrote(Wrote),
    Failed { at: usize, commit: String, runs: Vec<bp_check::Run>, listed: Value },
}

/// The active claim a repo verb acts under, where its work lives and the branch it lands on.
struct Claimed {
    claim: Id,
    top: PathBuf,
    branch: String,
    worktree: PathBuf,
    /// The commit the worktree was cut from: the base of the diff a reviewer saw.
    from: Option<String>,
    /// The base `land` tests (15, a dependency chain to landed).
    base: Base,
    /// The branch the pull request targets: the base branch, or while the layer below is open, its branch.
    target: String,
}

pub(crate) fn run_pr(cwd: &Path, p: &Pr, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match pr(cwd, p) {
        Ok(Opened::Wrote(w)) => crate::wrote(Ok(w), json, out),
        Ok(Opened::Failed { at, commit, runs, listed }) => {
            let then = format!("commit it and bp pr {}", p.issue);
            crate::check::failed(&runs[at], &then, &commit, crate::check::rows(&runs), Some(listed), json, out)
        }
        Err(refusal) => finish::<Value>(Err(refusal), json, out),
    }
}

fn pr(cwd: &Path, p: &Pr) -> Result<Opened, Refusal> {
    node_id(&p.issue)?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let now = crate::clock::now()?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &p.issue)?;
    let c = claimed(cwd, &index, &id)?;
    if !c.worktree.is_dir() {
        return Err(Refusal::failed(
            c.worktree.display().to_string(),
            format!("the claim's worktree is gone; bp cut {id} puts it back from its branch, then rerun"),
        ));
    }
    let blocks = bp_read::inputs::blocks(&vault, id.as_str())?;
    if blocks.is_empty() {
        return Err(Refusal::new(
            Code::CheckEmpty,
            id.as_str(),
            "land needs a passing run of each error block at the head, so an issue has one (21, derive)",
            format!(
                "add a named error-severity check block to {id}'s ## Check: bp set {id} --body-file <file>, then rerun"
            ),
        ));
    }
    let gh = Gh::new(&c.top);
    let head = git(&c.worktree, &["rev-parse", "HEAD"])?;
    let open = gh.prs_for_head(&c.branch)?.into_iter().find(PullRequest::is_open);
    let runs = bp_read::inputs::runs(&index, id.as_str())?;
    let carried = crate::carry::read(&vault, &index, &id)?;
    let body = carried.body();
    // The forge's body, read once: a stale one is rewritten at any head.
    let stale = open.as_ref().map(|o| gh.body(o.number).map(|b| !carried.describes(&b))).transpose()?.unwrap_or(true);
    let named = carried.named_by(&c.worktree, &head)?;
    // A failed, unrun or stale block at the head runs again; only passing evidence of each block, a current body and
    // a head that names the check set is a repeat.
    if let Some(o) = open.as_ref().filter(|o| o.head_ref_oid == head && o.base_ref_name.as_ref() == Some(&c.target))
        && intents::evidence(&runs, &head, &blocks).is_ok()
        && !stale
        && named
    {
        return Ok(Opened::Wrote(Wrote::Already(json!({
            "id": id,
            "pull_request": o.number,
            "head": head,
            "deltas": carried.deltas,
            "diagrams": carried.diagrams(),
            "check_changes": carried.changes(),
        }))));
    }
    let base = c.target.clone();
    let title = index.header(id.as_str())?.and_then(|h| h.get("title")?.as_str().map(str::to_string));
    let title = title.unwrap_or_else(|| id.to_string());
    // The blocks may take minutes, so they run with no lock held; a dirty worktree refuses here, before the push.
    drop(index);
    // A changed check set goes to the forge as a commit, before the runs, so they name the head that is pushed.
    if !named {
        carried.name_set(&c.worktree, &head)?;
    }
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(now));
    let ran = bp_check::run(&vault, &id, &c.worktree, None, &[], &clock)?.expect("an error block is a named block");
    // The head the runs name is the one pushed, even if a commit landed in the worktree while they ran.
    let head = ran.commit().to_string();
    let refspec = format!("{head}:refs/heads/{}", c.branch);
    // A rebased branch replaces the forge's head, but only the head the forge showed: a push since then refuses.
    let lease = open.as_ref().map(|o| format!("--force-with-lease=refs/heads/{}:{}", c.branch, o.head_ref_oid));
    let push: Vec<&str> =
        ["push", "-q", "origin"].into_iter().chain(lease.as_deref()).chain([refspec.as_str()]).collect();
    git(&c.top, &push)?;
    let number = match &open {
        Some(o) if o.base_ref_name.as_deref() == Some(base.as_str()) => o.number,
        Some(o) => {
            gh.retarget(o.number, &base)?;
            o.number
        }
        None => gh.create(&c.branch, &base, &title, &body)?,
    };
    if open.is_some() && stale {
        gh.describe(number, &body)?;
    }
    let writer = ran.writer(&vault, &format!("bp pr {id}"))?;
    let (changes, runs) = ran.records(&vault);
    let written = writer.commit("pr", role.as_str(), &format!("pr {id}"), &changes)?;
    let listed = json!({ "pull_request": number, "deltas": carried.deltas,
            "diagrams": carried.diagrams(), "check_changes": carried.changes() });
    if let Some(at) = runs.iter().position(bp_check::Run::failed) {
        return Ok(Opened::Failed { at, commit: written.commit, runs, listed });
    }
    Ok(Opened::Wrote(Wrote::Changed(json!({
        "id": id,
        "pull_request": number,
        "head": head,
        "branch": c.branch,
        "base": base,
        "runs": crate::check::rows(&runs),
        "deltas": carried.deltas,
            "diagrams": carried.diagrams(),
        "check_changes": carried.changes(),
        "commit": written.commit,
        "findings": written.findings,
    }))))
}

pub(crate) fn land(cwd: &Path, l: &Land) -> Result<Wrote, Refusal> {
    let role = crate::set::human(format!("land {}", l.issue))?;
    node_id(&l.issue)?;
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &l.issue)?;
    let resolved = IssueStatus::Resolved.as_str();
    let status = index.header(id.as_str())?.and_then(|h| h.get("status")?.as_str().map(str::to_string));
    if status.as_deref() == Some(resolved) {
        return Ok(Wrote::Already(json!({ "id": id, "status": resolved })));
    }
    let c = claimed(cwd, &index, &id)?;
    let gh = Gh::new(&c.top);
    let reviewer = reviewer(&index, &id, &gh.login()?)?;
    // The diff an earlier approval saw is compared with the base as origin holds it now.
    git(&c.top, &["fetch", "-q", "origin"])?;
    let Some(pr) =
        gh.prs_for_head(&c.branch)?.into_iter().filter(|p| p.is_open() || merged(p)).max_by_key(|p| p.number)
    else {
        return Err(Refusal::new(
            Code::ReviewMissing,
            id.as_str(),
            "land merges an open pull request the reviewer identity approved (15, Controls 1)",
            format!("{} has no open pull request; bp pr {id}, then the reviewer approves it", c.branch),
        ));
    };
    let n = pr.number;
    vault.clean([&id, &c.claim])?;
    if c.worktree.is_dir() && !git(&c.worktree, &["status", "--porcelain"])?.is_empty() {
        return Err(Refusal::new(
            Code::PathDirty,
            c.worktree.display().to_string(),
            "land removes the worktree, so it holds no uncommitted work (26, design change 2)",
            format!("commit and bp pr {id}, or stash, the changes in {}, then rerun", c.worktree.display()),
        ));
    }
    let runs = bp_read::inputs::runs(&index, id.as_str())?;
    let blocks = bp_read::inputs::blocks(&vault, id.as_str())?;
    let read = bp_read::inputs::pull_request(&pr, &reviewer);
    let carried = crate::carry::read(&vault, &index, &id)?;
    // A merged pull request passes the same gates: a land that stopped before its commit passes them again, as the
    // reviews and the rollup outlive the merge, and a merge made outside `land` is not recorded as landed.
    let gate = intents::landing(&read, &c.base, &runs, &blocks)
        .map_err(|unmet| refusal(&unmet, &c, &id, &pr, &reviewer))
        .and_then(|()| checks(&carried, &c, &id, &pr));
    if let Err(mut r) = gate {
        if merged(&pr) {
            // No approval or run can follow a merge: the human judges a change that skipped a gate.
            r.fix = format!(
                "pull request {n} is merged, so nothing can meet this now and nothing is recorded; the human reviews \
                 the merge, then bp release {id} --reason lost"
            );
        }
        return Err(r);
    }
    let children = crate::stack::children(&index, &id)?;
    if !merged(&pr) {
        gh.merge(n, &pr.head_ref_oid).map_err(|e| match gh.pr(n) {
            // The forge refuses a head that moved since the reads: that head has no approval yet.
            Ok(now) if now.head_ref_oid != pr.head_ref_oid => Refusal::new(
                Code::ReviewMissing,
                id.as_str(),
                "land merges only the head the reviewer identity approved (15, Controls 1)",
                format!(
                    "the head moved to {}; bp pr {id}, then the reviewer approves it, then bp land {id}",
                    now.head_ref_oid
                ),
            ),
            _ => e,
        })?;
    }
    drop(index);
    // Each layer above moves onto the base before the branch below goes (15, a dependency chain to landed).
    git(&c.top, &["fetch", "-q", "origin"])?;
    let moved = crate::stack::restack(&gh, &children, &c.target, &c.branch)?;
    let kept = remove(&c, &pr.head_ref_oid, !moved.stranded.is_empty())?;
    let mut writer = vault.writer()?;
    writer.clean([&id, &c.claim])?;
    // Another write may have landed while the forge merged: the claim must still be the one this land read.
    if held(writer.index(), &id)?.is_none_or(|h| h.id != c.claim.as_str()) {
        return Err(Refusal::failed(
            c.claim.as_str(),
            format!("pull request {} merged, but the claim closed meanwhile; bp show {id}", pr.number),
        ));
    }
    let to = crate::clock::instant(crate::clock::now()?);
    let more = format!("pull_request: {}\n", pr.number);
    let changes = BTreeMap::from([
        (c.claim.clone(), crate::claim::released(&vault.text(&c.claim)?, &to, "landed", &more)),
        (id.clone(), bp_vault::set_field(&vault.text(&id)?, "status", &resolved.into())),
    ]);
    writer.trailer(format!("Bp-Claim: {}", c.claim));
    let written = writer.commit("land", role.as_str(), &format!("land {id}"), &changes)?;
    Ok(Wrote::Changed(json!({
        "id": id,
        "claim": c.claim,
        "status": resolved,
        "pull_request": pr.number,
        "head": pr.head_ref_oid,
        "branch_kept": kept,
        "restacked": moved.restacked,
        "stranded": moved.stranded,
        "next": moved.next,
        "commit": written.commit,
        "findings": written.findings,
    })))
}

/// The project's reviewer identity, or `reviewer_invalid` when it is empty or the token's own `login`, case ignored.
fn reviewer(index: &Index, id: &Id, login: &str) -> Result<String, Refusal> {
    let reviewer = bp_read::inputs::reviewer(index, id.as_str())?;
    if !reviewer.is_empty() && !reviewer.eq_ignore_ascii_case(login) {
        return Ok(reviewer);
    }
    let project = id.project().unwrap_or_default();
    let why = if reviewer.is_empty() {
        "the project names no reviewer".into()
    } else {
        format!("{login} is the token's own login")
    };
    Err(Refusal::new(
        Code::ReviewerInvalid,
        format!("projects/{project}/project"),
        "land counts only the reviewer identity's approval, a login other than the token's (26, design change 3)",
        format!(
            "{why}; the human sets the second identity's forge login: bp set projects/{project}/project reviewer <login>"
        ),
    ))
}

/// The issue's active claim, its clone, its branch and its base branch, or `illegal_transition`: the issue's status
/// is not one where `pr` and `land` start, which is `in-progress` under a claim. The fix names the verb that moves it.
fn claimed(cwd: &Path, index: &Index, id: &Id) -> Result<Claimed, Refusal> {
    let Some(h) = held(index, id)? else {
        let status = index.header(id.as_str())?.and_then(|h| h.get("status")?.as_str().map(str::to_string));
        return Err(Refusal::new(
            Code::IllegalTransition,
            id.as_str(),
            "pr and land act on an issue under an active claim; cut makes it (21, claim)",
            crate::claim::unclaimed(id, status.as_deref()),
        ));
    };
    let top = clone_of(cwd, index, id.project().unwrap_or_default())?;
    // A claim cut before `base_branch` existed lands on origin's default branch, where `cut` started it. A layer whose
    // dependency is open targets the dependency's branch, its own `base_branch`.
    let (base, target) = match bp_read::inputs::base(index, &h)? {
        Base::Branch(b) => (Base::Branch(b.clone()), b),
        Base::Unknown => {
            let b = default_branch(&top)?;
            (Base::Branch(b.clone()), b)
        }
        unmerged @ Base::Unmerged(_) => (unmerged, field(&h, "base_branch").unwrap_or_default()),
    };
    Ok(Claimed {
        claim: Id::parse(&h.id).expect("the index holds ids"),
        branch: crate::claim::branch(id),
        worktree: field(&h, "worktree").map(PathBuf::from).unwrap_or_default(),
        from: field(&h, "base"),
        top,
        base,
        target,
    })
}

/// No approval at the head: `review_missing`, or `diff_changed` when the reviewer's latest approval, at an earlier
/// head, saw another diff. The comparison only names the refusal; the forge merges an approval of the latest push
/// alone (15, Controls 1).
fn unapproved(c: &Claimed, id: &Id, pr: &PullRequest, reviewer: &str) -> Refusal {
    let (n, head) = (pr.number, &pr.head_ref_oid);
    let rule = "land counts only the reviewer identity's approving review at the head (15, Controls 1)";
    let again = format!("the human approves pull request {n} at {head} as {reviewer}, then bp land {id}");
    let Some(&earlier) = pr.approved_heads(reviewer).last() else {
        return Refusal::new(Code::ReviewMissing, id.as_str(), rule, again);
    };
    match c.from.as_deref().and_then(|from| crate::stack::diff_unchanged(&c.top, from, earlier, pr)) {
        Some(false) => Refusal::new(
            Code::DiffChanged,
            id.as_str(),
            "the diff changed since the reviewer identity's latest approval (15, Controls 1)",
            format!("{reviewer} approved {earlier}, and the diff at {head} differs; {again}"),
        ),
        Some(true) => {
            Refusal::new(Code::ReviewMissing, id.as_str(), rule, format!("diff unchanged since {earlier}; {again}"))
        }
        None => Refusal::new(
            Code::ReviewMissing,
            id.as_str(),
            rule,
            format!("{reviewer} approved {earlier}, which this clone cannot compare; {again}"),
        ),
    }
}

/// `checks_changed`: the head's `Bp-Checks` trailer, or the set at the claim when it has none, names another check
/// set than the one the pull request carries now. The reviewer approved that head, so a block changed since then is
/// one the reviewer has not seen (27, design change 8).
fn checks(carried: &crate::carry::Carried, c: &Claimed, id: &Id, pr: &PullRequest) -> Result<(), Refusal> {
    if carried.named_by(&c.top, &pr.head_ref_oid)? {
        return Ok(());
    }
    Err(Refusal::new(
        Code::ChecksChanged,
        id.as_str(),
        "land merges a head that names the error blocks the pull request carries now (27, design change 8)",
        format!(
            "an error block of {id}, its milestone or a carried delta changed after {} was pushed; bp pr {id} pushes \
             a commit that names them, then the reviewer approves pull request {} at its new head, then bp land {id}",
            pr.head_ref_oid, pr.number
        ),
    ))
}

/// The first unmet part of needs landing as its refusal (26, design change 4).
fn refusal(unmet: &Unmet, c: &Claimed, id: &Id, pr: &PullRequest, reviewer: &str) -> Refusal {
    let (n, head) = (pr.number, &pr.head_ref_oid);
    match unmet {
        Unmet::Unmerged(below) => Refusal::new(
            Code::BaseUnmerged,
            id.as_str(),
            "a layer lands after the layer it was cut from (15, a dependency chain to landed)",
            format!("bp land {below} first: it moves pull request {n} onto its base; then bp land {id}"),
        ),
        Unmet::Mismatch(base) => Refusal::new(
            Code::BaseMismatch,
            id.as_str(),
            "land merges into the claim's base branch only (26, design change 4)",
            format!(
                "pull request {n} targets {}; bp pr {id} retargets it to {base}, then bp land {id}",
                pr.base_ref_name.as_deref().unwrap_or_default()
            ),
        ),
        Unmet::Review => unapproved(c, id, pr, reviewer),
        Unmet::Checks => Refusal::new(
            Code::ChecksPending,
            id.as_str(),
            "the forge check at the head is green (15, Controls 3)",
            if pr.has_failed() {
                format!("a check of pull request {n} failed at {head}; fix it, commit it, then bp pr {id}")
            } else {
                format!("wait for the checks of pull request {n} at {head} to pass, then bp land {id}")
            },
        ),
        Unmet::Failed(run) => Refusal::new(
            Code::CheckFailed,
            run.id.as_str(),
            "every error-severity check block passes at the head (26, design change 4)",
            format!(
                "{} did not pass at {head}; read it with bp show {}, fix the cause, commit it, then bp pr {id}",
                run.check, run.id
            ),
        ),
        Unmet::Evidence(block) => Refusal::new(
            Code::EvidenceStale,
            id.as_str(),
            "land needs a passing run of each error block's current text at the head (26, design change 4)",
            format!("{block} has no run of its current text at {head}; bp pr {id}, then bp land {id}"),
        ),
    }
}

/// Removes what the claim added to the clone: the worktree, the branch and the forge's copy of it. Each step skips
/// what is already gone, so a land that stopped part way finishes on its rerun. A branch with a commit the merged
/// head lacks stays, so no work is lost, and so does one a `stranded` layer still targets; the result is `true` then.
fn remove(c: &Claimed, merged: &str, stranded: bool) -> Result<bool, Refusal> {
    if c.worktree.is_dir() {
        git(&c.top, &["worktree", "remove", &c.worktree.display().to_string()])?;
    }
    if stranded {
        return Ok(true);
    }
    git(&c.top, &["fetch", "-q", "--prune", "origin"])?;
    let exists = |r: &str| git(&c.top, &["rev-parse", "--verify", "-q", r]).is_ok();
    // A squash merge leaves the branch unmerged by ancestry, so the test is the merged head, not the base.
    let held = |r: &str| git(&c.top, &["merge-base", "--is-ancestor", r, merged]).is_ok();
    let (local, remote) = (format!("refs/heads/{}", c.branch), format!("refs/remotes/origin/{}", c.branch));
    let mut kept = false;
    if exists(&local) {
        if held(&local) {
            git(&c.top, &["branch", "-q", "-D", &c.branch])?;
        } else {
            kept = true;
        }
    }
    if exists(&remote) {
        if held(&remote) {
            git(&c.top, &["push", "-q", "origin", "--delete", &c.branch])?;
        } else {
            kept = true;
        }
    }
    Ok(kept)
}

fn merged(p: &PullRequest) -> bool {
    p.state.as_deref() == Some("MERGED")
}
