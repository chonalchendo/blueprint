//! What needs review reads of an issue's reviews (37, design changes 1, 2, 6 and 8): the level in force at a head,
//! its round and the lenses it still lacks. The intents, `record review` and the review stop read these, so they
//! cannot disagree (21, rule 1).

use super::{Issue, recorded_risk};
use crate::risk::{self, Level};
use crate::rounds;

/// The level at `head` before the round caps: the highest of the declared level, the level `pr` recorded at the head
/// and each raise (37, design changes 1 and 2). A head that no `pr` run names reads the highest level recorded at an
/// earlier head, so a plain `git push` cannot lower the level or the lenses; none at all reads medium, never low. The
/// intents fetch nothing, so `bp risk` alone judges the diff afresh.
fn uncapped(i: &Issue, head: &str) -> Level {
    let raises = i.reviews.iter().filter(|r| r.raises()).count() + usize::from(i.delta);
    let computed = recorded_risk(&i.runs, head)
        .or_else(|| i.runs.iter().filter_map(|r| Some(r.risk?.level)).max())
        .unwrap_or(Level::Medium);
    risk::level(i.declared.unwrap_or(Level::Medium), Some(computed), raises)
}

/// A run at `head` records that the diff touches a high path outside the low patterns, so round one adds the
/// security lens (37, design change 8). A head that no `pr` run names reads each earlier head's record, as
/// [`uncapped`] does.
fn security(i: &Issue, head: &str) -> bool {
    let recorded = |r: &&super::Run| r.risk.is_some();
    let at = i.runs.iter().filter(recorded).filter(|r| r.commit == head).collect::<Vec<_>>();
    let runs = if at.is_empty() { i.runs.iter().filter(recorded).collect() } else { at };
    runs.iter().any(|r| r.risk.is_some_and(|x| x.security))
}

/// The level in force at `head` as the intents read it, and the round whose cap raised it last with the level whose
/// cap it passed: the [`uncapped`] level, then the round caps (37, design change 6).
pub fn level(i: &Issue, head: &str) -> (Level, Option<(u64, Level)>) {
    rounds::capped(uncapped(i, head), &i.reviews)
}

/// The round of `head`: [`rounds::round_of`] at the level and lenses the intents read. `record review` reads the same.
pub fn round(i: &Issue, head: &str) -> u64 {
    rounds::round_of(uncapped(i, head), security(i, head), &i.reviews, head)
}

/// The reviews needs review waits for at `head`: [`rounds::missing`] at the level and lenses the intents read. The
/// review stop and `record review` read the same (21, rule 1).
pub fn missing(i: &Issue, head: &str) -> Vec<rounds::Lens> {
    rounds::missing(uncapped(i, head), security(i, head), &i.reviews, head)
}
