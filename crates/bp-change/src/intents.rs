//! The `Intents` function (21): one intent per live node, the first true predicate in a fixed order. Blocked comes
//! first, because a blocked issue must never read as ready. Pure: the caller hands in the node's records, its
//! dependencies' states and the forge's view of its pull request.
//!
//! Predicates whose records do not exist yet (dispositions, drift runs, finish-line runs, concern answers,
//! waivers) never hold; they arrive with the milestones that write those records.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{IssueStatus, MilestoneStatus};

/// Twelve hours: a claim older than this with no run since reads `working (stale)`.
pub const STALE_AFTER: i64 = 12 * 3600;

/// The intents, in predicate order (21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

/// A claim record, reduced to what the predicates read.
#[derive(Debug, Clone, Default)]
pub struct Claim {
    /// `from`, Unix seconds.
    pub at: Option<i64>,
    /// The claim has a release.
    pub released: bool,
    /// The release reason was `escalated`.
    pub escalated: bool,
}

/// A run record, reduced.
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// `at`, Unix seconds.
    pub at: Option<i64>,
    /// The commit the run verified.
    pub commit: String,
    /// The verdict was `pass`.
    pub passed: bool,
}

/// The forge's view of the issue's open pull request.
#[derive(Debug, Clone, Default)]
pub struct PullRequest {
    /// The head commit.
    pub head: String,
    /// Every check at the head succeeded, and there is at least one.
    pub green: bool,
    /// An approving review exists.
    pub approved: bool,
}

/// A `depends` target.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// The target id.
    pub id: String,
    /// The target's status is `resolved`.
    pub resolved: bool,
    /// The target holds an open pull request.
    pub open_pr: bool,
    /// The target exists in the vault.
    pub exists: bool,
}

/// Everything the issue predicates read.
#[derive(Debug, Clone, Default)]
pub struct Issue {
    /// The header status.
    pub status: String,
    /// Claim records, in id order.
    pub claims: Vec<Claim>,
    /// Run records, in id order.
    pub runs: Vec<Run>,
    /// `head` of each review record, in id order.
    pub review_heads: Vec<String>,
    /// The `depends` targets.
    pub depends: Vec<Dependency>,
    /// The open pull request; `None` when there is none or the forge was not read.
    pub pr: Option<PullRequest>,
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

    const NONE: Self = Self { intent: None, note: String::new() };
}

/// A live issue: open or in progress.
pub fn is_live_issue(status: &str) -> bool {
    matches!(status.parse(), Ok(IssueStatus::Open | IssueStatus::InProgress))
}

/// The intent of one issue at `now` (Unix seconds).
pub fn issue(i: &Issue, now: i64) -> Verdict {
    let status = i.status.parse::<IssueStatus>().ok();
    if !matches!(status, Some(IssueStatus::Open | IssueStatus::InProgress)) {
        return Verdict::NONE;
    }
    // The latest claim by time; claims without a time sort first, as they predate the field.
    let claim = i.claims.iter().max_by_key(|c| c.at);
    let held = claim.filter(|c| !c.released);
    let pr_head = i.pr.as_ref().map(|p| p.head.as_str());
    let run = i.runs.iter().max_by_key(|r| r.at);
    if let (Some(last), Some(head)) = (run, pr_head)
        && last.commit == head
        && !last.passed
    {
        return Verdict::of(Intent::Blocked, "run failed at head");
    }
    if claim.is_some_and(|c| c.escalated) {
        return Verdict::of(Intent::Blocked, "escalated");
    }
    let open = status == Some(IssueStatus::Open);
    if open && held.is_none() {
        let waits: Vec<&str> = i.depends.iter().filter(|d| !d.resolved).map(|d| d.id.as_str()).collect();
        // A missing target cannot be waited on; integrity names it instead.
        let blocked_by = i.depends.iter().any(|d| d.exists && !d.resolved && !d.open_pr);
        return if blocked_by {
            Verdict::of(Intent::Waiting, format!("waits for {}", waits.join(", ")))
        } else {
            Verdict::of(Intent::Ready, "")
        };
    }
    let Some(pr) = &i.pr else {
        let Some(c) = held else { return Verdict::NONE };
        let stale =
            c.at.is_some_and(|at| now - at > STALE_AFTER && !i.runs.iter().any(|r| r.at.is_none_or(|t| t > at)));
        return Verdict::of(Intent::Working, if stale { "stale" } else { "" });
    };
    let reviewed = i.review_heads.last().is_some_and(|h| *h == pr.head);
    if !reviewed {
        return Verdict::of(Intent::NeedsReview, "");
    }
    if pr.green && !pr.approved {
        return Verdict::of(Intent::NeedsApproval, "");
    }
    if pr.green && pr.approved {
        return Verdict::of(Intent::NeedsLanding, "");
    }
    Verdict::NONE
}

/// The intent of one milestone. Derivation and acceptance need finish-line run records, which arrive later.
pub fn milestone(status: &str) -> Verdict {
    match status.parse() {
        Ok(MilestoneStatus::Planned) => Verdict::of(Intent::NeedsMilestoneApproval, ""),
        _ => Verdict::NONE,
    }
}

/// A claim's release: the `key: value` lines of its appended `## Released` section (13). Empty while it is held.
pub fn release(body: &str) -> BTreeMap<String, String> {
    let body = bp_vault::Body::parse(body);
    let lines = body.sections_named("Released").into_iter().flat_map(|s| s.text.lines());
    lines.filter_map(|l| l.split_once(": ")).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

/// `YYYY-MM-DDTHH:MM:SS` followed by `Z`, as Unix seconds. Every timestamp the vault writes is UTC.
pub fn timestamp(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let shape = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if bytes.len() < 20 || !text.ends_with('Z') || shape.iter().any(|&(at, sep)| bytes[at] != sep) {
        return None;
    }
    let field = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (field(0..4)?, field(5..7)?, field(8..10)?);
    // Days from the civil date (Howard Hinnant's algorithm), so no calendar crate is needed for one subtraction.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + field(11..13)? * 3600 + field(14..16)? * 60 + field(17..19)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_792_281_600; // 2026-10-18T00:00:00Z

    fn open() -> Issue {
        Issue { status: "open".into(), ..Issue::default() }
    }

    fn dep(id: &str, resolved: bool, open_pr: bool) -> Dependency {
        Dependency { id: id.into(), resolved, open_pr, exists: true }
    }

    fn pr(head: &str, green: bool, approved: bool) -> PullRequest {
        PullRequest { head: head.into(), green, approved }
    }

    #[test]
    fn a_release_is_the_appended_section() {
        let r = release("## Released\n\nto: 2026-10-13T10:09:12Z\nreason: landed\ncost_tokens: 9\n");
        assert_eq!(r["to"], "2026-10-13T10:09:12Z");
        assert_eq!(r["reason"], "landed");
        assert!(release("").is_empty(), "a held claim has no release");
    }

    #[test]
    fn parses_utc_timestamps() {
        assert_eq!(timestamp("2026-10-18T00:00:00Z"), Some(NOW));
        assert_eq!(timestamp("1970-01-01T00:00:01Z"), Some(1));
        assert_eq!(timestamp("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(timestamp("2026-10-18 00:00:00"), None);
    }

    #[test]
    fn ready_waiting_and_their_dependencies() {
        assert_eq!(issue(&open(), NOW).intent, Some(Intent::Ready));
        let i = Issue { depends: vec![dep("a", true, false), dep("b", false, true)], ..open() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::Ready), "an open pull request on the target is enough");
        let i = Issue { depends: vec![dep("a", false, false), dep("b", false, true)], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Waiting, "waits for a, b"));
        let missing = Dependency { exists: false, ..dep("gone", false, false) };
        assert_eq!(issue(&Issue { depends: vec![missing], ..open() }, NOW).intent, Some(Intent::Ready));
    }

    #[test]
    fn working_and_stale() {
        let claim = |at| Claim { at, ..Claim::default() };
        let i = Issue { status: "in-progress".into(), claims: vec![claim(Some(NOW - 3600))], ..Issue::default() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Working, ""));
        let mut i = Issue { claims: vec![claim(Some(NOW - 13 * 3600))], ..i };
        assert_eq!(issue(&i, NOW).note, "stale");
        i.runs.push(Run { at: Some(NOW - 3600), ..Run::default() });
        assert_eq!(issue(&i, NOW).note, "", "a run since the claim is progress");
        let i = Issue { claims: vec![claim(None)], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Working, ""), "a claim with no time is never stale");
    }

    #[test]
    fn the_review_path_in_order() {
        let held = Issue { status: "in-progress".into(), claims: vec![Claim::default()], ..Issue::default() };
        let i = Issue { pr: Some(pr("h", true, false)), ..held.clone() };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsReview));
        let i = Issue { review_heads: vec!["old".into(), "h".into()], ..i };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsApproval));
        let i = Issue { pr: Some(pr("h", true, true)), ..i };
        assert_eq!(issue(&i, NOW).intent, Some(Intent::NeedsLanding));
        let red = Issue { pr: Some(pr("h", false, true)), ..i.clone() };
        assert_eq!(issue(&red, NOW).intent, None);
        let failed = Issue { runs: vec![Run { at: Some(NOW), commit: "h".into(), passed: false }], ..i };
        assert_eq!(issue(&failed, NOW), Verdict::of(Intent::Blocked, "run failed at head"));
    }

    #[test]
    fn blocked_comes_before_ready() {
        let i = Issue { claims: vec![Claim { released: true, escalated: true, ..Claim::default() }], ..open() };
        assert_eq!(issue(&i, NOW), Verdict::of(Intent::Blocked, "escalated"));
    }

    #[test]
    fn only_live_nodes_have_an_intent() {
        assert_eq!(issue(&Issue { status: "resolved".into(), ..Issue::default() }, NOW), Verdict::NONE);
        assert!(is_live_issue("in-progress") && !is_live_issue("abandoned"));
        assert_eq!(milestone("planned").intent, Some(Intent::NeedsMilestoneApproval));
        assert_eq!(milestone("approved").intent, None);
        assert_eq!(milestone("in-progress").intent, None);
    }
}
