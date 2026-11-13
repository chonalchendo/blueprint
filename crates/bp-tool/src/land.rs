//! `bp pr <issue>` and `bp land <issue>`: the pull request and its merge (15, the repo verbs; 26, design changes 3, 4
//! and 6). `pr` fetches origin and refuses a head behind its base (34, design change 5), refuses a block that passes at
//! the red-first base (37, design change 10), runs the issue's blocks at the worktree's head with no lock held, pushes
//! that head, opens or retargets the pull request, keeps its body current with what it carries (27, design change 8)
//! and commits the runs, dropping the claim's `## Next`; a changed check set first gets a commit that names it. `land`
//! gates by the risk level (37, design change 3): it reads no forge approval. It reads the forge with no lock held and
//! refuses on the base, the forge check, the run evidence, the reviews of the head's round, their dispositions, an
//! error row that holds, for high the human's signed approval of the head, the check set the head names, a level
//! that rose after `pr` and then a head behind its base, in that order, for a pull request already merged too, which
//! skips the last. The session runs it; a low or medium head lands with no human act. It merges at the head it read
//! and moves each layer cut from it onto the base. Then it removes the worktree and a branch the merged head holds, and
//! commits the claim's release and the issue at `resolved`; the result names each moved layer's next step, and a
//! stranded layer's held claim takes its step as `## Next`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use bp_change::IssueStatus;
use bp_change::intents::{self, Base, Issue, Unmet};
use bp_forge::{Gh, PullRequest};
use bp_read::inputs::{field, held};
use bp_read::landing;
use bp_read::snapshot::clone_of;
use bp_vault::{Code, Id, Index, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::claim::{default_branch, git};
use crate::{Wrote, finish, node_id, subject};

mod level;
mod merging;

use level::{lander, risen, trusted};
use merging::{kept, merge, remove, worktree_clean};

/// How long one `gh` or git call of `land` may run before it refuses `could_not_run` (review of #100, F11).
const DEADLINE: Duration = Duration::from_secs(120);

/// `bp pr <issue>`.
#[derive(Args)]
pub struct Pr {
    /// The claimed issue whose branch goes to the forge.
    pub issue: String,
}

/// `bp land <issue> [--dry-run]`.
#[derive(Args)]
pub struct Land {
    /// The claimed issue whose pull request merges.
    pub issue: String,
    /// Run every gate and print the refusal land would give at this head, or the head it would merge; fetch origin,
    /// but never push, merge, release or write.
    #[arg(long)]
    pub dry_run: bool,
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
    // The forge merges no head behind its base, so `pr` refuses one before the runs and the push (34, design change 5).
    git(&c.top, &["fetch", "-q", "origin"])?;
    let head = git(&c.worktree, &["rev-parse", "HEAD"])?;
    holds_base(&c, &id, &head, false)?;
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(now));
    let carried = red_first(&vault, &index, &id, &c, &head, role.as_str(), &clock)?;
    let gh = Gh::new(&c.top);
    let open = gh.prs_for_head(&c.branch)?.into_iter().find(PullRequest::is_open);
    let (runs, body) = (bp_read::inputs::runs(&index, id.as_str())?, carried.body());
    // The forge's body, read once: a stale one is rewritten at any head.
    let stale = open.as_ref().map(|o| gh.body(o.number).map(|b| !carried.describes(&b))).transpose()?.unwrap_or(true);
    let named = carried.named_by(&c.worktree, &head)?;
    // A failed, unrun or stale block at the head runs again; only passing evidence of each block, a current body and
    // a head that names the check set is a repeat.
    if let Some(o) = open.as_ref().filter(|o| o.head_ref_oid == head && o.base_ref_name.as_ref() == Some(&c.target))
        && intents::evidence(&runs, &head, &blocks).is_ok()
        && intents::recorded_risk(&runs, &head).is_some()
        && !stale
        && named
    {
        let row = json!({ "id": id, "pull_request": o.number, "head": head });
        return Ok(Opened::Wrote(Wrote::Already(carried.listed(row))));
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
    let ran = bp_check::run(&vault, &id, &c.worktree, None, &[], &[], &clock)?.expect("the issue has a named block");
    // The head the runs name is the one pushed, even if a commit landed in the worktree while they ran.
    let head = ran.commit().to_string();
    // Each run records the level of the head it names, judged after a fetch: an intent reads it there and fetches
    // nothing (22, constraint 8). `land` judges again after its own fetch.
    let computed = crate::risk::judge(&c.top, &c.target, vec![head.clone()])?.computed;
    let ran = ran.with_risk(computed.level.as_str(), computed.security);
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
    let (mut changes, runs) = ran.records(&vault);
    // The head holds its base, so a stranding step on the claim is done (34, design change 5).
    changes.extend(cleared(&vault, writer.index(), &id, &c.claim)?);
    let written = writer.commit("pr", role.as_str(), &format!("pr {id}"), &changes)?;
    let listed = carried.listed(json!({ "pull_request": number }));
    if let Some(at) = runs.iter().position(bp_check::Run::failed) {
        return Ok(Opened::Failed { at, commit: written.commit, runs, listed });
    }
    Ok(Opened::Wrote(Wrote::Changed(carried.listed(json!({
        "id": id,
        "pull_request": number,
        "head": head,
        "branch": c.branch,
        "base": base,
        "runs": crate::check::rows(&runs),
        "commit": written.commit,
        "findings": written.findings,
    })))))
}

pub(crate) fn land(cwd: &Path, l: &Land) -> Result<Wrote, Refusal> {
    let role = lander(&l.issue)?;
    node_id(&l.issue)?;
    // The clock refuses before any forge call, dry run too, never after the merge (review of #100, F6).
    let now = crate::clock::now()?;
    bp_read::git::within(DEADLINE);
    let vault = Vault::discover(cwd)?;
    let mut index = Index::open(&vault)?;
    let id = subject(&index, &l.issue)?;
    if let Some(done) = repeat(&vault, &mut index, &id, l.dry_run)? {
        return Ok(done);
    }
    let resolved = IssueStatus::Resolved.as_str();
    let c = claimed(cwd, &index, &id)?;
    let gh = Gh::new(&c.top).within(DEADLINE);
    // The head's ancestry and its level are tested against the base as origin holds it now. The dry run fetches too,
    // so a dry run that passes is a land the forge accepts (34, design change 5).
    git(&c.top, &["fetch", "-q", "origin"])?;
    let Some(pr) =
        gh.prs_for_head(&c.branch)?.into_iter().filter(|p| p.is_open() || merged(p)).max_by_key(|p| p.number)
    else {
        return Err(landing::no_pull_request(&id));
    };
    let n = pr.number;
    level::shaped(&c, &pr)?;
    // The dry run takes no writer, so it runs the writer's commit precondition here (34, design change 7).
    vault.committed()?;
    vault.clean([&id, &c.claim])?;
    worktree_clean(&c, &id)?;
    let blocks = bp_read::inputs::blocks(&vault, id.as_str())?;
    let mut issue = Issue { base: c.base.clone(), blocks, ..bp_read::inputs::reviewed(&vault, &index, id.as_str())? };
    let read = bp_read::inputs::pull_request(&pr);
    let carried = crate::carry::read(&vault, &index, &id, crate::red_first::Based::default())?;
    let level = trusted(&c, &gh, &index, &id, &pr, &mut issue)?;
    // A merged pull request passes the same gates: a land that stopped before its commit passes them again, as the
    // records and the rollup outlive the merge, and a merge made outside `land` is not recorded as landed.
    // The check set, the level after this fetch and then the head's ancestry are the last gates, so a head the clone
    // cannot read refuses only when every other gate holds.
    let named = carried.named_by(&c.top, &pr.head_ref_oid);
    let gate = match intents::gates(&issue, &read, named.as_ref().ok().copied()).unmet {
        Some(Unmet::Approval) => Err(level.1.unwrap_or_else(|| landing::refusal(&Unmet::Approval, &id, &pr))),
        Some(unmet) => Err(landing::refusal(&unmet, &id, &pr)),
        // The base the forge names is read after the gates above it, so a layer whose base has not landed gets
        // `base_unmerged` and not a refusal about a sha its own land replaces (review of #100, V6).
        None => named
            .and_then(|_| level::base(&c, &id, &pr))
            .and_then(|base| risen(&c, &id, &issue, &pr.head_ref_oid, base))
            .and_then(|()| {
                // A merged head is behind its base by its own merge.
                if merged(&pr) { Ok(()) } else { holds_base(&c, &id, &pr.head_ref_oid, true) }
            }),
    };
    if let Err(mut r) = gate {
        // Only a gate that refuses a merged pull request makes the merge lost: a tool that could not run keeps its
        // own fix, and the rerun finishes the land (review of #100, F2).
        if merged(&pr) && r.exit == 2 {
            r.fix = landing::after_merge(n, &id);
        }
        return Err(r);
    }
    let level = level.0;
    // Each layer above, its worktree and its claim are tested before the merge, and by the dry run, as the land
    // after the merge would test them (review of #100, F7).
    let children = crate::stack::children(&index, &id)?;
    if !children.is_empty() {
        vault.clean(children.iter().map(|child| &child.claim))?;
    }
    if l.dry_run {
        return Ok(Wrote::Changed(json!({ "land": id, "head": pr.head_ref_oid, "level": level, "dry_run": true })));
    }
    let pr = if merged(&pr) { pr } else { merge(&vault, &gh, &c, &id, &pr)? };
    drop(index);
    kept(&vault, &id, &c, &pr)?;
    // Each layer above moves onto the base before the branch below goes (15, a dependency chain to landed).
    git(&c.top, &["fetch", "-q", "origin"])?;
    let moved = crate::stack::restack(&gh, &children, &c.target, &c.branch)?;
    let kept = remove(&c, &pr.head_ref_oid, !moved.stranded.is_empty())?;
    let mut writer = vault.writer()?;
    writer.clean([&id, &c.claim].into_iter().chain(moved.notes.iter().map(|(_, claim, _)| claim)))?;
    // Another write may have landed while the forge merged: the claim must still be the one this land read.
    if held(writer.index(), &id)?.is_none_or(|h| h.id != c.claim.as_str()) {
        return Err(Refusal::failed(
            c.claim.as_str(),
            format!("pull request {} merged, but the claim closed meanwhile; bp show {id}", pr.number),
        ));
    }
    let to = crate::clock::instant(crate::clock::now().unwrap_or(now));
    let more = format!("pull_request: {}\n", pr.number);
    let mut changes = BTreeMap::from([
        (c.claim.clone(), crate::claim::released(&vault.text(&c.claim)?, &to, "landed", &more)),
        (id.clone(), bp_vault::set_field(&vault.text(&id)?, "status", &resolved.into())),
    ]);
    // A stranded layer's step goes on its claim too, so its walk carries it to the next session (34, design change 5).
    changes.extend(noted(&vault, writer.index(), &moved)?);
    writer.trailer(format!("Bp-Claim: {}", c.claim));
    let written = writer.commit("land", role.as_str(), &format!("land {id}"), &changes)?;
    Ok(Wrote::Changed(json!({
        "id": id,
        "claim": c.claim,
        "status": resolved,
        "pull_request": pr.number,
        "head": pr.head_ref_oid,
        "level": level,
        "branch_kept": kept,
        "restacked": moved.restacked,
        "stranded": moved.stranded,
        "next": moved.next,
        "commit": written.commit,
        "findings": written.findings,
    })))
}

/// The repeat of a land already done: the issue is `resolved` (exit 3). A land killed inside its vault commit leaves
/// `resolved` on disk, uncommitted, beside its journal; a writer puts the pre-images back first, so the rerun lands
/// again and writes the commit (review of #100, F9). The dry run writes nothing, so it reads the file as it stands.
fn repeat(vault: &Vault, index: &mut Index, id: &Id, dry_run: bool) -> Result<Option<Wrote>, Refusal> {
    let resolved = IssueStatus::Resolved.as_str();
    let status = |index: &Index| -> Result<bool, Refusal> {
        Ok(index.header(id.as_str())?.and_then(|h| h.get("status")?.as_str().map(str::to_string)).as_deref()
            == Some(resolved))
    };
    if status(index)? && vault.clean([id]).is_err() && !dry_run {
        drop(vault.writer()?);
        *index = Index::open(vault)?;
    }
    Ok(status(index)?.then(|| Wrote::Already(json!({ "id": id, "status": resolved }))))
}

/// `head_behind_base` unless `head` holds origin's `target` as last fetched: the forge merges no head behind its base
/// (34, design change 5). The fix rebases the branch's own commits: those past the claim's base while the head holds
/// it, else, as for a layer `land` moved, those past the point where the head leaves the base. That point is too low
/// when the layer below was rewritten twice after this head left the claim's base: the rebase then replays the lower
/// layer's older commits too, which git drops when their patches match and which conflict when they do not. Recording
/// the base at each restack and each passing `pr` would close it.
/// `land` names the same rebase as the worker's act, which the session dispatches (review of #100, C3).
fn holds_base(c: &Claimed, id: &Id, head: &str, land: bool) -> Result<(), Refusal> {
    let tip = git(&c.top, &["rev-parse", &format!("refs/remotes/origin/{}", c.target)])?;
    let fork = git(&c.top, &["merge-base", &tip, head])?;
    if fork == tip {
        return Ok(());
    }
    let own = |from: &String| git(&c.top, &["merge-base", "--is-ancestor", from, head]).is_ok();
    let from = c.from.clone().filter(own).unwrap_or(fork);
    let (t, w) = (&c.target, c.worktree.display());
    let step = format!("git -C {w} rebase --onto origin/{t} {from}, resolve any conflict, then bp pr {id}");
    let fix = if land {
        format!("{head} lacks origin/{t} at {tip}; dispatch a worker to run {step}; then bp land {id}")
    } else {
        format!("{head} lacks origin/{t} at {tip}; {step}")
    };
    Err(Refusal::new(
        Code::HeadBehindBase,
        id.as_str(),
        "a head lands only when it holds origin's base branch: the forge merges no head behind it (34, design change 5)",
        fix,
    ))
}

/// What the pull request carries, after red-first: a block that passes where the head leaves the target refuses
/// before the runs at the head and the push (37, design change 10).
fn red_first(
    vault: &Vault,
    index: &Index,
    id: &Id,
    c: &Claimed,
    head: &str,
    role: &str,
    clock: &dyn Fn() -> String,
) -> Result<crate::carry::Carried, Refusal> {
    let from = crate::red_first::base(&c.top, &c.target, head)?;
    let based = crate::red_first::base_runs(vault, index, id, &c.top, &from, role, clock)?;
    crate::carry::read(vault, index, id, based)
}

/// The `## Next` write for each layer `land` stranded, on its claim while that claim is still held: a layer released
/// while the forge merged keeps `## Released` as its closing append (13, Claim).
fn noted(vault: &Vault, index: &Index, moved: &crate::stack::Moved) -> Result<Vec<(Id, String)>, Refusal> {
    let mut out = Vec::new();
    for (layer, claim, step) in &moved.notes {
        if held(index, layer)?.is_some_and(|h| h.id == claim.as_str()) {
            out.push((claim.clone(), crate::claim::next(&vault.text(claim)?, step)));
        }
    }
    Ok(out)
}

/// The held claim without its `## Next`, when it has one: `pr` passed the ancestry test, so the step is done.
fn cleared(vault: &Vault, index: &Index, id: &Id, claim: &Id) -> Result<Option<(Id, String)>, Refusal> {
    if held(index, id)?.is_none_or(|h| h.id != claim.as_str()) {
        return Ok(None);
    }
    Ok(crate::claim::without_next(&vault.text(claim)?).map(|text| (claim.clone(), text)))
}

/// The issue's active claim, its clone, its branch and its base branch, or `illegal_transition`: the issue's status
/// is not one where `pr` and `land` start, which is `in-progress` under a claim. The fix names the verb that moves it.
fn claimed(cwd: &Path, index: &Index, id: &Id) -> Result<Claimed, Refusal> {
    let Some(h) = held(index, id)? else {
        let status = index.header(id.as_str())?.and_then(|h| h.get("status")?.as_str().map(str::to_string));
        return Err(landing::unheld(id, status.as_deref()));
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

fn merged(p: &PullRequest) -> bool {
    p.state.as_deref() == Some("MERGED")
}
