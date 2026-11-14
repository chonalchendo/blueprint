//! The steps of `land` that change the forge or the clone: the merge, the claim test after it, the worktree test before
//! it and the removal of what the claim added.

use bp_forge::{Gh, PullRequest};
use bp_read::inputs::held;
use bp_vault::{Code, Id, Index, Refusal, Vault};

use super::{Claimed, merged};
use crate::claim::git;

/// Merges `pr` at the head the gates read, once the claim is still the one `land` read (review of #100, F10), then
/// reads the pull request again: `gh pr merge` exits 0 when a merge queue or auto-merge takes the pull request and
/// merges nothing, so only the forge's `MERGED` at that head lets `land` remove the branch (F1). Else it refuses and
/// changes nothing.
pub(super) fn merge(vault: &Vault, gh: &Gh, c: &Claimed, id: &Id, pr: &PullRequest) -> Result<PullRequest, Refusal> {
    let (n, head) = (pr.number, pr.head_ref_oid.as_str());
    if held(&Index::open(vault)?, id)?.is_none_or(|h| h.id != c.claim.as_str()) {
        return Err(Refusal::new(
            Code::ClaimLost,
            c.claim.as_str(),
            "land merges only under the claim it read; nothing merged (15, land)",
            format!("bp show {id} names who closed it; to land it, dispatch a worker to run `bp cut {id}`, then rerun"),
        ));
    }
    gh.merge(n, head).map_err(|e| match gh.pr(n) {
        // The forge refuses a head that moved since the reads: no review has read that head yet.
        Ok(now) if now.head_ref_oid != head => Refusal::new(
            Code::ReviewMissing,
            id.as_str(),
            "land merges only the head its reviews read (37, design change 3)",
            format!(
                "the head moved to {}; dispatch a worker to run `bp pr {id}`; then its review, then bp land {id}",
                now.head_ref_oid
            ),
        ),
        _ => e,
    })?;
    let now = gh.pr(n)?;
    if merged(&now) && now.head_ref_oid == head {
        return Ok(now);
    }
    Err(Refusal::failed(
        format!("gh pr merge {n}"),
        format!(
            "gh exited 0, but the forge reads pull request {n} as {} at {}: a merge queue or auto-merge holds it, and \
             land removed nothing; once the forge merges it, rerun bp land {id}",
            now.state.as_deref().unwrap_or("OPEN"),
            now.head_ref_oid
        ),
    ))
}

/// The claim's worktree holds no uncommitted work, since `land` removes it (26, design change 2).
pub(super) fn worktree_clean(c: &Claimed, id: &Id) -> Result<(), Refusal> {
    if !c.worktree.is_dir() || git(&c.worktree, &["status", "--porcelain"])?.is_empty() {
        return Ok(());
    }
    Err(Refusal::new(
        Code::PathDirty,
        c.worktree.display().to_string(),
        "land removes the worktree, so it holds no uncommitted work (26, design change 2)",
        format!(
            "dispatch a worker to commit the changes in {} and run `bp pr {id}`, or to stash them; then rerun",
            c.worktree.display()
        ),
    ))
}

/// The claim `land` read is still the held one after the merge: a release during it, or since a land that stopped,
/// leaves the merge in place and nothing else is removed (review of #100, V3).
pub(super) fn kept(vault: &Vault, id: &Id, c: &Claimed, pr: &PullRequest) -> Result<(), Refusal> {
    if held(&Index::open(vault)?, id)?.is_some_and(|h| h.id == c.claim.as_str()) {
        return Ok(());
    }
    let at = pr.merge_commit.as_ref().map_or(pr.head_ref_oid.as_str(), |m| m.oid.as_str());
    Err(Refusal::failed(
        c.claim.as_str(),
        format!(
            "pull request {} merged at {at}, but the claim closed meanwhile, so land removed nothing; bp show {id} \
             names who closed it",
            pr.number
        ),
    ))
}

/// Removes what the claim added to the clone: the worktree, the branch and the forge's copy of it. Each step skips
/// what is already gone, so a land that stopped part way finishes on its rerun. A branch with a commit the merged
/// head lacks stays, so no work is lost, and so does one a `stranded` layer still targets; the result is `true` then.
pub(super) fn remove(c: &Claimed, merged: &str, stranded: bool) -> Result<bool, Refusal> {
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
