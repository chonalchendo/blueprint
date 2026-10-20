//! `bp walk <id> --for <action> --done`: the action's stop, a predicate on records (21, rule 4; 27, design change
//! 11). A skill's `Stop:` line calls it, so it reads no slice; it reads only what its row's Stop column names.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bp_change::{MilestoneStatus, intents};
use bp_operation::Action;
use bp_reasoning::ThreadStatus;
use bp_vault::git::Repo;
use bp_vault::{Body, Filter, Id, Index, Kind, Refusal};
use serde_json::{Map, Value};

use crate::claim::{field, held};

/// The batch size for derive, until it is a project setting (21, open questions: the lean is three).
const BATCH: usize = 3;

/// How deep `search` must find a learning by its cue: the search verb's default limit.
const RETRIEVAL: usize = 10;

/// Whether the action's Stop column holds for the seed `id`, whose header the walk read.
pub(crate) fn done(
    root: &Path,
    index: &Index,
    id: &Id,
    header: &Map<String, Value>,
    action: Action,
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
        Action::Derive => derived(index, id)?,
        Action::Complete => {
            let claims = crate::intent::claims(&index.records(id.as_str(), Kind::Claim)?);
            if intents::latest_claim(&claims).is_some_and(|c| c.escalated) {
                return Ok(true);
            }
            match open_head(root, index, id)? {
                Some((pr, Some(worktree))) => Repo::find(&worktree).and_then(|r| r.head()) == Some(pr),
                _ => false,
            }
        }
        Action::Review => match open_head(root, index, id)? {
            Some((pr, _)) => index
                .records(id.as_str(), Kind::Review)?
                .iter()
                .any(|r| field(r, "head").as_deref() == Some(pr.as_str())),
            None => false,
        },
        Action::Respond => responded(root, index, id)?,
        Action::TriageDrift => {
            // An integrity run checks the vault, not a design: 21's drifted reads drift runs only.
            let failed = index.records(id.as_str(), Kind::Run)?.into_iter().filter(|r| {
                matches!(field(r, "verdict").as_deref(), Some("fail" | "timed-out"))
                    && field(r, "check_kind").as_deref() != Some("integrity")
            });
            for r in failed {
                let run = Id::parse(&r.id).expect("the index holds ids");
                if index.citing(&run, &[Kind::Delta, Kind::Thread])?.is_empty() {
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

/// The batch is full or the finish line is covered: each named block in the milestone's `## Finish line` has a
/// namesake in the `## Check` of an issue under it (21, needs derivation).
fn derived(index: &Index, id: &Id) -> Result<bool, Refusal> {
    let f = Filter { kind: Kind::Issue, status: None, project: None, under: Some(id.as_str()), limit: crate::ALL };
    let (issues, _) = index.list(&f)?;
    if issues.iter().filter(|i| intents::is_live_issue(i.status.as_deref().unwrap_or_default())).count() >= BATCH {
        return Ok(true);
    }
    let mut covered = BTreeSet::new();
    for i in &issues {
        covered.extend(names(&index.body(&i.id)?, "Check"));
    }
    Ok(names(&index.body(id.as_str())?, "Finish line").is_subset(&covered))
}

/// The names of the named blocks in the body's sections under `heading`.
fn names(body: &str, heading: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for s in Body::parse(body).sections_named(heading) {
        out.extend(bp_check::named(&Body::parse(&s.text)).into_iter().map(|c| c.attrs["name"].clone()));
    }
    out
}

/// Every finding of the latest review has a disposition, matched by its index, and the head moved if one is `fixed`.
fn responded(root: &Path, index: &Index, id: &Id) -> Result<bool, Refusal> {
    let (Some((pr, _)), Some(review)) = (open_head(root, index, id)?, index.records(id.as_str(), Kind::Review)?.pop())
    else {
        return Ok(false);
    };
    // The findings are the rows of the body's table, after its header and separator lines, counted from 1 (21,
    // respond). A disposition for an index past the last row answers nothing.
    let rows = review.body.lines().filter(|l| l.trim_start().starts_with('|')).count().saturating_sub(2);
    let dispositions = index.records(&review.id, Kind::Disposition)?;
    let answered: BTreeSet<u64> = dispositions.iter().filter_map(|d| d.header.get("finding")?.as_u64()).collect();
    let fixed = dispositions.iter().any(|d| field(d, "as").as_deref() == Some("fixed"));
    Ok((1..=rows as u64).all(|n| answered.contains(&n)) && !(fixed && field(&review, "head") == Some(pr)))
}

/// The head of the issue's open pull request while a claim is held, and the claim's worktree when this machine has
/// it. The forge is the snapshot the intents read, never a call (22, constraint 8): with none, no pull request is in
/// view, and the stop reads false until a repo verb writes one.
fn open_head(root: &Path, index: &Index, id: &Id) -> Result<Option<(String, Option<PathBuf>)>, Refusal> {
    let Some(claim) = held(index, id)? else { return Ok(None) };
    let worktree = field(&claim, "worktree").map(PathBuf::from).filter(|w| w.is_dir());
    let world = crate::intent::World::read(root)?;
    Ok(world.pr(id.as_str()).map(|p| (p.head_ref_oid.clone(), worktree)))
}
