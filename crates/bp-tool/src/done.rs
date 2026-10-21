//! `bp walk <id> --for <action> --done`: the action's stop, a predicate on records (21, rule 4; 27, design change
//! 11). A skill's `Stop:` line calls it, so it reads no slice; it reads only what its row's Stop column names.

use std::path::{Path, PathBuf};

use bp_change::{MilestoneStatus, intents};
use bp_operation::Action;
use bp_reasoning::ThreadStatus;
use bp_vault::git::Repo;
use bp_vault::{Filter, Id, Index, Kind, Refusal, Vault};
use serde_json::{Map, Value};

use crate::claim::{field, held};

/// How deep `search` must find a learning by its cue: the search verb's default limit.
const RETRIEVAL: usize = 10;

/// Whether the action's Stop column holds for the seed `id`, whose header the walk read. A stop that reads the forge
/// sets `forge_age` to its snapshot's age; one that reads none leaves it `None`.
pub(crate) fn done(
    vault: &Vault,
    index: &Index,
    id: &Id,
    header: &Map<String, Value>,
    action: Action,
    forge_age: &mut Option<i64>,
) -> Result<bool, Refusal> {
    let text = |k: &str| header.get(k).and_then(Value::as_str);
    Ok(match action {
        // Call-shaped stops with no record predicate (21, rule 4): research ends when the dispatch returns, capture
        // with its one call, and design when the session writes a node whose id the walk cannot know before.
        Action::Design | Action::Capture | Action::Research => true,
        Action::Shape => {
            let planned = Some(MilestoneStatus::Planned.as_str());
            // A shared design has no project, so no milestone shapes from it.
            let Some(project) = id.project() else { return Ok(false) };
            let f = Filter { kind: Kind::Milestone, status: planned, project: Some(project), under: None, limit: 1 };
            index.list(&f)?.1 > 0
        }
        Action::Derive => intents::uncovered(&crate::intent::derivation(index, id.as_str())?).is_empty(),
        // A worker that cannot finish releases the claim `escalated`, and that ends its dispatch (21).
        Action::Complete | Action::Respond if escalated(index, id)? => true,
        Action::Complete => {
            // An open pull request is not done until each error block passes at its head: `pr` opens one even
            // when it refuses `check_failed` (27, issue 8).
            match open_head(&vault.root, index, id, forge_age)? {
                Some((pr, Some(worktree))) => {
                    Repo::find(&worktree).and_then(|r| r.head()).as_ref() == Some(&pr)
                        && intents::evidence(
                            &crate::intent::runs(index, id.as_str())?,
                            &pr,
                            &crate::intent::blocks(vault, id.as_str())?,
                        )
                        .is_ok()
                }
                _ => false,
            }
        }
        Action::Review => match open_head(&vault.root, index, id, forge_age)? {
            Some((pr, _)) => index
                .records(id.as_str(), Kind::Review)?
                .iter()
                .any(|r| field(r, "head").as_deref() == Some(pr.as_str())),
            None => false,
        },
        Action::Respond => responded(&vault.root, index, id, forge_age)?,
        Action::TriageDrift => {
            // The drifted predicate over each design the seed covers: a project's drift runs sit under its designs.
            let kind = text("kind").and_then(|k| k.parse().ok()).expect("the index stores closed kinds");
            for d in crate::intent::drift_designs(index, id, kind)? {
                if !crate::intent::drift(index, &d)?.is_empty() {
                    return Ok(false);
                }
            }
            true
        }
        Action::Deliberate => {
            text("status") == Some(ThreadStatus::Closed.as_str())
                && !index.citing(id, &[Kind::Decision, Kind::Learning])?.is_empty()
        }
        Action::Distil => {
            let learnings = match text("kind") {
                Some("learning") => vec![id.clone()],
                _ => index.citing(id, &[Kind::Learning])?,
            };
            for l in learnings {
                let h = index.header(l.as_str())?.unwrap_or_default();
                // A learning with no cue has nothing to retrieve it by: the stop stays false until the agent sets one.
                let cue = h.get("cue").and_then(Value::as_str).unwrap_or_default();
                if !cue.trim().is_empty()
                    && index.search(&[cue.to_string()], None, None, RETRIEVAL)?.iter().any(|hit| hit.id == l.as_str())
                {
                    return Ok(true);
                }
            }
            false
        }
    })
}

/// The latest claim of the issue is released `escalated`.
fn escalated(index: &Index, id: &Id) -> Result<bool, Refusal> {
    let claims = crate::intent::claims(&index.records(id.as_str(), Kind::Claim)?);
    Ok(intents::latest_claim(&claims).is_some_and(|c| c.escalated))
}

/// Every finding of the latest review has a disposition, matched by its index, and the head moved if one is `fixed`.
fn responded(root: &Path, index: &Index, id: &Id, forge_age: &mut Option<i64>) -> Result<bool, Refusal> {
    let (Some((pr, _)), Some(review)) =
        (open_head(root, index, id, forge_age)?, index.records(id.as_str(), Kind::Review)?.pop())
    else {
        return Ok(false);
    };
    let (open, fixed) = crate::intent::findings(index, &review)?;
    Ok(open.is_empty() && !(fixed && field(&review, "head") == Some(pr)))
}

/// The head of the issue's open pull request while a claim is held, and the claim's worktree when this machine has
/// it. The forge is the snapshot the intents read, never a call (22, constraint 8): with none, no pull request is in
/// view, and the stop reads false until a repo verb writes one. Only the issue's own project is read.
fn open_head(
    root: &Path,
    index: &Index,
    id: &Id,
    forge_age: &mut Option<i64>,
) -> Result<Option<(String, Option<PathBuf>)>, Refusal> {
    Ok(open(root, index, id, forge_age)?.map(|(p, worktree)| (p.head_ref_oid, worktree)))
}

/// The issue's open pull request and the claim's worktree, as `open_head` reads them. The review slice carries its
/// number and head, so a reviewer reviews the head its stop compares (27, issue 9).
pub(crate) fn open(
    root: &Path,
    index: &Index,
    id: &Id,
    forge_age: &mut Option<i64>,
) -> Result<Option<(bp_forge::PullRequest, Option<PathBuf>)>, Refusal> {
    let Some(claim) = held(index, id)? else { return Ok(None) };
    let worktree = field(&claim, "worktree").map(PathBuf::from).filter(|w| w.is_dir());
    let world = crate::intent::World::read(root, &id.project().into_iter().collect())?;
    *forge_age = world.forge_age();
    Ok(world.pr(id.as_str()).map(|p| (p.clone(), worktree)))
}
