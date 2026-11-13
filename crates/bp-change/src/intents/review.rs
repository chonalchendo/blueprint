//! What needs review reads of an issue's reviews (37, design changes 1, 2, 6 and 8): the level in force at a head,
//! its round and the lenses it still lacks. The intents, `record review`, the review stop and `land` read these, so
//! they cannot disagree (21, rule 1).

use super::{Issue, recorded_risk};
use crate::risk::{self, Level};
use crate::rounds;

/// The computed level that the intents read at `head`: the level `pr` recorded there. A head that no `pr` run names
/// reads the highest level recorded at an earlier head, so a plain `git push` cannot lower the level or the lenses;
/// none at all reads medium, never low. The intents fetch nothing, so `bp risk` and `land` judge the diff afresh.
pub fn recorded(i: &Issue, head: &str) -> Level {
    recorded_risk(&i.runs, head)
        .or_else(|| i.runs.iter().filter_map(|r| Some(r.risk?.level)).max())
        .unwrap_or(Level::Medium)
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
pub fn in_force(i: &Issue, head: &str, computed: Level) -> (Level, Option<(u64, Level)>) {
    let raises = i.reviews.iter().filter(|r| r.raises()).count() + usize::from(i.delta);
    let judged = risk::level(i.declared.unwrap_or(Level::Medium), Some(computed), raises);
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
