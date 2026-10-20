//! `bp pr <issue>` and `bp land <issue>`: the pull request and its merge (15, the repo verbs; 26, design changes 3,
//! 4 and 6). `pr` runs the issue's blocks at the worktree's head with no lock held, pushes that head, opens or
//! retargets the pull request and commits the runs. `land` reads the forge with no lock held and refuses on the
//! reviewer, the review, the forge check and the run evidence, in that order. It merges at the head it read, removes
//! the worktree and the branch, then commits the claim's release and the issue at `resolved`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bp_change::IssueStatus;
use bp_change::intents::{self, Unmet};
use bp_forge::{Gh, PullRequest};
use bp_vault::{Code, Id, Index, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::claim::{clone_of, field, git, held};
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
    Failed { at: usize, commit: String, runs: Vec<bp_check::Run>, number: u64 },
}

/// The active claim a repo verb acts under, and where its work lives.
struct Claimed {
    claim: Id,
    top: PathBuf,
    branch: String,
    worktree: PathBuf,
}

pub(crate) fn run_pr(cwd: &Path, p: &Pr, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match pr(cwd, p) {
        Ok(Opened::Wrote(w)) => crate::wrote(Ok(w), json, out),
        Ok(Opened::Failed { at, commit, runs, number }) => {
            let then = format!("commit it and bp pr {}", p.issue);
            crate::check::failed(&runs[at], &then, &commit, crate::check::rows(&runs), Some(number), json, out)
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
    let gh = Gh::new(&c.top);
    let head = git(&c.worktree, &["rev-parse", "HEAD"])?;
    let open = gh.prs_for_head(&c.branch)?.into_iter().find(PullRequest::is_open);
    let blocks = crate::intent::blocks(&vault, id.as_str())?;
    let runs = crate::intent::runs(&index, id.as_str())?;
    if let Some(o) = open.as_ref().filter(|o| o.head_ref_oid == head)
        && blocks.iter().all(|b| runs.iter().any(|r| r.commit == head && &r.check == b))
    {
        return Ok(Opened::Wrote(Wrote::Already(json!({ "id": id, "pull_request": o.number, "head": head }))));
    }
    let base = git(&c.top, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).map_err(|e| {
        Refusal::failed(e.subject, "origin has no default branch here: git remote set-head origin --auto")
    })?;
    let base = base.strip_prefix("origin/").unwrap_or(&base).to_string();
    let title = index.header(id.as_str())?.and_then(|h| h.get("title")?.as_str().map(str::to_string));
    let title = title.unwrap_or_else(|| id.to_string());
    // The blocks may take minutes, so they run with no lock held; a dirty worktree refuses here, before the push.
    drop(index);
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(now));
    let ran = bp_check::run(&vault, &id, &c.worktree, None, &[], &clock)?;
    // The head the runs name is the one pushed, even if a commit landed in the worktree while they ran.
    let head = ran.as_ref().map_or(head, |r| r.commit().to_string());
    git(&c.top, &["push", "-q", "origin", &format!("{head}:refs/heads/{}", c.branch)])?;
    let number = match &open {
        Some(o) if o.base_ref_name.as_deref() == Some(base.as_str()) => o.number,
        Some(o) => {
            gh.retarget(o.number, &base)?;
            o.number
        }
        None => gh.create(&c.branch, &base, &title, id.as_str())?,
    };
    let mut result = json!({
        "id": id,
        "pull_request": number,
        "head": head,
        "branch": c.branch,
        "base": base,
        "runs": [],
        "commit": null,
        "findings": [],
    });
    // With no named block there is nothing to record, so no commit.
    let Some(ran) = ran else { return Ok(Opened::Wrote(Wrote::Changed(result))) };
    let writer = ran.writer(&vault, &format!("bp pr {id}"))?;
    let (changes, runs) = ran.records(&vault);
    let written = writer.commit("pr", role.as_str(), &format!("pr {id}"), &changes)?;
    if let Some(at) = runs.iter().position(bp_check::Run::failed) {
        return Ok(Opened::Failed { at, commit: written.commit, runs, number });
    }
    result["runs"] = crate::check::rows(&runs).into();
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(&written.findings).expect("findings serialize");
    Ok(Opened::Wrote(Wrote::Changed(result)))
}

pub(crate) fn land(cwd: &Path, l: &Land) -> Result<Wrote, Refusal> {
    node_id(&l.issue)?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let now = crate::clock::now()?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &l.issue)?;
    let resolved = IssueStatus::Resolved.as_str();
    let status = index.header(id.as_str())?.and_then(|h| h.get("status")?.as_str().map(str::to_string));
    if status.as_deref() == Some(resolved) {
        return Ok(Wrote::Already(json!({ "id": id, "status": resolved })));
    }
    let c = claimed(cwd, &index, &id)?;
    let project = id.project().unwrap_or_default();
    let reviewer = crate::intent::reviewer(&index, id.as_str())?;
    let gh = Gh::new(&c.top);
    let login = gh.login()?;
    if reviewer.is_empty() || reviewer == login {
        let why = if reviewer.is_empty() {
            "the project names no reviewer".into()
        } else {
            format!("{login} is the token's own login")
        };
        return Err(Refusal::new(
            Code::ReviewerInvalid,
            format!("projects/{project}/project"),
            "land counts only the reviewer identity's approval, a login other than the token's (26, design change 3)",
            format!(
                "{why}; the human sets the second identity's forge login: bp set projects/{project}/project reviewer <login>"
            ),
        ));
    }
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
    // A merged pull request is a land that stopped before its commit: the forge already passed it, so the rest
    // finishes without asking again.
    if !merged(&pr) {
        vault.clean([&id, &c.claim])?;
        if c.worktree.is_dir() && !git(&c.worktree, &["status", "--porcelain"])?.is_empty() {
            return Err(Refusal::new(
                Code::PathDirty,
                c.worktree.display().to_string(),
                "land removes the worktree, so it holds no uncommitted work (26, design change 2)",
                format!("commit and bp pr {id}, or stash, the changes in {}, then rerun", c.worktree.display()),
            ));
        }
        let runs = crate::intent::runs(&index, id.as_str())?;
        let blocks = crate::intent::blocks(&vault, id.as_str())?;
        let read = crate::intent::pull_request(&pr, &reviewer);
        if let Err(unmet) = intents::landing(&read, &runs, &blocks) {
            return Err(refusal(&unmet, &id, &pr, &reviewer));
        }
        gh.merge(pr.number, &pr.head_ref_oid)?;
    }
    drop(index);
    remove(&c)?;
    let mut writer = vault.writer()?;
    writer.clean([&id, &c.claim])?;
    // Another write may have landed while the forge merged: the claim must still be the one this land read.
    if held(writer.index(), &id)?.is_none_or(|h| h.id != c.claim.as_str()) {
        return Err(Refusal::failed(
            c.claim.as_str(),
            format!("pull request {} merged, but the claim closed meanwhile; bp show {id}", pr.number),
        ));
    }
    let to = crate::clock::instant(now);
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
        "commit": written.commit,
        "findings": written.findings,
    })))
}

/// The issue's active claim, its clone and its branch, or `illegal_transition`: `pr` and `land` act on claimed work.
fn claimed(cwd: &Path, index: &Index, id: &Id) -> Result<Claimed, Refusal> {
    let Some(h) = held(index, id)? else {
        return Err(Refusal::new(
            Code::IllegalTransition,
            id.as_str(),
            "pr and land act on an issue under an active claim; cut makes it (21, claim)",
            format!("bp cut {id}, then rerun"),
        ));
    };
    let project = id.project().unwrap_or_default();
    let slug = id.as_str().rsplit('/').next().unwrap_or_default();
    Ok(Claimed {
        claim: Id::parse(&h.id).expect("the index holds ids"),
        top: clone_of(cwd, index, project)?,
        branch: format!("{project}/{slug}"),
        worktree: field(&h, "worktree").map(PathBuf::from).unwrap_or_default(),
    })
}

/// The first unmet part of needs landing as its refusal (26, design change 4).
fn refusal(unmet: &Unmet, id: &Id, pr: &PullRequest, reviewer: &str) -> Refusal {
    let (n, head) = (pr.number, &pr.head_ref_oid);
    match unmet {
        Unmet::Review => Refusal::new(
            Code::ReviewMissing,
            id.as_str(),
            "land counts only the reviewer identity's approving review at the head (15, Controls 1)",
            format!("the human approves pull request {n} at {head} as {reviewer}, then bp land {id}"),
        ),
        Unmet::Checks => Refusal::new(
            Code::ChecksPending,
            id.as_str(),
            "the forge check at the head is green (15, Controls 3)",
            format!("wait for the checks of pull request {n} at {head} to pass, then bp land {id}"),
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
            "land needs a passing run of each error block at the head (26, design change 4)",
            format!("{block} has no run at {head}; bp pr {id}, then bp land {id}"),
        ),
    }
}

/// Removes what the claim added to the clone: the worktree, the branch and the forge's copy of it. Each step skips
/// what is already gone, so a land that stopped part way finishes on its rerun.
fn remove(c: &Claimed) -> Result<(), Refusal> {
    if c.worktree.is_dir() {
        git(&c.top, &["worktree", "remove", &c.worktree.display().to_string()])?;
    }
    // A squash merge leaves the branch unmerged by ancestry, so `-d` would refuse it.
    if git(&c.top, &["rev-parse", "--verify", "-q", &format!("refs/heads/{}", c.branch)]).is_ok() {
        git(&c.top, &["branch", "-q", "-D", &c.branch])?;
    }
    git(&c.top, &["fetch", "-q", "--prune", "origin"])?;
    if git(&c.top, &["rev-parse", "--verify", "-q", &format!("refs/remotes/origin/{}", c.branch)]).is_ok() {
        git(&c.top, &["push", "-q", "origin", "--delete", &c.branch])?;
    }
    Ok(())
}

fn merged(p: &PullRequest) -> bool {
    p.state.as_deref() == Some("MERGED")
}
