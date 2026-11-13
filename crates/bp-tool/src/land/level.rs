//! What `land` reads by the risk level (37, design changes 1 to 4): who lands, the base whose rules judge the level,
//! which signed acts count, and whether the level rose after `pr`.

use bp_change::intents::{self, Issue};
use bp_change::risk::Level;
use bp_forge::{Gh, PullRequest};
use bp_operation::Role;
use bp_vault::{Code, Id, Index, Refusal};

use super::{Claimed, holds_base, merged};
use crate::claim::git;
use crate::signed::{is_branch, is_sha};

/// The role that lands (37, design change 3): the session, which runs `land` at needs-landing, or the human. A worker,
/// a reviewer or a researcher refuses `role_forbidden`. The label is forgeable, so it stops a slip only: the gates, and
/// for high the human's signature, hold whoever runs the verb.
pub(super) fn lander(issue: &str) -> Result<Role, Refusal> {
    let role = crate::set::role()?;
    if matches!(role, Role::Human | Role::Session) {
        return Ok(role);
    }
    Err(Refusal::new(
        Code::RoleForbidden,
        format!("land {issue}"),
        "BP_ROLE names a role that does not land: the session lands at needs-landing, or the human does (37, design \
         change 3)",
        format!("the worker stops at bp pr; the session runs bp land {issue} when its intent is needs-landing"),
    ))
}

/// `Ok` with `v` when the forge gave a full commit sha for `what`, else `could_not_run`, before a git call reads it
/// (review of #100, F4 and V9).
fn sha(c: &Claimed, pr: &PullRequest, what: &str, v: Option<&str>) -> Result<String, Refusal> {
    match v.filter(|s| is_sha(s)) {
        Some(s) => Ok(s.to_string()),
        None => Err(Refusal::failed(
            format!("gh pr list --head {}", c.branch),
            format!(
                "the forge gave {what} {} for pull request {}, not a full commit sha; fix gh, then rerun",
                v.unwrap_or("nothing"),
                pr.number
            ),
        )),
    }
}

/// `Ok` when the forge names a full sha for the head, and a branch for the base of an open pull request: `land` reads
/// them first, since every later git call and gate takes them (review of #100, F4 and F5).
pub(super) fn shaped(c: &Claimed, pr: &PullRequest) -> Result<(), Refusal> {
    sha(c, pr, "head", Some(&pr.head_ref_oid))?;
    if !merged(pr) && !pr.base_ref_name.as_deref().is_some_and(is_branch) {
        return Err(Refusal::failed(
            format!("gh pr list --head {}", c.branch),
            format!("the forge names no base branch of pull request {}; fix gh, then rerun", pr.number),
        ));
    }
    Ok(())
}

/// The commit whose rules judge the head's level, from the forge's own shas (37, design change 1): for an open pull
/// request its `baseRefOid`, once `git ls-remote` names the same sha for its base branch; for a merged one its
/// `baseRefOid` alone, since the branch has moved on. The pull request's own diff is then judged, whatever the merge
/// made: a rebase merge's commit holds the head's earlier commits in its first parent (review of #100, F8 and V4). A
/// `url.<x>.insteadOf` rule that points origin at another repository so refuses at every level, before a level is
/// judged (C2). Each sha the forge gives is a full sha before a git call reads it, and an open pull request names its
/// base branch (F4, F5). `land` runs it after the gates that come before the base (V6), and [`shaped`] first.
pub(super) fn base(c: &Claimed, id: &Id, pr: &PullRequest) -> Result<String, Refusal> {
    let sha = |what: &str, v: Option<&str>| sha(c, pr, what, v);
    if merged(pr) {
        let base = sha("base", pr.base_ref_oid.as_deref())?;
        git(&c.top, &["fetch", "-q", "origin", &base])?;
        return git(&c.top, &["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")]);
    }
    // `shaped` ran: an open pull request names its base branch.
    let name = pr.base_ref_name.as_deref().unwrap_or_default();
    let forge = sha("base", pr.base_ref_oid.as_deref())?;
    let origin = crate::risk::at_origin(&c.top, name)?;
    if origin.as_deref() == Some(forge.as_str()) {
        return Ok(forge);
    }
    // A head behind origin's base refuses as such: its rebase and push give the forge the new base too.
    if name == c.target {
        holds_base(c, id, &pr.head_ref_oid, true)?;
    }
    Err(Refusal::new(
        Code::BaseUnconfirmed,
        format!("git ls-remote origin refs/heads/{name}"),
        "land judges the level by the rules on the base that both origin and the forge name (37, design change 1)",
        format!(
            "the forge holds pull request {} against {name} at {forge}, and origin answers {}: if a branch landed \
             meanwhile, rerun; else check git config --get-regexp '^url\\.' and remote.origin.url in {}, then rerun",
            pr.number,
            origin.as_deref().unwrap_or("no such branch"),
            c.top.display()
        ),
    ))
}

/// Puts into `issue` only the signed acts that verify (37, design changes 2 to 4), and returns the level in force with,
/// for high, the refusal of an approval that does not verify. A lowering counts only when its signature verifies for
/// the forge's pull request, its base and its head; an approval the same. The signer is read only when a lowering
/// record names the head or the level is high, so a low or medium land reads no key. At high's round cap a fault of
/// `ssh-keygen` or of the signer read refuses at once, with its own code.
pub(super) fn trusted(
    c: &Claimed,
    gh: &Gh,
    index: &Index,
    id: &Id,
    pr: &PullRequest,
    issue: &mut Issue,
) -> Result<(Level, Option<Refusal>), Refusal> {
    let head = pr.head_ref_oid.as_str();
    let forge = (pr.number, pr.base_ref_name.as_deref().unwrap_or_default());
    let mut key: Option<String> = None;
    let mut signer = || -> Result<String, Refusal> {
        if let Some(k) = &key {
            return Ok(k.clone());
        }
        let k = forge_signer(c, gh)?;
        key = Some(k.clone());
        Ok(k)
    };
    issue.lowerings = if issue.lowerings.iter().any(|(h, _)| h == head) {
        let lower = crate::approve::lowered(index, id, head, &signer()?, Some(forge))?;
        lower.map(|l| (head.to_string(), l)).into_iter().collect()
    } else {
        Vec::new()
    };
    let level = intents::level(issue, head).0;
    if level < Level::High {
        issue.approvals.clear();
        return Ok((level, None));
    }
    // A signer that cannot be read refuses as an approval that does not verify, at the approval's place in the gate
    // order, so a high head with an open review still refuses on its review first.
    let approved = signer().and_then(|s| crate::approve::approved(index, id, head, &s, forge));
    let (approvals, refused) = match approved {
        // A signed list names rows only at high's round cap, so a list outside it is no approval (review of #100, W1).
        Ok(accepts) if !accepts.is_empty() && !intents::at_cap(issue, head) => (
            Vec::new(),
            Some(Refusal::new(
                Code::SignatureInvalid,
                id.as_str(),
                "an approval names the rows it accepts only at high's round cap (37, design change 6)",
                format!(
                    "git rm the record under {id}/approvals that names rows at {head} and commit it; then the human \
                     runs bp approve {id} from a shell"
                ),
            )),
        ),
        Ok(accepts) => (vec![intents::Approval { head: head.to_string(), accepts }], None),
        // At the cap the rows the human accepted read open without the approval, so a tool or signer fault there
        // refuses as itself, before the gates: never `findings_open` and a worker whose fix voids the approval
        // (review of #100, W3).
        Err(e) if intents::at_cap(issue, head) && !matches!(e.code, Code::SignatureInvalid | Code::ApprovalMissing) => {
            return Err(e);
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    issue.approvals = approvals;
    Ok((level, refused))
}

/// The signer of `.bp/risk.yml` at the sha `git ls-remote` names for origin's default branch, once that sha is the
/// head of the default branch that the forge names (review of #95, row 24). `gh` finds its repository from origin's
/// url as git rewrites it, so a rewrite onto another repository of the same host passes this compare; `signer`'s
/// test of the rewritten url refuses it.
fn forge_signer(c: &Claimed, gh: &Gh) -> Result<String, Refusal> {
    let (key, sha) = crate::signed::signer(&c.top)?;
    let (branch, oid) = gh.default_head()?;
    if oid != sha {
        return Err(Refusal::new(
            Code::SignerMissing,
            "git ls-remote --symref origin HEAD",
            "bp reads the signer at the head of the default branch that the forge names (37, design change 4)",
            format!(
                "the forge's {branch} is at {oid}, and origin answers {sha}: check git config --get-regexp \
                 '^url\\.' and remote.origin.url in {}, git fetch, then rerun",
                c.top.display()
            ),
        ));
    }
    Ok(key)
}

/// `evidence_stale` when the level of the head against `base` ([`base`]) is above the level `pr` recorded at the head,
/// or touches a high path that `pr` did not record (37, design changes 1 and 8): a review read the head at the
/// recorded level, so a rise needs `bp pr` to record it and the new level's lenses. The diff is judged again, never
/// read from a record.
pub(super) fn risen(c: &Claimed, id: &Id, issue: &Issue, head: &str, base: String) -> Result<(), Refusal> {
    let fresh = crate::risk::judge_at(&c.top, base, &c.target, vec![head.to_string()])?.computed;
    let (was, now) = (intents::level(issue, head).0, intents::in_force(issue, head, Some(fresh.level)).0);
    let security = fresh.security && !intents::security(issue, head);
    if now <= was && !security {
        return Ok(());
    }
    let what = if now > was { format!("rose from {was} to {now}") } else { "touches a high path".into() };
    Err(Refusal::new(
        Code::EvidenceStale,
        id.as_str(),
        "land judges the level of the head after its own fetch and refuses a level above the one pr recorded (37, \
         design change 1)",
        format!(
            "the level at {head} {what} since pr recorded it ({}); dispatch a worker to run `bp pr {id}`, which \
             records it; then the review of the new level's lenses, then bp land {id}",
            fresh.reasons.join("; ")
        ),
    ))
}
