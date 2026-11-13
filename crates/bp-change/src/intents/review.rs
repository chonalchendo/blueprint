//! What needs review reads of an issue's reviews (37, design changes 1, 2, 6 and 8): the level in force at a head,
//! its round and the lenses it still lacks. The intents, `record review`, the review stop and `land` read these, so
//! they cannot disagree (21, rule 1).

use super::{Issue, recorded_risk};
use crate::risk::{self, Level};
use crate::rounds;
use crate::rounds::Review;

/// The level `pr` recorded at `head`, as the intents read it. A head that no `pr` run names reads the highest level
/// recorded at an earlier head, so a plain `git push` cannot lower the level or the lenses; with a pull request open
/// and none at all, it reads medium, never low. A ready issue has no pull request and no head: the caller passes `""`,
/// so every recorded run counts as an earlier head's, and the label alone sets the floor. The intents fetch nothing,
/// so `bp risk` and `land` judge the diff afresh.
fn recorded(i: &Issue, head: &str) -> Option<Level> {
    recorded_risk(&i.runs, head)
        .or_else(|| i.runs.iter().filter_map(|r| Some(r.risk?.level)).max())
        .or(i.pr.as_ref().map(|_| Level::Medium))
}

/// A run at `head` records that the diff touches a high path outside the low patterns, so round one adds the
/// security lens (37, design change 8). A head that no `pr` run names reads each earlier head's record, as
/// [`recorded`] does.
pub fn security(i: &Issue, head: &str) -> bool {
    let recorded = |r: &&super::Run| r.risk.is_some();
    let at = i.runs.iter().filter(recorded).filter(|r| r.commit == head).collect::<Vec<_>>();
    let runs = if at.is_empty() { i.runs.iter().filter(recorded).collect() } else { at };
    runs.iter().any(|r| r.risk.is_some_and(|x| x.security))
}

/// The level in force at `head` for the `computed` level of its diff, and the round whose cap raised it last with the
/// level whose cap it passed. The highest of the declared level, `computed` and each raise (37, design changes 1 and
/// 2); then the round caps (design change 6); then the latest lowering of `head`, when it is lower. A lowering binds
/// its head: it lowers the label, the computed level, the raises and the caps there, and nothing at another head.
pub fn in_force(i: &Issue, head: &str, computed: Option<Level>) -> (Level, Option<(u64, Level)>) {
    let raises = i.reviews.iter().filter(|r| r.raises()).count() + usize::from(i.delta);
    let judged = risk::level(i.declared.unwrap_or(Level::Medium), computed, raises);
    let (level, at) = rounds::capped(judged, &i.reviews);
    match i.lowerings.iter().rev().find(|(h, _)| h == head) {
        Some(&(_, lower)) if lower < level => (lower, None),
        _ => (level, at),
    }
}

/// [`in_force`] at the [`recorded`] level: the level the intents read.
pub fn level(i: &Issue, head: &str) -> (Level, Option<(u64, Level)>) {
    in_force(i, head, recorded(i, head))
}

/// The round of `head`: [`rounds::round_of`] at the level and lenses the intents read. `record review` reads the same.
pub fn round(i: &Issue, head: &str) -> u64 {
    rounds::round_of(level(i, head).0, security(i, head), &i.reviews, head)
}

/// The reviews needs review waits for at `head`: [`rounds::missing`] at the level and lenses the intents read, after
/// the round caps, so a rise by a round cap adds the new level's lenses too (review of #98, row 36). The review stop
/// and `record review` read the same (21, rule 1).
pub fn missing(i: &Issue, head: &str) -> Vec<rounds::Lens> {
    rounds::missing(level(i, head).0, security(i, head), &i.reviews, head)
}

/// An approval record: the head it signs and the rows it accepts, each as a review's id and the row's number (37,
/// design change 6). The rows are named only when the human approved at high's round cap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Approval {
    /// The head the human approved.
    pub head: String,
    /// The rows the human took on at the cap.
    pub accepts: Vec<(String, u64)>,
}

/// `rows` less each row an approval of `head` accepts (37, design change 6). At high's cap the human's approval is the
/// disposition `accepted` of the rows it names, so neither the dispositions nor the land wait on them. A row no
/// approval of this head names, such as one a review adds after the approval, still needs its own.
pub fn unaccepted<'a>(
    rows: Vec<(&'a Review, Vec<u64>)>,
    approvals: &[Approval],
    head: &str,
) -> Vec<(&'a Review, Vec<u64>)> {
    let accepted = |r: &Review, n: u64| {
        approvals.iter().any(|a| a.head == head && a.accepts.iter().any(|(id, m)| *id == r.id && *m == n))
    };
    let left = |(r, ns): (&'a Review, Vec<u64>)| (r, ns.into_iter().filter(|n| !accepted(r, *n)).collect::<Vec<_>>());
    rows.into_iter().map(left).filter(|(_, ns)| !ns.is_empty()).collect()
}

/// The rows that still need a disposition at `head`: [`rounds::unanswered`] less the rows an approval accepts.
pub fn open<'a>(reviews: &'a [Review], approvals: &[Approval], head: &str) -> Vec<(&'a Review, Vec<u64>)> {
    unaccepted(rounds::unanswered(reviews), approvals, head)
}

/// The error rows that hold the land at `head` ([`rounds::blocking`] at the level in force, less the rows an approval
/// of `head` accepts), and whether one is past high's round cap. Past it the human decides (37, design change 6): the
/// approval record of the head names the open rows it accepts. `land` keeps only the approval that verifies.
pub fn holding<'a>(i: &'a Issue, head: &str) -> (Vec<(&'a Review, Vec<u64>)>, bool) {
    let level = level(i, head).0;
    let rows = unaccepted(rounds::blocking(level, &i.reviews), &i.approvals, head);
    let capped = level == Level::High && rows.iter().any(|(r, _)| r.round >= rounds::cap(Level::High));
    (rows, capped)
}

/// The rows the human's approval of `head` would accept now: every row that holds the land or needs a disposition,
/// when an error row is past high's round cap, else none (37, design change 6). `bp approve` names them in the record.
pub fn accepting(i: &Issue, head: &str) -> Vec<(String, u64)> {
    let (holding, capped) = holding(i, head);
    if !capped {
        return Vec::new();
    }
    let mut rows = open(&i.reviews, &i.approvals, head);
    rows.extend(holding);
    let mut named: Vec<(String, u64)> = rows.iter().flat_map(|(r, ns)| ns.iter().map(|n| (r.id.clone(), *n))).collect();
    named.sort();
    named.dedup();
    named
}
