//! The `Intents` function (21): one intent per live node, the first true predicate in a fixed order. Blocked comes
//! first, because a blocked issue must never read as ready. Pure: the caller hands in the node's records, its
//! dependencies' states and the forge's view of its pull request.
//!
//! The issue predicates live here; the design and milestone predicates, which follow them in the order, live in
//! [`planning`].

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::IssueStatus;

mod gates;
mod planning;
pub use gates::*;
pub use planning::*;

/// Twelve hours: a claim older than this with no run since reads `working (stale)`.
pub const STALE_AFTER: i64 = 12 * 3600;

/// The intents, in predicate order (21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
#[allow(missing_docs)]
pub enum Intent {
    Blocked,
    Ready,
    Waiting,
    Working,
    NeedsReview,
    NeedsResponse,
    NeedsApproval,
    NeedsLanding,
    Drifted,
    NeedsDerivation,
    NeedsShaping,
    NeedsMilestoneApproval,
    NeedsAcceptance,
    ReAsked,
}

impl Intent {
    /// Every intent, in predicate order: a reader that places each one (the Moving groups) iterates it.
    pub const ALL: [Self; 14] = [
        Self::Blocked,
        Self::Ready,
        Self::Waiting,
        Self::Working,
        Self::NeedsReview,
        Self::NeedsResponse,
        Self::NeedsApproval,
        Self::NeedsLanding,
        Self::Drifted,
        Self::NeedsDerivation,
        Self::NeedsShaping,
        Self::NeedsMilestoneApproval,
        Self::NeedsAcceptance,
        Self::ReAsked,
    ];
}

/// A claim record, reduced to what the predicates read.
#[derive(Debug, Clone, Default)]
pub struct Claim {
    /// `from`, Unix seconds.
    pub at: Option<i64>,
    /// The release's `to`, Unix seconds; `None` while held (13: `to` is absent while held).
    pub released: Option<i64>,
    /// The release reason was `escalated`.
    pub escalated: bool,
}

/// A run record, reduced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    /// The run record's id, which `land`'s `check_failed` names.
    pub id: String,
    /// The block's name.
    pub check: String,
    /// The block's hash when it ran; the run counts for the block only while it matches (26, design change 4).
    pub check_hash: String,
    /// `at`, Unix seconds.
    pub at: Option<i64>,
    /// The commit the run verified.
    pub commit: String,
    /// The attempt at that commit; the latest run of a check at a commit is the highest (13, Run).
    pub attempt: u64,
    /// The block's severity is `error`; a `warn` run never blocks (21, blocked).
    pub error: bool,
    /// The verdict was `fail` or `timed-out`. `could-not-run` and `skipped` say nothing about the code.
    pub failed: bool,
    /// The verdict was `pass`, or `skipped`: a `when=post-land` block holds only after the merge. Only these count as
    /// evidence for a land; a run that could not run proves nothing.
    pub passed: bool,
}

/// An integrity run of a project, reduced (25, issue 11).
#[derive(Debug, Clone, Default)]
pub struct IntegrityRun {
    /// The run record's id.
    pub id: String,
    /// The node that owns the block.
    pub owner: String,
    /// The block's name.
    pub check: String,
    /// `at`, Unix seconds.
    pub at: Option<i64>,
    /// The attempt at its commit.
    pub attempt: u64,
    /// The block's severity is `error`.
    pub error: bool,
    /// The verdict.
    pub verdict: String,
    /// The nodes it failed.
    pub names: BTreeSet<String>,
}

impl IntegrityRun {
    /// The project's run that `blocked` reads (21): the latest `error` run that passed or failed. A run that could
    /// not run or was skipped says nothing about the vault, and a `warn` run never blocks. Integrity is vault state,
    /// so the latest is the latest across commits: the highest `at`, then attempt, then id, so a tie is decided.
    pub fn counting(runs: &[Self]) -> Option<&Self> {
        runs.iter()
            .filter(|r| r.error && matches!(r.verdict.as_str(), "pass" | "fail"))
            .max_by(|a, b| (a.at, a.attempt, &a.id).cmp(&(b.at, b.attempt, &b.id)))
    }

    /// The blocked note when this run failed and names `id`: the run, and the rerun that clears it once the node is
    /// fixed, since a run older than the fix still blocks (12, a fresh clone).
    pub fn blocks(&self, id: &str) -> Option<String> {
        (self.verdict == "fail" && self.names.contains(id))
            .then(|| format!("integrity: {}; rerun bp check {} --name {}", self.id, self.owner, self.check))
    }
}

/// A named `error` block of the issue, as its runs name it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Block {
    /// The block's name.
    pub name: String,
    /// The block's hash now, as a run records it.
    pub hash: String,
}

/// The forge's view of the issue's pull request: the open one, or the merged one of a held claim, which `land`
/// finishes.
#[derive(Debug, Clone, Default)]
pub struct PullRequest {
    /// The head commit.
    pub head: String,
    /// The branch it targets; a snapshot written before milestone 3 lacks it.
    pub base: Option<String>,
    /// Every check at the head succeeded, and there is at least one.
    pub green: bool,
    /// A check at the head ended in a way no wait turns green.
    pub failed: bool,
    /// The reviewer identity's approving review at the head exists; no other login's counts, and no earlier head's,
    /// as the forge requires an approval of the latest push (15, Controls 1).
    pub approved: bool,
    /// The reviewer identity's login; empty when the project names none.
    pub reviewer: String,
}

/// The branch a held claim's pull request lands on (15, a dependency chain to landed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Base {
    /// Not recorded: a converted claim names no base branch. `land` tests origin's default branch; a read without the
    /// clone cannot, so needs landing holds and the Change page withholds the command.
    #[default]
    Unknown,
    /// This branch.
    Branch(String),
    /// The claim was cut from this dependency's pull request head, and the dependency is not resolved yet.
    Unmerged(String),
}

/// A `depends` target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dependency {
    /// The target id.
    pub id: String,
    /// The target's status; `None` when the target is not in the vault.
    pub status: Option<String>,
    /// The target holds an open pull request.
    pub open_pr: bool,
    /// The stack a dependant would sit on: the target, then each held claim's unresolved `base_issue` below it, at
    /// most [`STACK_DEPTH`]. Read only for an unresolved target with an open pull request.
    pub layers: Vec<String>,
}

/// A stack is at most three layers deep; the cap is a choice, not a measurement (15, a dependency chain to landed).
pub const STACK_DEPTH: usize = 3;

/// Why an issue cannot be cut on its unresolved dependencies (15, a dependency chain to landed).
#[derive(Debug, PartialEq, Eq)]
pub enum Unstacked<'a> {
    /// These hold no open pull request to base on (`dependency_unpushed`).
    Unpushed(Vec<&'a str>),
    /// No one stack holds them all (`dependency_split`).
    Split(Vec<&'a str>),
    /// The new layer would sit on these, the top first, past the cap (`stack_too_deep`).
    Deep(&'a [String]),
}

/// The dependency an issue is cut on: the unresolved one whose stack holds every other; `None` when each is resolved.
/// `cut` refuses on the error and ready waits on it, so the two never disagree (21, rule 1).
pub fn stack(depends: &[Dependency]) -> Result<Option<&Dependency>, Unstacked<'_>> {
    let below: Vec<&Dependency> = depends.iter().filter(|d| d.status.as_deref() != Some("resolved")).collect();
    let unpushed: Vec<&str> = below.iter().filter(|d| !d.open_pr).map(|d| d.id.as_str()).collect();
    if !unpushed.is_empty() {
        return Err(Unstacked::Unpushed(unpushed));
    }
    if below.is_empty() {
        return Ok(None);
    }
    let Some(&top) = below.iter().find(|d| below.iter().all(|o| d.layers.contains(&o.id))) else {
        return Err(Unstacked::Split(below.iter().map(|d| d.id.as_str()).collect()));
    };
    if top.layers.len() >= STACK_DEPTH {
        return Err(Unstacked::Deep(&top.layers));
    }
    Ok(Some(top))
}

/// Everything the issue predicates read. The caller builds it only for a live issue.
#[derive(Debug, Clone, Default)]
pub struct Issue {
    /// The header status: `open` or `in-progress`.
    pub status: String,
    /// Claim records.
    pub claims: Vec<Claim>,
    /// Run records.
    pub runs: Vec<Run>,
    /// The issue's named `error` blocks; read only while a pull request is in view.
    pub blocks: Vec<Block>,
    /// `head` of each review record.
    pub review_heads: Vec<String>,
    /// The findings of the review at the head that no disposition answers, counted from 1; read only while a
    /// pull request is in view.
    pub unanswered: Vec<u64>,
    /// The `depends` targets.
    pub depends: Vec<Dependency>,
    /// The open pull request, or the merged one of a held claim; `None` when there is none or the forge was not read.
    pub pr: Option<PullRequest>,
    /// The held claim's base; read only while a pull request is in view.
    pub base: Base,
    /// A commit after the latest claim's release commit touched the issue file: commit order, not time, so an answer
    /// in the release's own second counts (21, blocked). Read only after an escalated release.
    pub answered: bool,
    /// The note of the project's counting integrity run, when it failed and names the issue
    /// ([`IntegrityRun::blocks`]).
    pub integrity: Option<String>,
}

/// One node's intent and its note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verdict {
    /// The first true predicate; `None` when none holds.
    pub intent: Option<Intent>,
    /// What the intent names: the failed run, the targets waited on, `stale`.
    pub note: String,
}

impl Verdict {
    fn of(intent: Intent, note: impl Into<String>) -> Self {
        Self { intent: Some(intent), note: note.into() }
    }

    /// No predicate holds.
    pub const NONE: Self = Self { intent: None, note: String::new() };
}

/// A live issue: open or in progress.
pub fn is_live_issue(status: &str) -> bool {
    matches!(status.parse(), Ok(IssueStatus::Open | IssueStatus::InProgress))
}

/// The latest claim, the one an escalation is read from; claims without a time sort first, as they predate the field.
pub fn latest_claim(claims: &[Claim]) -> Option<&Claim> {
    claims.iter().max_by_key(|c| c.at)
}

/// The intent of one live issue at `now` (Unix seconds).
pub fn issue(i: &Issue, now: i64) -> Verdict {
    let held = i.claims.iter().find(|c| c.released.is_none());
    if let Some(pr) = &i.pr
        && failed_at(&i.runs, &pr.head)
    {
        return Verdict::of(Intent::Blocked, "run failed at head");
    }
    // Escalated holds until the issue changes after the release: the human's answer is a write to the issue.
    if latest_claim(&i.claims).is_some_and(|c| c.escalated) && !i.answered {
        return Verdict::of(Intent::Blocked, "escalated");
    }
    for d in &i.depends {
        match d.status.as_deref() {
            None => return Verdict::of(Intent::Blocked, format!("depends on missing {}", d.id)),
            Some("abandoned") => return Verdict::of(Intent::Blocked, format!("depends on abandoned {}", d.id)),
            Some(_) => {}
        }
    }
    if let Some(note) = &i.integrity {
        return Verdict::of(Intent::Blocked, note.clone());
    }
    if i.status == "open" && held.is_none() && i.pr.is_none() {
        return match stack(&i.depends) {
            Ok(_) => Verdict::of(Intent::Ready, ""),
            Err(Unstacked::Unpushed(waits)) => Verdict::of(Intent::Waiting, format!("waits for {}", waits.join(", "))),
            Err(Unstacked::Split(ids)) => {
                Verdict::of(Intent::Waiting, format!("waits for one stack: land all but one of {}", ids.join(", ")))
            }
            Err(Unstacked::Deep(layers)) => {
                let bottom = layers.last().map(String::as_str).unwrap_or_default();
                Verdict::of(Intent::Waiting, format!("waits for a stack under three layers: land {bottom}"))
            }
        };
    }
    let Some(pr) = &i.pr else {
        let Some(c) = held else { return Verdict::NONE };
        let stale =
            c.at.is_some_and(|at| now - at > STALE_AFTER && !i.runs.iter().any(|r| r.at.is_none_or(|t| t > at)));
        return Verdict::of(Intent::Working, if stale { "stale" } else { "" });
    };
    // The forge approval is the gate, not the vault's review record: `land` reads none, so neither does this (21).
    if pr.approved && pr.green {
        return match landing(pr, &i.base, &i.runs, &i.blocks) {
            Ok(()) => Verdict::of(Intent::NeedsLanding, ""),
            // No agent action helps: the human lands the layer below, which reads needs landing itself.
            Err(Unmet::Unmerged(a)) => Verdict::of(Intent::Waiting, format!("waits for {a} to land")),
            Err(Unmet::Mismatch(b)) => Verdict::of(Intent::Working, format!("not on {b}: bp pr")),
            Err(Unmet::Failed(r)) => Verdict::of(Intent::Working, format!("{} did not pass at head: bp pr", r.check)),
            Err(Unmet::Evidence(b)) => Verdict::of(Intent::Working, format!("no run of {b} at head: bp pr")),
            Err(Unmet::Review | Unmet::Checks) => unreachable!("approved and green"),
            Err(Unmet::ChecksChanged) => unreachable!("landing reads no check set"),
        };
    }
    if !i.review_heads.contains(&pr.head) {
        return Verdict::of(Intent::NeedsReview, "");
    }
    if !pr.green {
        return Verdict::of(Intent::Working, "checks not green");
    }
    if !i.unanswered.is_empty() {
        let rows: Vec<String> = i.unanswered.iter().map(u64::to_string).collect();
        return Verdict::of(Intent::NeedsResponse, format!("no disposition: findings {}", rows.join(", ")));
    }
    Verdict::of(Intent::NeedsApproval, "")
}

/// The first part of needs landing that does not hold, in `land`'s refusal order (26, design change 4).
#[derive(Debug, PartialEq, Eq)]
pub enum Unmet<'a> {
    /// The claim was cut from this dependency's head, which has not landed (`base_unmerged`).
    Unmerged(&'a str),
    /// The pull request does not target this branch, the claim's base (`base_mismatch`).
    Mismatch(&'a str),
    /// No approving review from the reviewer identity at the head; `land` reads the forge again to say whether one
    /// at an earlier head saw another diff (`diff_changed`).
    Review,
    /// The forge check at the head is not green.
    Checks,
    /// The latest run of this `error` block at the head did not pass.
    Failed(&'a Run),
    /// This `error` block has no run at the head of its current text: none, or one made before the block changed.
    Evidence(&'a str),
    /// The head names another check set than the one the pull request carries now (`checks_changed`; 27, design
    /// change 8): a block changed after the push the reviewer approved.
    ChecksChanged,
}

/// Needs landing (21): the pull request on the claim's `base`, the reviewer's approval at the head, the forge check
/// green at the head, and the run [`evidence`], in that order (26, design change 4). `land` refuses on the first part
/// that does not hold, and the `Intents` function reads the same answer, so the two never disagree (21, rule 1).
/// It is the first-unmet projection of [`gates`], which the Change page reads whole, without the check set: that
/// needs the clone, which `land` reads and passes to [`gates`] itself.
pub fn landing<'a>(pr: &'a PullRequest, base: &'a Base, runs: &'a [Run], blocks: &'a [Block]) -> Result<(), Unmet<'a>> {
    gates(pr, base, runs, blocks, None).unmet.map_or(Ok(()), Err)
}

/// The run part of needs landing: the latest run of each `error` check at `head` passed, and each block has one whose
/// `check_hash` is the block's hash now. `pr` reruns the blocks until this holds.
pub fn evidence<'a>(runs: &'a [Run], head: &str, blocks: &'a [Block]) -> Result<(), Unmet<'a>> {
    let latest = latest_at(runs, head);
    if let Some(r) = latest.values().find(|r| !r.passed) {
        return Err(Unmet::Failed(r));
    }
    match blocks.iter().find(|b| latest.get(b.name.as_str()).is_none_or(|r| r.check_hash != b.hash)) {
        Some(b) => Err(Unmet::Evidence(&b.name)),
        None => Ok(()),
    }
}

/// The latest run of an `error` check at `head` failed: blocked's first test, and the view's "red at head".
pub fn failed_at(runs: &[Run], head: &str) -> bool {
    latest_at(runs, head).values().any(|r| r.failed)
}

/// The latest run of each `error` check at `head`; `at` is to the second, so attempts order the reruns.
fn latest_at<'a>(runs: &'a [Run], head: &str) -> BTreeMap<&'a str, &'a Run> {
    let mut latest: BTreeMap<&str, &Run> = BTreeMap::new();
    for r in runs.iter().filter(|r| r.error && r.commit == head) {
        let slot = latest.entry(r.check.as_str()).or_insert(r);
        if r.attempt > slot.attempt {
            *slot = r;
        }
    }
    latest
}

/// A claim's release: the `key: value` lines of its appended `## Released` section (13). Empty while it is held.
pub fn release(body: &str) -> BTreeMap<String, String> {
    let body = bp_vault::Body::parse(body);
    let lines = body.sections_named("Released").into_iter().flat_map(|s| s.text.lines());
    lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

/// `YYYY-MM-DDTHH:MM:SSZ` as Unix seconds. Every timestamp the vault writes is UTC with `Z`; anything else is `None`.
pub fn timestamp(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let seps = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':'), (19, b'Z')];
    if b.len() != 20 || seps.iter().any(|&(at, sep)| b[at] != sep) {
        return None;
    }
    let field = |at: usize, len: usize, max: i64| -> Option<i64> {
        let digits = &b[at..at + len];
        let n = digits.iter().try_fold(0, |n, d| d.is_ascii_digit().then(|| n * 10 + i64::from(d - b'0')))?;
        (n <= max).then_some(n)
    };
    let (year, month) = (field(0, 4, 9999)?, field(5, 2, 12)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    let day = field(8, 2, last)?;
    if month == 0 || day == 0 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm), so no calendar crate is needed for one subtraction.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + field(11, 2, 23)? * 3600 + field(14, 2, 59)? * 60 + field(17, 2, 59)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_792_281_600; // 2026-10-18T00:00:00Z

    fn open() -> Issue {
        Issue { status: "open".into(), ..Issue::default() }
    }

    fn dep(id: &str, status: &str, open_pr: bool) -> Dependency {
        let layers = if open_pr { vec![id.to_string()] } else { Vec::new() };
        Dependency { id: id.into(), status: Some(status.into()), open_pr, layers }
    }

    const BASE: Base = Base::Unknown;

    fn pr(head: &str, green: bool, approved: bool) -> PullRequest {
        PullRequest { head: head.into(), base: Some("master".into()), green, approved, ..PullRequest::default() }
    }

    fn run(check: &str, at: i64, commit: &str, failed: bool) -> Run {
        let id = format!("{check}-{commit}");
        Run {
            id,
            check: check.into(),
            check_hash: format!("#{check}"),
            at: Some(at),
            commit: commit.into(),
            attempt: 1,
            error: true,
            failed,
            passed: !failed,
        }
    }

    fn block(name: &str) -> Block {
        Block { name: name.into(), hash: format!("#{name}") }
    }

    fn released(at: i64) -> Claim {
        Claim { at: Some(at), released: Some(at + 60), escalated: false }
    }

    #[test]
    fn a_design_drifts_while_a_failed_drift_run_is_uncited() {
        assert_eq!(design("d", &[], &[], false), Verdict::NONE);
        let v = design("d", &["d/runs/x-1".to_string()], &["c".to_string()], false);
        assert_eq!(v, Verdict::of(Intent::Drifted, "d/runs/x-1; bp walk d --for triage-drift"));
    }

    #[test]
    fn a_failed_or_timed_out_design_run_is_drift() {
        assert!(is_drift("fail", "shell") && is_drift("timed-out", "scenario"));
        assert!(!is_drift("could-not-run", "shell") && !is_drift("skipped", "shell") && !is_drift("pass", "shell"));
        assert!(!is_drift("fail", "integrity"), "an integrity run checks the vault");
    }

    #[test]
    fn the_latest_error_integrity_run_that_passed_or_failed_counts() {
        let a = "projects/bp/issues/a";
        let run = |id: &str, at: i64, attempt: u64, verdict: &str| IntegrityRun {
            id: id.into(),
            owner: "projects/bp/milestones/m".into(),
            check: "integrity".into(),
            at: Some(at),
            attempt,
            error: true,
            verdict: verdict.into(),
            names: if verdict == "fail" { BTreeSet::from([a.to_string()]) } else { BTreeSet::new() },
        };
        let latest = |runs: &[IntegrityRun]| IntegrityRun::counting(runs).map(|r| r.id.clone());
        let failed = run("r1", NOW, 1, "fail");
        let note = "integrity: r1; rerun bp check projects/bp/milestones/m --name integrity";
        assert_eq!(failed.blocks(a).as_deref(), Some(note));
        assert_eq!(failed.blocks("projects/bp/issues/b"), None, "only the named node");
        assert_eq!(latest(&[]), None);
        assert_eq!(latest(&[failed.clone(), run("r2", NOW + 60, 1, "pass")]).as_deref(), Some("r2"), "a pass clears");
        for skip in ["could-not-run", "skipped", "timed-out"] {
            let runs = [failed.clone(), run("r2", NOW + 60, 1, skip)];
            assert_eq!(latest(&runs).as_deref(), Some("r1"), "{skip} says nothing about the vault");
        }
        let warn = IntegrityRun { error: false, ..run("r2", NOW + 60, 1, "pass") };
        assert_eq!(latest(&[failed.clone(), warn]).as_deref(), Some("r1"), "a warn run never counts");
        let rerun = [run("r2", NOW, 2, "pass"), failed.clone()];
        assert_eq!(latest(&rerun).as_deref(), Some("r2"), "one second: the highest attempt");
        let tie = [run("b-run", NOW, 1, "pass"), run("a-run", NOW, 1, "fail")];
        assert_eq!(latest(&tie).as_deref(), Some("b-run"), "then the id, whatever the read order");
        let i = Issue { integrity: failed.blocks(a), ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Blocked, note));
        let m = |status: &str| Milestone { status: status.into(), integrity: failed.blocks(a), ..Milestone::default() };
        assert_eq!(milestone("m", &m("planned")), Verdict::of(Intent::Blocked, note));
        assert_eq!(milestone("m", &m("approved")).intent, Some(Intent::Blocked));
        assert_eq!(milestone("m", &m("done")), Verdict::NONE, "a done milestone has nothing left to do");
    }

    #[test]
    fn a_release_is_the_appended_section() {
        let r = release("## Released\n\nto: 2026-10-13T10:09:12Z\nreason: landed\ncost_tokens: 9\n");
        assert_eq!(r["to"], "2026-10-13T10:09:12Z");
        assert_eq!(r["reason"], "landed");
        assert_eq!(release("## Released\n\nto:\n")["to"], "", "an empty `to` reads as no release");
        assert!(release("").is_empty(), "a held claim has no release");
    }

    #[test]
    fn parses_utc_timestamps_and_nothing_else() {
        assert_eq!(timestamp("2026-10-18T00:00:00Z"), Some(NOW));
        assert_eq!(timestamp("1970-01-01T00:00:01Z"), Some(1));
        assert_eq!(timestamp("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert!(timestamp("2000-02-29T00:00:00Z").is_some() && timestamp("2026-04-30T00:00:00Z").is_some());
        for bad in [
            "2026-02-30T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-10-18 00:00:00",
            "2026-13-45T99:99:99Z",
            "2026-10-18T00:00:00ZZZ",
            "+026-10-18T00:00:00Z",
            "2026-10-18T00:00:00+00:00",
            "2026-00-18T00:00:00Z",
            "2026-10-18T24:00:00Z",
        ] {
            assert_eq!(timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn ready_waiting_and_their_dependencies() {
        assert_eq!(issue(&open(), NOW).intent, Some(Intent::Ready));
        let i = Issue { depends: vec![dep("a", "resolved", false), dep("b", "open", true)], ..open() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::Ready), "an open pull request on the target is enough");
        let i = Issue { depends: vec![dep("a", "open", false), dep("b", "open", true)], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Waiting, "waits for a"), "the note names only blockers");
        let i = Issue { claims: vec![released(NOW - 60)], depends: vec![dep("a", "open", false)], ..open() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::Waiting), "a released claim is no claim");
        let missing = Dependency { id: "gone".into(), status: None, ..Dependency::default() };
        let i = Issue { depends: vec![missing], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Blocked, "depends on missing gone"));
        let i = Issue { depends: vec![dep("x", "abandoned", false)], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Blocked, "depends on abandoned x"));
        let own = Issue { pr: Some(pr("h", false, false)), ..open() };
        assert_eq!(
            issue(&own, NOW).intent,
            Some(Intent::NeedsReview),
            "an issue with its own pull request is not ready"
        );
    }

    #[test]
    fn working_and_stale() {
        let claim = |at| Claim { at, ..Claim::default() };
        let i = Issue { status: "in-progress".into(), claims: vec![claim(Some(NOW - 3600))], ..Issue::default() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Working, ""));
        let mut i = Issue { claims: vec![claim(Some(NOW - 13 * 3600))], ..i };
        assert_eq!(issue(&i, NOW).note, "stale");
        i.runs.push(run("c", NOW - 3600, "", false));
        assert_eq!(issue(&i, NOW).note, "", "a run since the claim is progress");
        let i = Issue { claims: vec![claim(None)], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Working, ""), "a claim with no time is never stale");
        let i = Issue { claims: vec![claim(Some(NOW - 7200)), released(NOW - 3600)], ..i };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::Working), "any held claim holds, not only the latest");
    }

    #[test]
    fn the_review_path_in_order() {
        let held = Issue { status: "in-progress".into(), claims: vec![Claim::default()], ..Issue::default() };
        let i = Issue { pr: Some(pr("h", true, false)), ..held.clone() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsReview));
        let i = Issue { review_heads: vec!["h".into(), "old".into()], ..i };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsApproval), "a review for h counts in any order");
        let red = Issue { pr: Some(pr("h", false, true)), ..i.clone() };
        assert_eq!(issue(&red, NOW), Verdict::of(Intent::Working, "checks not green"));
        let i = Issue { pr: Some(pr("h", true, true)), ..i };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsLanding));
        let failed = Issue { runs: vec![run("a", NOW, "h", true)], ..i.clone() };
        assert_eq!(issue(&failed, NOW), Verdict::of(Intent::Blocked, "run failed at head"));
        let two = Issue { runs: vec![run("a", NOW - 60, "h", true), run("b", NOW, "h", false)], ..i.clone() };
        assert_eq!(issue(&two, NOW).intent, Some(Intent::Blocked), "the latest run of each check counts");
        let again = Run { attempt: 2, ..run("a", NOW, "h", false) };
        let rerun = Issue { runs: vec![run("a", NOW, "h", true), again.clone()], ..i.clone() };
        assert_eq!(issue(&rerun, NOW).intent, Some(Intent::NeedsLanding), "a passing rerun clears its check");
        let rerun = Issue { runs: vec![again, run("a", NOW, "h", true)], ..i.clone() };
        assert_eq!(issue(&rerun, NOW).intent, Some(Intent::NeedsLanding), "the highest attempt, not the order read");
        let warn = Issue { runs: vec![Run { error: false, ..run("w", NOW, "h", true) }], ..i.clone() };
        assert_eq!(issue(&warn, NOW).intent, Some(Intent::NeedsLanding), "a failing warn run does not block");
        let old = Issue { runs: vec![run("a", NOW, "old", true)], ..i };
        assert_eq!(issue(&old, NOW).intent, Some(Intent::NeedsLanding), "only runs at the head count");
    }

    #[test]
    fn landing_refuses_in_order_and_needs_a_passing_run_of_each_error_block() {
        let blocks = vec![block("a"), block("b")];
        let runs = vec![run("a", NOW, "h", false), run("b", NOW, "h", false)];
        assert_eq!(
            landing(&pr("h", false, false), &BASE, &runs, &blocks),
            Err(Unmet::Review),
            "the review comes first"
        );
        assert_eq!(landing(&pr("h", false, true), &BASE, &runs, &blocks), Err(Unmet::Checks));
        assert_eq!(landing(&pr("h", true, true), &BASE, &runs, &blocks), Ok(()));
        assert_eq!(landing(&pr("h", true, true), &BASE, &runs[..1], &blocks), Err(Unmet::Evidence("b")));
        assert_eq!(
            landing(&pr("g", true, true), &BASE, &runs, &blocks),
            Err(Unmet::Evidence("a")),
            "only runs at the head"
        );
        let failed = [run("a", NOW, "h", true), runs[1].clone()];
        assert_eq!(landing(&pr("h", true, true), &BASE, &failed, &blocks), Err(Unmet::Failed(&failed[0])));
        let unrun = [Run { passed: false, ..runs[0].clone() }, runs[1].clone()];
        assert_eq!(
            landing(&pr("h", true, true), &BASE, &unrun, &blocks),
            Err(Unmet::Failed(&unrun[0])),
            "could not run"
        );
        assert_eq!(landing(&pr("h", true, true), &BASE, &[], &[]), Ok(()), "an issue with no error block needs no run");
        let edited = [block("a"), Block { hash: "#a2".into(), ..block("b") }];
        assert_eq!(
            landing(&pr("h", true, true), &BASE, &runs, &edited),
            Err(Unmet::Evidence("b")),
            "b changed after its run"
        );

        let held = Issue { status: "in-progress".into(), claims: vec![Claim::default()], ..Issue::default() };
        let i = Issue { pr: Some(pr("h", true, true)), review_heads: vec!["h".into()], blocks, runs, ..held };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsLanding));
        let unread = Issue { review_heads: Vec::new(), ..i.clone() };
        assert_eq!(
            issue(&unread, NOW).intent,
            Some(Intent::NeedsLanding),
            "land reads no review record, nor does this"
        );
        let stale = Issue { runs: Vec::new(), ..i.clone() };
        assert_eq!(issue(&stale, NOW), Verdict::of(Intent::Working, "no run of a at head: bp pr"), "pr is due again");
        let unrun = Issue { runs: vec![Run { passed: false, ..i.runs[0].clone() }, i.runs[1].clone()], ..i };
        assert_eq!(issue(&unrun, NOW), Verdict::of(Intent::Working, "a did not pass at head: bp pr"), "could not run");
    }

    #[test]
    fn ready_and_cut_share_one_stack_test() {
        let on = |id: &str, layers: &[&str]| Dependency {
            layers: layers.iter().map(|&l| l.into()).collect(),
            ..dep(id, "open", true)
        };
        assert_eq!(stack(&[dep("a", "resolved", false)]), Ok(None));
        let (b, a) = (on("b", &["b", "a"]), on("a", &["a"]));
        assert_eq!(stack(&[a.clone(), b.clone()]).map(|d| d.map(|d| d.id.clone())), Ok(Some("b".into())), "the top");
        let split = [on("a", &["a"]), on("x", &["x"])];
        assert_eq!(stack(&split), Err(Unstacked::Split(vec!["a", "x"])));
        let full = [on("c", &["c", "b", "a"])];
        assert_eq!(stack(&full), Err(Unstacked::Deep(&full[0].layers)));
        let i = Issue { depends: split.to_vec(), ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Waiting, "waits for one stack: land all but one of a, x"));
        let i = Issue { depends: full.to_vec(), ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Waiting, "waits for a stack under three layers: land a"));
        let i = Issue { depends: vec![a, b], ..open() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::Ready));
    }

    #[test]
    fn landing_reads_the_stack_base_first() {
        let (master, a) = (Base::Branch("master".into()), "projects/bp/issues/a");
        let below = Base::Unmerged(a.into());
        let ok = pr("h", true, true);
        assert_eq!(landing(&ok, &master, &[], &[]), Ok(()));
        assert_eq!(landing(&pr("h", false, false), &below, &[], &[]), Err(Unmet::Unmerged(a)), "the base comes first");
        let stacked = Base::Branch("bp/a".into());
        assert_eq!(landing(&ok, &stacked, &[], &[]), Err(Unmet::Mismatch("bp/a")));
        let unread = PullRequest { base: None, ..ok.clone() };
        assert_eq!(landing(&unread, &stacked, &[], &[]), Ok(()), "a snapshot with no base is not tested");
        assert_eq!(
            landing(&pr("h", true, false), &master, &[], &[]),
            Err(Unmet::Review),
            "only an approval at the head"
        );

        let held = Issue {
            status: "in-progress".into(),
            claims: vec![Claim::default()],
            review_heads: vec!["h".into()],
            ..Issue::default()
        };
        let i = Issue { pr: Some(ok.clone()), base: below, ..held.clone() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Waiting, format!("waits for {a} to land")));
        let i = Issue { pr: Some(ok), base: master.clone(), ..held.clone() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsLanding));
        let i = Issue { pr: Some(pr("h", true, false)), base: master, ..held };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsApproval), "a push needs a new approval");
    }

    #[test]
    fn escalated_blocks_until_the_issue_changes() {
        let escalated = Claim { at: Some(NOW - 600), released: Some(NOW - 300), escalated: true };
        let i = Issue { claims: vec![escalated], answered: false, ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Blocked, "escalated"));
        let answered = Issue { answered: true, ..i };
        assert_eq!(issue(&answered, NOW).intent, Some(Intent::Ready));
    }

    #[test]
    fn a_waiver_is_reasked_while_latest_in_date_and_not_passed() {
        let w = |id: &str, block: &str, seq, valid_until| Waiver {
            id: id.into(),
            milestone: "m".into(),
            block: block.into(),
            seq,
            valid_until,
        };
        let all =
            [w("f", "f", 1, None), w("f-2", "f", 2, None), w("g", "g", 1, Some(NOW + 1)), w("h", "h", 1, Some(NOW))];
        let ids = |passed: &[&str]| -> Vec<String> {
            reasked(&all, NOW, |w| passed.contains(&w.id.as_str())).iter().map(|w| w.id.clone()).collect()
        };
        assert_eq!(ids(&[]), ["f-2", "h"], "the latest of f; g is in date; h's date is past");
        assert_eq!(ids(&["f-2"]), ["h"], "a passing run after the latest closes it");
        assert_eq!(ids(&["f"]), ["f-2", "h"], "a run after an earlier waiver says nothing about the latest");
    }

    #[test]
    fn milestones() {
        let of = |m: &Milestone| milestone("m", m).intent;
        let status = |s: &str| Milestone { status: s.into(), ..Milestone::default() };
        assert_eq!(of(&status("planned")), Some(Intent::NeedsMilestoneApproval));
        assert_eq!(of(&status("in-progress")), None);
        assert_eq!(of(&status("done")), None);
        assert!(is_live_issue("in-progress") && !is_live_issue("abandoned"));

        let approved = Milestone { finish: vec![block("a"), block("b")], ..status("approved") };
        let v = milestone("m", &approved);
        assert_eq!(v, Verdict::of(Intent::NeedsDerivation, "a, b uncovered; bp walk m --for derive"));
        let full = Milestone { issues: vec!["open".into(), "in-progress".into(), "open".into()], ..approved.clone() };
        assert_eq!(of(&full), None, "a full batch waits for its issues");
        let covered = Milestone { covered: ["a".into(), "b".into()].into(), ..approved.clone() };
        assert_eq!(milestone("m", &covered), Verdict::of(Intent::NeedsAcceptance, "bp stamp m"), "no run yet");
        let one = Milestone { issues: vec!["resolved".into(), "open".into()], ..covered.clone() };
        assert_eq!(of(&one), None, "a live issue under it");
        assert_eq!(of(&Milestone { broken: true, ..covered.clone() }), None, "a broken file in the project");
        // A failed run keeps the intent, and the note names the waive (21, needs acceptance).
        let failed = Milestone { runs: vec![run("a", NOW, "c1", true)], ..covered.clone() };
        let waive = "a failed; bp stamp m --waive a --reason <why>";
        assert_eq!(milestone("m", &failed), Verdict::of(Intent::NeedsAcceptance, waive));
        let mut rerun = failed.clone();
        rerun.runs.push(run("a", NOW + 60, "c2", false));
        assert_eq!(milestone("m", &rerun), Verdict::of(Intent::NeedsAcceptance, "bp stamp m"), "a later pass");
        rerun.runs[1].check_hash = "#old".into();
        assert_eq!(milestone("m", &rerun).note, waive, "a pass of another text says nothing");

        let done = Milestone { reasked: vec!["f".into()], ..status("done") };
        assert_eq!(
            milestone("m", &done),
            Verdict::of(Intent::ReAsked, "f; bp stamp <next> --waive m/f --reason <why>")
        );
    }

    #[test]
    fn a_design_needs_shaping_while_a_concern_is_unproven_and_no_milestone_is_due() {
        let answer = |concern: &str, stamp: &str, state: &str, at: Option<i64>| ConcernAnswer {
            concern: concern.into(),
            stamp: stamp.into(),
            state: state.into(),
            decided_at: at,
        };
        let answers = [
            answer("a", "2", "designed", Some(NOW)),
            answer("a", "1", "open", Some(NOW - 60)),
            answer("b", "1", "open", Some(NOW)),
            answer("c", "1", "waived", Some(NOW)),
        ];
        let unproven = unproven_concerns(&answers, None);
        assert_eq!(unproven, ["a designed", "b open"], "no stamp proves the designed a; c is waived");
        let note = "a designed, b open; bp walk d --for shape";
        assert_eq!(design("d", &[], &unproven, false), Verdict::of(Intent::NeedsShaping, note));
        assert_eq!(design("d", &[], &unproven, true), Verdict::NONE, "a planned or approved milestone");
        assert_eq!(
            unproven_concerns(&answers, Some(NOW)),
            ["a designed", "b open"],
            "a stamp at the answer is no proof"
        );
        assert_eq!(unproven_concerns(&answers, Some(NOW + 1)), ["b open"], "a stamp after the answer proves it");
        let undated = [answer("a", "1", "designed", None)];
        assert_eq!(
            unproven_concerns(&undated, Some(NOW)),
            ["a designed"],
            "no stamp follows an answer with no instant"
        );
    }

    #[test]
    fn needs_response_follows_a_review_with_an_unanswered_finding() {
        let reviewed = Issue {
            status: "in-progress".into(),
            claims: vec![Claim::default()],
            review_heads: vec!["h".into()],
            pr: Some(pr("h", true, false)),
            unanswered: vec![2, 3],
            ..Issue::default()
        };
        let v = issue(&reviewed, NOW);
        assert_eq!(v, Verdict::of(Intent::NeedsResponse, "no disposition: findings 2, 3"));
        let red = Issue { pr: Some(pr("h", false, false)), ..reviewed.clone() };
        assert_eq!(issue(&red, NOW).intent, Some(Intent::Working), "working comes first in the order");
        let answered = Issue { unanswered: Vec::new(), ..reviewed };
        assert_eq!(issue(&answered, NOW).intent, Some(Intent::NeedsApproval));
    }
}
