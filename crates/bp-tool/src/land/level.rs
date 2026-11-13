//! What `land` reads by the risk level (37, design changes 1 to 4): who lands, which signed acts count, and whether
//! the level rose after `pr`.

use bp_change::intents::{self, Issue};
use bp_change::risk::Level;
use bp_forge::{Gh, PullRequest};
use bp_operation::Role;
use bp_vault::{Code, Id, Index, Refusal};

use super::Claimed;

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

/// Puts into `issue` only the signed acts that verify (37, design changes 2 to 4), and returns the level in force with,
/// for high, the refusal of an approval that does not verify. A lowering counts only when its signature verifies for
/// the forge's pull request, its base and its head; an approval the same. The signer is read only when a lowering
/// record names the head or the level is high, so a low or medium land reads no key.
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
    issue.approvals = if approved.is_ok() { vec![head.to_string()] } else { Vec::new() };
    Ok((level, approved.err()))
}

/// The signer of `.bp/risk.yml` at the sha `git ls-remote` names for origin's default branch, once that sha is the
/// head of the default branch that the forge itself names (review of #95, row 24): a `url.<x>.insteadOf` rule or a
/// changed `remote.origin.url` redirects `ls-remote`, never `gh`.
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

/// `evidence_stale` when the level after `land`'s own fetch is above the level `pr` recorded at the head, or touches a
/// high path that `pr` did not record (37, design changes 1 and 8): a review read the head at the recorded level, so a
/// rise needs `bp pr` to record it and the new level's lenses. The diff is judged again, never read from a record.
pub(super) fn risen(c: &Claimed, id: &Id, issue: &Issue, head: &str) -> Result<(), Refusal> {
    let fresh = crate::risk::judge(&c.top, &c.target, vec![head.to_string()])?.computed;
    let (was, now) = (intents::level(issue, head).0, intents::in_force(issue, head, fresh.level).0);
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
            "the level at {head} {what} since bp pr recorded it ({}); bp pr {id} records it, then the review of the \
             new level's lenses, then bp land {id}",
            fresh.reasons.join("; ")
        ),
    ))
}
