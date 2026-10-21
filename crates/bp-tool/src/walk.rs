//! `bp walk <id> --for <action>`: the ranked slice an action needs, under its budget, with the scope's intents (21).
//! `--done` answers the action's stop instead (27, design change 11).

use std::collections::BTreeSet;
use std::path::Path;

use bp_operation::Action;
use bp_vault::{Code, Gap, Id, Index, Kind, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::{intent, node_id, subject, unknown};

/// `bp walk <id> --for <action> [--budget <tokens>] [--depth <hops>] [--ids] [--done]`.
#[derive(Args)]
pub struct Walk {
    /// The seed: an issue for complete, review and respond; a milestone for derive; a thread for deliberate.
    pub id: String,
    /// The action: complete, review, respond, derive, design, shape, research, triage-drift, deliberate, distil,
    /// capture.
    #[arg(long = "for")]
    pub action: String,
    /// The token budget; the action's own by default (21).
    #[arg(long)]
    pub budget: Option<usize>,
    /// Hops from the seed, at most 3; 15 walks two.
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..=3))]
    pub depth: u8,
    /// The map only: every reached id in rank order, no bodies.
    #[arg(long)]
    pub ids: bool,
    /// The action's stop only: `{id, for, done}`, no slice (21, rule 4).
    #[arg(long, conflicts_with = "ids")]
    pub done: bool,
}

pub(crate) fn walk(cwd: &Path, walk: &Walk) -> Result<Value, Refusal> {
    let action: Action = walk.action.parse().map_err(|_| {
        let actions: Vec<&str> = Action::ALL.iter().map(|a| a.as_str()).collect();
        unknown("for", &walk.action, format!("use one of: {}", actions.join(", ")))
    })?;
    node_id(&walk.id)?;
    // The bundle gates the walk: an installed bundle that drifted refuses before the vault is read (17, check 7).
    if let Some(top) = bp_vault::git::Repo::find(cwd).and_then(|r| r.main_top()) {
        bp_install::check(&top)?;
    }
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let reading = index.reading()?;
    let id = subject(&index, &walk.id)?;
    let header = index.header(id.as_str())?.ok_or_else(|| {
        vault.unparsed(&id).unwrap_or_else(|| {
            Refusal::new(
                Code::NoNode,
                id.as_str(),
                "an id names a markdown file under the vault root",
                format!("bp search {}", id.as_str().rsplit('/').next().unwrap_or_default().replace(['-', '.'], " ")),
            )
        })
    })?;
    let kind: Kind = header["kind"].as_str().and_then(|k| k.parse().ok()).expect("the index stores closed kinds");
    if !action.seed().contains(&kind) {
        let kinds: Vec<&str> = action.seed().iter().map(|k| k.as_str()).collect();
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("{id} --for {action}"),
            "an action walks only from its seed kinds (21)",
            format!("{action} walks from: {}; bp list {} --json", kinds.join(", "), kinds[0]),
        ));
    }
    if walk.done {
        // A stop reads a few rows, and `list` opens its own read transaction.
        drop(reading);
        let mut forge_age = None;
        let done = crate::done::done(&vault, &index, &id, &header, action, &mut forge_age)?;
        return Ok(serde_json::json!({ "id": id, "for": action, "done": done, "forge_age": forge_age }));
    }
    context(&vault, &index, &id, action)?;
    let mut reached = index.reach(id.as_str(), usize::from(walk.depth))?;
    if let Some(project) = id.project() {
        for c in index.cited_conventions(&format!("projects/{project}/project"))? {
            if !reached.iter().any(|r| r.id == c.id) {
                reached.push(c);
            }
        }
    }
    let projects: BTreeSet<&str> =
        reached.iter().filter(|r| r.kind == Kind::Issue).filter_map(|r| intent::split(&r.id)).map(|(p, _)| p).collect();
    let read = intent::Intents::new(&vault, &index, (!projects.is_empty()).then_some(&projects))?;
    let mut intents = Vec::new();
    for r in reached.iter().filter(|r| matches!(r.kind, Kind::Issue | Kind::Milestone)) {
        let verdict = read.row(r.kind, &r.id, r.status.as_deref())?;
        if let Some(intent) = verdict.intent {
            intents.push(serde_json::json!({ "id": r.id, "intent": intent, "note": verdict.note }));
        }
    }
    // The latest record of each kind the action answers, latest by its time field, as `index.records` orders them.
    let mut records = Vec::new();
    for kind in action.records() {
        if let Some(r) = index.records(id.as_str(), *kind)?.pop() {
            records.push(serde_json::json!({ "id": r.id, "kind": kind, "header": r.header, "body": r.body }));
        }
    }
    let pull_request = pull_request(&vault, &index, &id, action)?;
    let line =
        |r: &bp_vault::Reached| serde_json::json!({ "id": r.id, "kind": r.kind, "title": r.title, "rank": r.rank });
    if walk.ids {
        // The map keeps the fill's order: conventions close the tail (21).
        let (conventions, rest): (Vec<_>, Vec<_>) = reached.iter().partition(|r| r.kind == Kind::Convention);
        let answered = records
            .iter()
            .map(|r| serde_json::json!({ "id": r["id"], "kind": r["kind"], "title": null, "rank": null }));
        let ids: Vec<Value> = answered.chain(rest.into_iter().chain(conventions).map(line)).collect();
        return Ok(with(
            serde_json::json!({ "seed": id, "for": action, "ids": ids, "intents": intents }),
            pull_request,
        ));
    }
    let budget = walk.budget.unwrap_or_else(|| action.budget());
    // The intents, the answered records and the pull request are always sent, so they spend the budget before the
    // fill does.
    let pr = pull_request.as_ref().map(serde_json::to_string);
    let named = [serde_json::to_string(&intents), serde_json::to_string(&records)]
        .into_iter()
        .chain(pr)
        .map(|s| s.expect("plain JSON serialises").len().div_ceil(4))
        .sum::<usize>();
    let f = bp_vault::fill(&reached, action.bodies(), budget.saturating_sub(named));
    let by_id = |want: &String| reached.iter().find(|r| &r.id == want).expect("the fill chose from the reached");
    let mut bodies = records;
    for b in &f.bodies {
        let mut v = line(by_id(b));
        v["body"] = index.body(b)?.into();
        bodies.push(v);
    }
    let ids: Vec<Value> = f.ids.iter().map(|i| line(by_id(i))).collect();
    let slice = serde_json::json!({
        "seed": id, "for": action, "budget": budget, "used": f.used + named,
        "bodies": bodies, "ids": ids, "cut": f.cut, "intents": intents,
    });
    Ok(with(slice, pull_request))
}

/// Review carries the pull request whose head its stop compares; null when no pull request is in view (27, issue
/// 9). Other actions carry no such field.
fn pull_request(vault: &Vault, index: &Index, id: &Id, action: Action) -> Result<Option<Value>, Refusal> {
    if action != Action::Review {
        return Ok(None);
    }
    let open = crate::done::open(&vault.root, index, id, &mut None)?;
    Ok(Some(open.map_or(Value::Null, |(p, _)| serde_json::json!({ "number": p.number, "head": p.head_ref_oid }))))
}

/// The walk's answer, with the pull request when the action carries one.
fn with(mut answer: Value, pull_request: Option<Value>) -> Value {
    if let Some(p) = pull_request {
        answer["pull_request"] = p;
    }
    answer
}

/// The context the action cannot run without (21, rule 3), by the predicate the integrity check shares: a target
/// that does not parse refuses as itself, so the fix names the parse error.
fn context(vault: &Vault, index: &Index, id: &Id, action: Action) -> Result<(), Refusal> {
    for req in action.requires() {
        let Some(gap) = index.context_gap(id.as_str(), *req)? else { continue };
        if let Gap::Absent(t) = &gap
            && let Some(unparsed) = Id::parse(t).ok().and_then(|t| vault.unparsed(&t))
        {
            return Err(unparsed);
        }
        return Err(Refusal::new(
            Code::ContextIncomplete,
            format!("{id} --for {action}"),
            "a walk refuses when a kind its action requires is absent or empty (21, rule 3)",
            format!("{}; bp show {id} --no-body", gap.detail(*req)),
        ));
    }
    Ok(())
}
