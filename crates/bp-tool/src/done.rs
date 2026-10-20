//! `bp walk <id> --for <action> --done`: the action's stop, a predicate on records (21, rule 4; 27, design change
//! 11). A skill's `Stop:` line calls it, so it reads no slice; it reads only what its row's Stop column names.

use std::collections::BTreeSet;
use std::path::Path;

use bp_change::{MilestoneStatus, intents};
use bp_forge::{Gh, PullRequest};
use bp_operation::Action;
use bp_reasoning::ThreadStatus;
use bp_vault::git::Repo;
use bp_vault::{Filter, Id, Index, Kind, Refusal, Vault};
use serde_json::{Map, Value};

use crate::claim::{branch, field, held};

/// The batch size for derive, until it is a project setting (21, open questions: the lean is three).
const BATCH: usize = 3;

/// How deep `search` must find a learning by its cue: the search verb's default limit.
const RETRIEVAL: usize = 10;

/// Whether the action's Stop column holds for the seed `id`, whose header the walk read.
pub(crate) fn done(
    vault: &Vault,
    index: &Index,
    id: &Id,
    header: &Map<String, Value>,
    action: Action,
) -> Result<bool, Refusal> {
    let status = header.get("status").and_then(Value::as_str);
    Ok(match action {
        // The seed is in the index, so it parsed. A capture and a returned report are their one call.
        Action::Design | Action::Capture | Action::Research => true,
        Action::Shape => {
            let planned = Some(MilestoneStatus::Planned.as_str());
            // A shared design has no project, so no milestone shapes from it.
            let Some(project) = id.project() else { return Ok(false) };
            let f = Filter { kind: Kind::Milestone, status: planned, project: Some(project), under: None, limit: 1 };
            index.list(&f)?.1 > 0
        }
        Action::Derive => derived(vault, index, id)?,
        Action::Complete => {
            // The records sort by time, so the last claim is the latest.
            let claims = index.records(id.as_str(), Kind::Claim)?;
            let escalated = claims.last().is_some_and(|c| {
                let r = intents::release(&c.body);
                r.get("to").is_some_and(|t| !t.is_empty()) && r.get("reason").map(String::as_str) == Some("escalated")
            });
            escalated || heads(index, id)?.is_some_and(|(pr, local)| pr == local)
        }
        Action::Review => match heads(index, id)? {
            Some((pr, _)) => index
                .records(id.as_str(), Kind::Review)?
                .iter()
                .any(|r| field(r, "head").as_deref() == Some(pr.as_str())),
            None => false,
        },
        Action::Respond => responded(index, id)?,
        Action::TriageDrift => {
            let failed = index
                .records(id.as_str(), Kind::Run)?
                .into_iter()
                .filter(|r| matches!(field(r, "verdict").as_deref(), Some("fail" | "timed-out")));
            for r in failed {
                let run = Id::parse(&r.id).expect("the index holds ids");
                if citing(index, &run, &[Kind::Delta, Kind::Thread])?.is_empty() {
                    return Ok(false);
                }
            }
            true
        }
        Action::Deliberate => {
            status == Some(ThreadStatus::Closed.as_str())
                && !citing(index, id, &[Kind::Decision, Kind::Learning])?.is_empty()
        }
        Action::Distil => {
            let learnings = match header.get("kind").and_then(Value::as_str) {
                Some("learning") => vec![id.clone()],
                _ => citing(index, id, &[Kind::Learning])?,
            };
            for l in learnings {
                let h = index.header(l.as_str())?.unwrap_or_default();
                // A converted learning has no cue yet; its title is the line a search would try first.
                let cue = h.get("cue").or_else(|| h.get("title")).and_then(Value::as_str).unwrap_or_default();
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

/// The batch is full or the finish line is covered: each named block of the milestone has a namesake in an issue
/// under it.
fn derived(vault: &Vault, index: &Index, id: &Id) -> Result<bool, Refusal> {
    let f = Filter { kind: Kind::Issue, status: None, project: None, under: Some(id.as_str()), limit: usize::MAX >> 1 };
    let (issues, _) = index.list(&f)?;
    if issues.iter().filter(|i| intents::is_live_issue(i.status.as_deref().unwrap_or_default())).count() >= BATCH {
        return Ok(true);
    }
    let mut covered = BTreeSet::new();
    for i in &issues {
        let file = vault.read(&Id::parse(&i.id).expect("the index holds ids"))?;
        covered.extend(bp_check::named(&file.body).into_iter().map(|c| c.attrs["name"].clone()));
    }
    Ok(bp_check::named(&vault.read(id)?.body).iter().all(|c| covered.contains(&c.attrs["name"])))
}

/// Every finding of the latest review has a disposition, and the head moved if one is `fixed`.
fn responded(index: &Index, id: &Id) -> Result<bool, Refusal> {
    let (Some((pr, _)), Some(review)) = (heads(index, id)?, index.records(id.as_str(), Kind::Review)?.pop()) else {
        return Ok(false);
    };
    // The findings are the rows of the body's table, after its header and separator lines (13, Review).
    let findings = review.body.lines().filter(|l| l.trim_start().starts_with('|')).count().saturating_sub(2);
    let dispositions = index.records(&review.id, Kind::Disposition)?;
    let answered: BTreeSet<u64> = dispositions.iter().filter_map(|d| d.header.get("finding")?.as_u64()).collect();
    let fixed = dispositions.iter().any(|d| field(d, "as").as_deref() == Some("fixed"));
    Ok(answered.len() >= findings && !(fixed && field(&review, "head") == Some(pr)))
}

/// The open pull request's head and the held claim's worktree head. The forge comes from the snapshot when one is
/// set, as the intents read it, else from `gh` in the worktree; with no worktree there is no claim head to compare.
fn heads(index: &Index, id: &Id) -> Result<Option<(String, String)>, Refusal> {
    let Some(claim) = held(index, id)? else { return Ok(None) };
    let Some(worktree) = field(&claim, "worktree") else { return Ok(None) };
    let Some(local) = Repo::find(Path::new(&worktree)).and_then(|r| r.head()) else { return Ok(None) };
    let open = match crate::intent::World::read()?.forge {
        Some(s) => s.open_for(id.as_str().rsplit('/').next().unwrap_or_default()).cloned(),
        None => Gh::new(&worktree).prs_for_head(&branch(id))?.into_iter().find(PullRequest::is_open),
    };
    Ok(open.map(|p| (p.head_ref_oid, local)))
}

/// The files of `kinds` that name `id`.
fn citing(index: &Index, id: &Id, kinds: &[Kind]) -> Result<Vec<Id>, Refusal> {
    let mut out = Vec::new();
    for i in index.inbound(id)? {
        let kind = index.header(i.as_str())?.and_then(|h| h.get("kind")?.as_str().map(str::to_string));
        if kind.is_some_and(|k| kinds.iter().any(|w| w.as_str() == k)) {
            out.push(i);
        }
    }
    Ok(out)
}
