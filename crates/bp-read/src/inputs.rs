//! The vault's records, reduced to the inputs the pure intents read (21): the claims, runs, blocks, reviews and
//! waivers of one node, and the stack below a claim. `land`, `stamp` and `walk --done` read the same reductions, so a
//! verb and an intent cannot disagree (21, rule 1).

use std::collections::{BTreeMap, BTreeSet};

use bp_change::IssueStatus;
use bp_change::intents::{self, Base, Block, Claim, Milestone, PullRequest, STACK_DEPTH};
use bp_vault::{Body, Check, Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde_json::{Map, Value};

/// The issues under the milestone `id`, and while fewer than a batch are live, its finish line and the blocks the
/// issues' checks name: needs derivation's records, which derive's stop reads too (21).
pub fn derivation(index: &Index, id: &str) -> Result<Milestone, Refusal> {
    let f = Filter { kind: Kind::Issue, status: None, project: None, under: Some(id), limit: crate::ALL };
    let (rows, _) = index.list(&f)?;
    let mut m = Milestone {
        issues: rows.iter().map(|r| r.status.clone().unwrap_or_default()).collect(),
        ..Milestone::default()
    };
    if m.issues.iter().filter(|s| intents::is_live_issue(s)).count() >= intents::BATCH {
        return Ok(m);
    }
    m.finish = named(&index.body(id)?, "Finish line");
    // With no block to cover, the issues' checks say nothing.
    if !m.finish.is_empty() {
        for r in &rows {
            m.covered.extend(named(&index.body(&r.id)?, "Check").into_iter().map(|b| b.name));
        }
    }
    Ok(m)
}

/// The named blocks in the body's sections under `heading`, with their hashes now.
fn named(body: &str, heading: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for s in Body::parse(body).sections_named(heading) {
        let body = Body::parse(&s.text);
        out.extend(body.named_checks().into_iter().map(|c| Block { name: c.attrs["name"].clone(), hash: c.hash() }));
    }
    out
}

/// The findings of a review that no disposition answers, counted from 1, and whether a disposition is `fixed` (21,
/// respond). The findings are the rows of the body's table after its header and separator lines; a disposition of
/// an index past the last row answers nothing.
pub fn findings(index: &Index, review: &Record) -> Result<(Vec<u64>, bool), Refusal> {
    let rows = review.body.lines().filter(|l| l.trim_start().starts_with('|')).count().saturating_sub(2);
    let dispositions = index.records(&review.id, Kind::Disposition)?;
    let answered: BTreeSet<u64> = dispositions.iter().filter_map(|d| d.header.get("finding")?.as_u64()).collect();
    let fixed = dispositions.iter().any(|d| text(&d.header, "as").as_deref() == Some("fixed"));
    Ok(((1..=rows as u64).filter(|n| !answered.contains(n)).collect(), fixed))
}

/// The drift runs under a design that no delta or thread cites: the runs of 21's drifted, which the intent, the
/// triage-drift slice and its stop read. The predicate on a run is `bp-change`'s.
///
/// # Panics
///
/// Never: the index holds ids.
pub fn drift(index: &Index, design: &str) -> Result<Vec<Record>, Refusal> {
    let mut out = Vec::new();
    for r in index.records(design, Kind::Run)? {
        let field = |k: &str| text(&r.header, k).unwrap_or_default();
        if intents::is_drift(&field("verdict"), &field("check_kind"))
            && index.citing(&Id::parse(&r.id).expect("the index holds ids"), &[Kind::Delta, Kind::Thread])?.is_empty()
        {
            out.push(r);
        }
    }
    Ok(out)
}

/// The designs a triage-drift seed covers: the design, or each design of the project (21, triage drift).
pub fn drift_designs(index: &Index, id: &Id, kind: Kind) -> Result<Vec<String>, Refusal> {
    match id.project() {
        Some(project) if kind == Kind::Project => index.ids_of(Kind::Design, project),
        _ => Ok(vec![id.to_string()]),
    }
}

/// The claim records, reduced: the one reading of a release that blocked and `walk --done` share (21, rule 1).
pub fn claims(records: &[Record]) -> Vec<Claim> {
    records
        .iter()
        .map(|c| {
            let release = intents::release(&c.body);
            let to = release.get("to").filter(|t| !t.is_empty());
            Claim {
                at: text(&c.header, "from").and_then(|t| intents::timestamp(&t)),
                // A `to` that is not a timestamp still releases; it reads as the latest time, so an escalation stays blocked.
                released: to.map(|t| intents::timestamp(t).unwrap_or(i64::MAX)),
                escalated: to.is_some() && release.get("reason").map(String::as_str) == Some("escalated"),
            }
        })
        .collect()
}

/// The issue's run records, reduced.
pub fn runs(index: &Index, id: &str) -> Result<Vec<intents::Run>, Refusal> {
    Ok(index
        .records(id, Kind::Run)?
        .iter()
        .map(|r| {
            let verdict = text(&r.header, "verdict");
            intents::Run {
                id: r.id.clone(),
                check: text(&r.header, "check").unwrap_or_default(),
                check_hash: text(&r.header, "check_hash").unwrap_or_default(),
                at: text(&r.header, "at").and_then(|t| intents::timestamp(&t)),
                commit: text(&r.header, "commit").unwrap_or_default(),
                attempt: r.header.get("attempt").and_then(Value::as_u64).unwrap_or_default(),
                // A converted run has no severity; a block's default is `error` (15).
                error: text(&r.header, "severity").is_none_or(|s| s != "warn"),
                failed: matches!(verdict.as_deref(), Some("fail" | "timed-out")),
                passed: matches!(verdict.as_deref(), Some("pass" | "skipped")),
            }
        })
        .collect())
}

/// The issue's named `error` blocks: each needs a passing run of its current text at the head before a land.
///
/// # Panics
///
/// Never: the index holds ids.
pub fn blocks(vault: &Vault, id: &str) -> Result<Vec<Block>, Refusal> {
    let file = vault.read(&Id::parse(id).expect("the index holds ids"))?;
    let named = file.body.named_checks().into_iter().filter(|c| c.is_error());
    Ok(named.map(|c| Block { name: c.attrs["name"].clone(), hash: c.hash() }).collect())
}

/// The project's reviewer identity, the forge login whose approval alone counts; empty when unset (13, Project).
pub fn reviewer(index: &Index, id: &str) -> Result<String, Refusal> {
    let project = Id::parse(id).ok().and_then(|i| i.project().map(str::to_string)).unwrap_or_default();
    Ok(index.header(&format!("projects/{project}/project"))?.and_then(|h| text(&h, "reviewer")).unwrap_or_default())
}

/// The forge's pull request as the predicates read it: its head, its base, green, and the reviewer's approval at the
/// head.
pub fn pull_request(p: &bp_forge::PullRequest, reviewer: &str) -> PullRequest {
    let approved = p.approved_heads(reviewer).contains(&p.head_ref_oid.as_str());
    PullRequest { head: p.head_ref_oid.clone(), base: p.base_ref_name.clone(), green: p.is_green(), approved }
}

pub(crate) fn text(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).and_then(Value::as_str).map(str::to_string)
}

/// The held claim of `issue`: the latest claim with no release `to`.
pub fn held(index: &Index, issue: &Id) -> Result<Option<Record>, Refusal> {
    Ok(index
        .records(issue.as_str(), Kind::Claim)?
        .into_iter()
        .rev()
        .find(|c| intents::release(&c.body).get("to").is_none_or(String::is_empty)))
}

/// The text field `name` of a record's header.
pub fn field(r: &Record, name: &str) -> Option<String> {
    r.header.get(name).and_then(Value::as_str).map(str::to_string)
}

/// The status of the node `id`; `None` when the index holds no node there.
pub fn status(index: &Index, id: &str) -> Result<Option<String>, Refusal> {
    Ok(index.header(id)?.and_then(|h| h.get("status")?.as_str().map(str::to_string)))
}

/// The stack a dependant of `d` would sit on: `d`, then each held claim's unresolved `base_issue` below it, at most
/// [`STACK_DEPTH`] layers ([`intents::Dependency::layers`]).
pub fn layers(index: &Index, d: &str) -> Result<Vec<String>, Refusal> {
    let mut layers = vec![d.to_string()];
    while layers.len() < STACK_DEPTH {
        let Ok(below) = Id::parse(&layers[layers.len() - 1]) else { break };
        let Some(next) = held(index, &below)?.and_then(|h| field(&h, "base_issue")) else { break };
        if status(index, &next)?.as_deref() == Some(IssueStatus::Resolved.as_str()) {
            break;
        }
        layers.push(next);
    }
    Ok(layers)
}

/// The branch a held `claim`'s pull request lands on (15, a dependency chain to landed): its `base_branch`; or, when
/// it was cut from a dependency's head, `Unmerged` until the dependency resolves, then the branch the dependency's
/// landed claim landed on, followed down the stack.
pub fn base(index: &Index, claim: &Record) -> Result<Base, Refusal> {
    let (mut below, mut branch) = (field(claim, "base_issue"), field(claim, "base_branch"));
    // A stack is at most `STACK_DEPTH` layers, so the walk ends within as many steps.
    for _ in 0..STACK_DEPTH {
        let Some(issue) = below.take() else { break };
        if status(index, &issue)?.as_deref() != Some(IssueStatus::Resolved.as_str()) {
            return Ok(Base::Unmerged(issue));
        }
        let landed = index
            .records(&issue, Kind::Claim)?
            .into_iter()
            .rev()
            .find(|c| intents::release(&c.body).get("reason").map(String::as_str) == Some("landed"));
        // A dependency resolved by hand landed nowhere this tool knows.
        let Some(landed) = landed else { return Ok(Base::Unknown) };
        (below, branch) = (field(&landed, "base_issue"), field(&landed, "base_branch"));
    }
    Ok(branch.map_or(Base::Unknown, Base::Branch))
}

/// An open waiver: its id, its milestone, its block's name and the block as the finish line holds it now; `None`
/// when the block is gone, which `stamp` refuses.
#[derive(PartialEq)]
pub struct Reask {
    /// The waiver record.
    pub waiver: Id,
    /// The milestone that holds it.
    pub milestone: Id,
    /// The block it waived.
    pub name: String,
    /// The block now, if the finish line still holds it.
    pub check: Option<Check>,
}

impl Reask {
    /// The `--waive` value that waives it again.
    pub fn key(&self) -> String {
        format!("{}/{}", self.milestone, self.name)
    }
}

/// The project's open waivers, by 21's re-asked predicate: a passing run of the block follows the waiver when git
/// last touched the run, not the waiver. One query finds the milestones that hold a waiver, so a project with none
/// costs one read, which the intents of its `done` milestones pay. `stamp` reads them too (27, design change 10).
///
/// # Panics
///
/// Never: the index holds ids.
pub fn open_waivers(vault: &Vault, index: &Index, project: &str, now: i64) -> Result<Vec<Reask>, Refusal> {
    let all = Filter { kind: Kind::Waiver, status: None, project: Some(project), under: None, limit: crate::ALL };
    let holders: BTreeSet<String> = index.list(&all)?.0.into_iter().filter_map(|w| w.under).collect();
    let (mut waivers, mut runs) = (Vec::new(), BTreeMap::new());
    for m in holders {
        waivers.extend(index.records(&m, Kind::Waiver)?.iter().map(|r| reduce(&m, r)));
        runs.insert(m.clone(), index.records(&m, Kind::Run)?);
    }
    let passed_after = |w: &intents::Waiver| {
        let waiver = Id::parse(&w.id).expect("the index holds ids");
        let passed = runs[&w.milestone].iter().filter(|r| {
            let field = |k: &str| r.header.get(k).and_then(Value::as_str);
            field("check") == Some(w.block.as_str()) && field("verdict") == Some("pass")
        });
        // The latest first: it is the one that follows, if any does.
        passed.rev().any(|r| vault.touched_after(&Id::parse(&r.id).expect("the index holds ids"), &waiver))
    };
    let mut open = Vec::new();
    for w in intents::reasked(&waivers, now, passed_after) {
        let milestone = Id::parse(&w.milestone).expect("the index holds ids");
        let body = vault.read(&milestone)?.body;
        let check = body.named_checks().into_iter().find(|c| c.attrs["name"] == w.block).cloned();
        let waiver = Id::parse(&w.id).expect("the index holds ids");
        open.push(Reask { waiver, milestone, name: w.block.clone(), check });
    }
    Ok(open)
}

/// A waiver record of `milestone`, reduced.
fn reduce(milestone: &str, r: &Record) -> intents::Waiver {
    let key = r.id.rsplit('/').next().unwrap_or(&r.id);
    let field = |k: &str| r.header.get(k).and_then(Value::as_str).filter(|t| !t.is_empty());
    // A block name may end in `-<n>`, so only `check` splits the key `<check>-<n>`; 13's rule 1 on the bare key
    // would misread `walk-8` as the eighth waiver of `walk`. A waiver written by hand without it keys its own block.
    let block = field("check").unwrap_or(key);
    let seq = key.strip_prefix(block).and_then(|n| n.strip_prefix('-')).and_then(|n| n.parse().ok());
    // A `valid_until` that is not an instant reads as past, so the stamp asks again.
    let valid_until = field("valid_until").map(|t| intents::timestamp(t).unwrap_or(i64::MIN));
    intents::Waiver {
        id: r.id.clone(),
        milestone: milestone.into(),
        block: block.into(),
        seq: seq.unwrap_or(1),
        valid_until,
    }
}
