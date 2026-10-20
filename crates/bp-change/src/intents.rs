//! The `Intents` function (21): one intent per live node, the first true predicate in a fixed order. Blocked comes
//! first, because a blocked issue must never read as ready. Pure: the caller hands in the node's records, its
//! dependencies' states and the forge's view of its pull request.
//!
//! Predicates whose records do not exist yet (dispositions, drift runs, finish-line runs, concern answers,
//! waivers) never hold; they arrive with the milestones that write those records.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{IssueStatus, MilestoneStatus};

/// Twelve hours: a claim older than this with no run since reads `working (stale)`.
pub const STALE_AFTER: i64 = 12 * 3600;

/// The intents, in predicate order (21). A variant arrives with the milestone that writes the records it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[allow(missing_docs)]
pub enum Intent {
    Blocked,
    Ready,
    Waiting,
    Working,
    NeedsReview,
    NeedsApproval,
    NeedsLanding,
    NeedsMilestoneApproval,
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
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// The check it ran: owner id plus name.
    pub check: String,
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

/// The forge's view of the issue's open pull request.
#[derive(Debug, Clone, Default)]
pub struct PullRequest {
    /// The head commit.
    pub head: String,
    /// Every check at the head succeeded, and there is at least one.
    pub green: bool,
    /// An approving review at the head exists.
    pub approved: bool,
}

/// A `depends` target.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// The target id.
    pub id: String,
    /// The target's status; `None` when the target is not in the vault.
    pub status: Option<String>,
    /// The target holds an open pull request.
    pub open_pr: bool,
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
    /// `head` of each review record.
    pub review_heads: Vec<String>,
    /// The `depends` targets.
    pub depends: Vec<Dependency>,
    /// The open pull request; `None` when there is none or the forge was not read.
    pub pr: Option<PullRequest>,
    /// The issue file's last commit, Unix seconds (13: processing time). Read only after an escalated release.
    pub changed: Option<i64>,
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
    if let Some(pr) = &i.pr {
        // The latest run of each `error` check at the head; `at` is to the second, so attempts order the reruns.
        let mut latest: BTreeMap<&str, &Run> = BTreeMap::new();
        for r in i.runs.iter().filter(|r| r.error && r.commit == pr.head) {
            let slot = latest.entry(r.check.as_str()).or_insert(r);
            if r.attempt > slot.attempt {
                *slot = r;
            }
        }
        if latest.values().any(|r| r.failed) {
            return Verdict::of(Intent::Blocked, "run failed at head");
        }
    }
    // Escalated holds until the issue changes after the release: the human's answer is a write to the issue.
    if let Some(c) = latest_claim(&i.claims).filter(|c| c.escalated)
        && !matches!((i.changed, c.released), (Some(changed), Some(to)) if changed > to)
    {
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
        let waits: Vec<&str> = i
            .depends
            .iter()
            .filter(|d| d.status.as_deref() != Some("resolved") && !d.open_pr)
            .map(|d| d.id.as_str())
            .collect();
        return if waits.is_empty() {
            Verdict::of(Intent::Ready, "")
        } else {
            Verdict::of(Intent::Waiting, format!("waits for {}", waits.join(", ")))
        };
    }
    let Some(pr) = &i.pr else {
        let Some(c) = held else { return Verdict::NONE };
        let stale =
            c.at.is_some_and(|at| now - at > STALE_AFTER && !i.runs.iter().any(|r| r.at.is_none_or(|t| t > at)));
        return Verdict::of(Intent::Working, if stale { "stale" } else { "" });
    };
    if !i.review_heads.contains(&pr.head) {
        return Verdict::of(Intent::NeedsReview, "");
    }
    if !pr.green {
        return Verdict::of(Intent::Working, "checks not green");
    }
    if !pr.approved {
        return Verdict::of(Intent::NeedsApproval, "");
    }
    Verdict::of(Intent::NeedsLanding, "")
}

/// The intent of one milestone; `integrity` is the blocked note of a failed integrity run that names it
/// ([`IntegrityRun::blocks`]). Blocked comes first, for a milestone not yet done or abandoned. Derivation and
/// acceptance need finish-line run records, which arrive later.
pub fn milestone(status: &str, integrity: Option<String>) -> Verdict {
    let status = status.parse();
    if matches!(status, Ok(MilestoneStatus::Planned | MilestoneStatus::Approved))
        && let Some(note) = integrity
    {
        return Verdict::of(Intent::Blocked, note);
    }
    match status {
        Ok(MilestoneStatus::Planned) => Verdict::of(Intent::NeedsMilestoneApproval, ""),
        _ => Verdict::NONE,
    }
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
        Dependency { id: id.into(), status: Some(status.into()), open_pr }
    }

    fn pr(head: &str, green: bool, approved: bool) -> PullRequest {
        PullRequest { head: head.into(), green, approved }
    }

    fn run(check: &str, at: i64, commit: &str, failed: bool) -> Run {
        Run { check: check.into(), at: Some(at), commit: commit.into(), attempt: 1, error: true, failed }
    }

    fn released(at: i64) -> Claim {
        Claim { at: Some(at), released: Some(at + 60), escalated: false }
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
        assert_eq!(milestone("planned", failed.blocks(a)), Verdict::of(Intent::Blocked, note));
        assert_eq!(milestone("approved", failed.blocks(a)).intent, Some(Intent::Blocked));
        assert_eq!(milestone("done", failed.blocks(a)), Verdict::NONE, "a done milestone has nothing left to do");
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
        let missing = Dependency { id: "gone".into(), status: None, open_pr: false };
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
    fn escalated_blocks_until_the_issue_changes() {
        let escalated = Claim { at: Some(NOW - 600), released: Some(NOW - 300), escalated: true };
        let i = Issue { claims: vec![escalated], changed: Some(NOW - 600), ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Blocked, "escalated"));
        let unknown = Issue { changed: None, ..i.clone() };
        assert_eq!(issue(&unknown, NOW).intent, Some(Intent::Blocked));
        let answered = Issue { changed: Some(NOW - 60), ..i };
        assert_eq!(issue(&answered, NOW).intent, Some(Intent::Ready));
    }

    #[test]
    fn milestones() {
        assert_eq!(milestone("planned", None).intent, Some(Intent::NeedsMilestoneApproval));
        assert_eq!(milestone("approved", None).intent, None);
        assert_eq!(milestone("in-progress", None).intent, None);
        assert!(is_live_issue("in-progress") && !is_live_issue("abandoned"));
    }
}
