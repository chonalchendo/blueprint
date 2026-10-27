//! The intents of designs and milestones (21): drifted, needs derivation, needs shaping, needs milestone approval,
//! needs acceptance and re-asked, after the issue predicates in the order. Pure, as the issue half is: the caller
//! reads the records a node's status needs and hands them in.

use std::collections::{BTreeMap, BTreeSet};

use super::{Block, Intent, Run, Verdict, is_live_issue};
use crate::{IssueStatus, MilestoneStatus};

/// The batch size for derive, until it is a project setting (21, open questions: three live issues).
pub const BATCH: usize = 3;

/// What the milestone predicates read. Each field past `status` matters only for the status that reads it, so the
/// caller reads it only then: `integrity` for `planned` and `approved`; the issues, and while fewer than [`BATCH`]
/// are live the finish line and the coverage, for `approved`; the runs and `broken` once no issue is live; `reasked`
/// for `done`. A field left empty reads as no record.
#[derive(Debug, Clone, Default)]
pub struct Milestone {
    /// The header status.
    pub status: String,
    /// The blocked note of a failed integrity run that names it ([`super::IntegrityRun::blocks`]).
    pub integrity: Option<String>,
    /// The status of each issue under it.
    pub issues: Vec<String>,
    /// The named blocks of its `## Finish line`, with their hashes now.
    pub finish: Vec<Block>,
    /// The names of the named blocks in the `## Check` of each issue under it, of any status.
    pub covered: BTreeSet<String>,
    /// Its run records, the finish line's.
    pub runs: Vec<Run>,
    /// A file of its project does not parse. A broken issue's milestone is unknown, so any file of the project counts.
    pub broken: bool,
    /// The blocks of its open waivers ([`reasked`]).
    pub reasked: Vec<String>,
}

/// The intent of the milestone `id`. Blocked comes first, for a milestone not yet done or abandoned.
pub fn milestone(id: &str, m: &Milestone) -> Verdict {
    let status = m.status.parse();
    if matches!(status, Ok(MilestoneStatus::Planned | MilestoneStatus::Approved))
        && let Some(note) = &m.integrity
    {
        return Verdict::of(Intent::Blocked, note.clone());
    }
    match status {
        Ok(MilestoneStatus::Planned) => Verdict::of(Intent::NeedsMilestoneApproval, ""),
        Ok(MilestoneStatus::Approved) => match uncovered(m).as_slice() {
            [] if accepting(m) => match failed(m).as_slice() {
                [] => Verdict::of(Intent::NeedsAcceptance, format!("bp stamp {id}")),
                // A waiver exists only after `stamp --waive`, so the note names the waive (21, needs acceptance).
                failed => {
                    let waive: Vec<String> = failed.iter().map(|b| format!("--waive {b} --reason <why>")).collect();
                    let note = format!("{} failed; bp stamp {id} {}", failed.join(", "), waive.join(" "));
                    Verdict::of(Intent::NeedsAcceptance, note)
                }
            },
            [] => Verdict::NONE,
            open => Verdict::of(
                Intent::NeedsDerivation,
                format!("{} uncovered; bp walk {id} --for derive", open.join(", ")),
            ),
        },
        // The node of a re-asked waiver is its milestone, `done`, so no other intent holds there (21).
        Ok(MilestoneStatus::Done) if !m.reasked.is_empty() => {
            // `stamp` takes one `--reason` for each `--waive`, so each waive carries its own.
            let waive: Vec<String> = m.reasked.iter().map(|b| format!("--waive {id}/{b} --reason <why>")).collect();
            Verdict::of(Intent::ReAsked, format!("{}; bp stamp <next> {}", m.reasked.join(", "), waive.join(" ")))
        }
        _ => Verdict::NONE,
    }
}

/// Needs derivation's open blocks (21): while fewer than [`BATCH`] issues are live, the finish-line blocks that no
/// issue's `## Check` names. Empty when the batch is full or the line is covered, which is derive's stop.
pub fn uncovered(m: &Milestone) -> Vec<&str> {
    if m.issues.iter().filter(|s| is_live_issue(s)).count() >= BATCH {
        return Vec::new();
    }
    m.finish.iter().map(|b| b.name.as_str()).filter(|n| !m.covered.contains(*n)).collect()
}

/// Needs acceptance past the status (21): every issue resolved or abandoned, and every file of the project parses.
/// A failed run does not clear it: `stamp` refuses on it, and its note names the waive.
fn accepting(m: &Milestone) -> bool {
    let settled = m.issues.iter().all(|s| matches!(s.parse(), Ok(IssueStatus::Resolved | IssueStatus::Abandoned)));
    settled && !m.broken
}

/// The finish-line blocks whose latest `error` run of their text now failed. A run of an older text says nothing.
fn failed(m: &Milestone) -> Vec<&str> {
    let latest = |b: &Block| {
        let runs = m.runs.iter().filter(|r| r.error && r.check == b.name && r.check_hash == b.hash);
        runs.max_by_key(|r| (r.at, r.attempt)).is_some_and(|r| r.failed)
    };
    m.finish.iter().filter(|b| latest(b)).map(|b| b.name.as_str()).collect()
}

/// A concern answer, reduced (13, `ConcernAnswer`). The latest stamp of a concern is its state now.
#[derive(Debug, Clone, Default)]
pub struct ConcernAnswer {
    /// The concern's slug.
    pub concern: String,
    /// The record's stamp, which orders the states of one concern.
    pub stamp: String,
    /// `designed`, `waived` or `open`.
    pub state: String,
}

/// The concerns whose latest answer is `open` (21, needs shaping).
pub fn open_concerns(answers: &[ConcernAnswer]) -> Vec<String> {
    let mut latest: BTreeMap<&str, &ConcernAnswer> = BTreeMap::new();
    for a in answers {
        let slot = latest.entry(a.concern.as_str()).or_insert(a);
        if a.stamp > slot.stamp {
            *slot = a;
        }
    }
    latest.into_values().filter(|a| a.state == "open").map(|a| a.concern.clone()).collect()
}

/// A drift run (21, drifted; 13, Run): a run of a design's check that failed or timed out. An integrity run checks the
/// vault, not a design, and a run that could not run or was skipped says nothing about the code.
pub fn is_drift(verdict: &str, check_kind: &str) -> bool {
    matches!(verdict, "fail" | "timed-out") && check_kind != "integrity"
}

/// The intent of the design `id`: drifted, then needs shaping (21). `uncited` holds its failed drift runs that no
/// delta or thread cites. `open` holds the project's [`open_concerns`] when `id` is the project's root design and is
/// `active`, else none, so needs shaping is one node per project; `due` says that a milestone of the project is
/// `planned` or `approved`.
pub fn design(id: &str, uncited: &[String], open: &[String], due: bool) -> Verdict {
    if !uncited.is_empty() {
        return Verdict::of(Intent::Drifted, format!("{}; bp walk {id} --for triage-drift", uncited.join(", ")));
    }
    if !open.is_empty() && !due {
        return Verdict::of(Intent::NeedsShaping, format!("{} open; bp walk {id} --for shape", open.join(", ")));
    }
    Verdict::NONE
}

/// A waiver record, reduced (13, Waiver). Its key is the milestone, the block, its `check`, and the sequence, read
/// from the record key relative to `check`: `<check>` is 1, `<check>-<n>` is n.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waiver {
    /// The waiver record's id.
    pub id: String,
    /// The milestone whose finish line holds the block.
    pub milestone: String,
    /// The block's name.
    pub block: String,
    /// The sequence: a later waiver of one block takes the next.
    pub seq: u64,
    /// `valid_until`, Unix seconds; `None` when empty.
    pub valid_until: Option<i64>,
}

/// Re-asked (21): each waiver that is the latest of its block, whose `valid_until` is empty or past, and that no
/// passing run of its block follows, by commit order. `passed_after` answers that last test, which needs git, so it
/// runs only for a latest waiver still in date. `stamp` calls the same function.
pub fn reasked(waivers: &[Waiver], now: i64, mut passed_after: impl FnMut(&Waiver) -> bool) -> Vec<&Waiver> {
    let mut latest: BTreeMap<(&str, &str), &Waiver> = BTreeMap::new();
    for w in waivers {
        let slot = latest.entry((w.milestone.as_str(), w.block.as_str())).or_insert(w);
        if w.seq > slot.seq {
            *slot = w;
        }
    }
    latest.into_values().filter(|w| w.valid_until.is_none_or(|t| t <= now) && !passed_after(w)).collect()
}
