//! Review rounds and lenses (37, design changes 6, 7 and 8; 36, sections 2, 5, 7 and 8). Round one reads a head wide:
//! one review record per lens of the level. Each later round is one `verify` review of the fix. A `verify` round at or
//! past the cap whose reviews hold an error row that blocks raises the level one step. Pure: the reader hands in the
//! reviews as their first commit wrote them, so a later edit of a record changes nothing here (37, design change 2).

use crate::risk::Level;

bp_vault::closed_enum! {
    /// What one review attacks (36, section 7). A round-one dispatch reviews one lens; `verify` is a later round's.
    pub enum Lens {
        Conformance = "conformance",
        FailurePaths = "failure-paths",
        TestsAndChecks = "tests-and-checks",
        Security = "security",
        Verify = "verify",
    }
}

bp_vault::closed_enum! {
    /// How a review reads its head: round one is `full`, each later round `verify` (37, design change 6).
    pub enum Mode {
        Full = "full",
        Verify = "verify",
    }
}

/// The round cap of a level (36, section 2): low 1, medium 2, high 3.
pub const fn cap(level: Level) -> u64 {
    match level {
        Level::Low => 1,
        Level::Medium => 2,
        Level::High => 3,
    }
}

/// The lenses of round one at `level` (36, section 7): conformance; failure paths from medium; tests and checks at
/// high, and security at high when the diff touches a high path outside the low patterns, which `pr` records on its
/// runs as `security`.
pub fn lenses(level: Level, security: bool) -> Vec<Lens> {
    let mut out = vec![Lens::Conformance];
    if level >= Level::Medium {
        out.push(Lens::FailurePaths);
    }
    if level == Level::High {
        out.push(Lens::TestsAndChecks);
        if security {
            out.push(Lens::Security);
        }
    }
    out
}

/// A severity cell counts as an error unless it is `warn` or `risk`: a word the tool does not know blocks, as an
/// error does.
pub fn is_error(severity: &str) -> bool {
    severity != "warn" && severity != crate::risk::RISK_ROW
}

/// A disposition's answer to one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// `fixed`: the next head carries the fix.
    Fixed,
    /// `declined`, with a reason that names no other issue.
    Declined,
    /// `declined`, with a reason that names a follow-up issue (37, design change 7).
    FollowUp,
}

/// One row of a review's findings table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    /// The severity cell is neither `warn` nor `risk`: a word the tool does not know counts as an error.
    pub error: bool,
    /// A `risk` row: it raises the level to high and needs no disposition (37, design change 2).
    pub risk: bool,
    /// An error row of a `verify` review whose location the fix diff does not show; `record` writes the list.
    pub late: bool,
    /// The disposition that answers it; `None` while none does.
    pub answer: Option<Answer>,
}

impl Row {
    /// It blocks at `level`, whatever its answer (37, design change 7): an error row, unless it is late and the issue
    /// is low or medium. A late error of a high issue blocks as any error does.
    fn blocks(&self, level: Level) -> bool {
        self.error && !(self.late && level < Level::High)
    }
}

/// A review record, reduced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// The record's id.
    pub id: String,
    /// The head it read.
    pub head: String,
    /// Its round; a converted review with none reads 1.
    pub round: u64,
    /// Its lens; a converted review with none reads `conformance`.
    pub lens: Lens,
    /// The findings, in table order, counted from 1.
    pub rows: Vec<Row>,
}

impl Review {
    /// It holds a `risk` row, so it raises the issue to high.
    pub fn raises(&self) -> bool {
        self.rows.iter().any(|r| r.risk)
    }
}

/// The round of `head` (37, design change 6): the round of the reviews at it; else the round after the latest, or 1
/// when no head has a review yet. Round one is the first head with a review; each later head with one is the next.
pub fn round_of(reviews: &[Review], head: &str) -> u64 {
    if let Some(round) = reviews.iter().filter(|r| r.head == head).map(|r| r.round).max() {
        return round;
    }
    reviews.iter().map(|r| r.round).max().map_or(1, |r| r + 1)
}

/// The mode a review of round `round` reads in: `full` at round one, `verify` after it.
pub const fn mode(round: u64) -> Mode {
    if round > 1 { Mode::Verify } else { Mode::Full }
}

/// The head the last round before `round` reviewed: the base of the fix diff that a `verify` review reads.
pub fn prior_head(reviews: &[Review], round: u64) -> Option<&str> {
    reviews.iter().filter(|r| r.round < round).max_by_key(|r| r.round).map(|r| r.head.as_str())
}

/// The level after the round caps (37, design change 6), and the round that raised it last. From `base`, each
/// `verify` round at or past the cap of the level so far, whose reviews hold an error row that blocks, raises the
/// level one step. Round one never raises: a low issue fixes its round-one errors, and the next head needs one
/// `verify` round. High stays high past its cap: the human decides (34, design change 6).
pub fn capped(base: Level, reviews: &[Review]) -> (Level, Option<u64>) {
    let mut rounds: Vec<u64> = reviews.iter().map(|r| r.round).filter(|&r| r > 1).collect();
    rounds.sort_unstable();
    rounds.dedup();
    let (mut level, mut at) = (base, None);
    for round in rounds {
        let error = reviews.iter().filter(|r| r.round == round).flat_map(|r| &r.rows).any(|row| row.blocks(level));
        if error && round >= cap(level) {
            let up = match level {
                Level::Low => Level::Medium,
                Level::Medium | Level::High => Level::High,
            };
            if up != level {
                (level, at) = (up, Some(round));
            }
        }
    }
    (level, at)
}

/// The reviews needs review still waits for at `head` (37, design change 8): at a round-one head, each lens of
/// `level` with no review record there; at a later head, one `verify` review.
pub fn missing(level: Level, security: bool, reviews: &[Review], head: &str) -> Vec<Lens> {
    let want = if round_of(reviews, head) == 1 { lenses(level, security) } else { vec![Lens::Verify] };
    want.into_iter().filter(|l| !reviews.iter().any(|r| r.head == head && r.lens == *l)).collect()
}

/// The rows of each review at `head` that no disposition answers, counted from 1, less each `risk` row (21, needs
/// response). Reviews with every row answered are left out.
pub fn unanswered<'a>(reviews: &'a [Review], head: &str) -> Vec<(&'a Review, Vec<u64>)> {
    rows_at(reviews, head, |row| !row.risk && row.answer.is_none())
}

/// The error rows of each review at `head` that hold the land at `level` (37, design change 7; 36, section 1: no
/// error row): each error row, but a late one of a low or medium issue declined with a follow-up issue. A row marked
/// `fixed` still holds at this head: the fix is a new head, which a `verify` round reads.
pub fn blocking<'a>(level: Level, reviews: &'a [Review], head: &str) -> Vec<(&'a Review, Vec<u64>)> {
    let followed = |row: &Row| row.late && level < Level::High && row.answer == Some(Answer::FollowUp);
    rows_at(reviews, head, |row| row.error && !followed(row))
}

fn rows_at<'a>(reviews: &'a [Review], head: &str, keep: impl Fn(&Row) -> bool) -> Vec<(&'a Review, Vec<u64>)> {
    reviews
        .iter()
        .filter(|r| r.head == head)
        .map(|r| (r, (1..).zip(&r.rows).filter(|(_, row)| keep(row)).map(|(n, _)| n).collect::<Vec<u64>>()))
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

/// The rows of `by` in words, for a note: `findings 2, 3`, with each review's lens when more than one review holds
/// rows.
pub fn named(by: &[(&Review, Vec<u64>)]) -> String {
    let one = |rows: &[u64]| rows.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
    match by {
        [(_, rows)] => format!("findings {}", one(rows)),
        _ => by.iter().map(|(r, rows)| format!("{} findings {}", r.lens, one(rows))).collect::<Vec<_>>().join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(head: &str, round: u64, lens: Lens, rows: Vec<Row>) -> Review {
        Review { id: format!("r/{head}-{lens}"), head: head.into(), round, lens, rows }
    }

    fn error(late: bool, answer: Option<Answer>) -> Row {
        Row { error: true, risk: false, late, answer }
    }

    #[test]
    fn lenses_by_level() {
        use Lens::*;
        assert_eq!(lenses(Level::Low, true), [Conformance]);
        assert_eq!(lenses(Level::Medium, true), [Conformance, FailurePaths]);
        assert_eq!(lenses(Level::High, false), [Conformance, FailurePaths, TestsAndChecks]);
        assert_eq!(lenses(Level::High, true), [Conformance, FailurePaths, TestsAndChecks, Security]);
    }

    #[test]
    fn the_round_of_a_head_follows_the_heads_with_a_review() {
        let reviews = [
            review("a", 1, Lens::Conformance, vec![]),
            review("a", 1, Lens::FailurePaths, vec![]),
            review("b", 2, Lens::Verify, vec![]),
        ];
        assert_eq!(round_of(&[], "a"), 1, "the first head");
        assert_eq!((round_of(&reviews, "a"), round_of(&reviews, "b"), round_of(&reviews, "c")), (1, 2, 3));
        assert_eq!((mode(1), mode(2)), (Mode::Full, Mode::Verify));
        assert_eq!(
            (prior_head(&reviews, 1), prior_head(&reviews, 2), prior_head(&reviews, 3)),
            (None, Some("a"), Some("b"))
        );
    }

    #[test]
    fn round_caps_by_level() {
        assert_eq!((cap(Level::Low), cap(Level::Medium), cap(Level::High)), (1, 2, 3));
        let r1 = review("a", 1, Lens::Conformance, vec![error(false, Some(Answer::Fixed))]);
        let r2 = review("b", 2, Lens::Verify, vec![error(false, None)]);
        let r3 = review("c", 3, Lens::Verify, vec![error(false, None)]);
        let r4 = review("d", 4, Lens::Verify, vec![error(false, None)]);
        // Low: one round. Its round-one error is fixed, and the next head needs one verify round.
        assert_eq!(capped(Level::Low, std::slice::from_ref(&r1)), (Level::Low, None), "round one never raises");
        assert_eq!(missing(Level::Low, false, std::slice::from_ref(&r1), "b"), [Lens::Verify]);
        // An error in the verify round is past low's cap, so low reads medium; round 3 is past medium's.
        assert_eq!(capped(Level::Low, &[r1.clone(), r2.clone()]), (Level::Medium, Some(2)));
        assert_eq!(capped(Level::Low, &[r1.clone(), r2.clone(), r3.clone()]), (Level::High, Some(3)));
        // Medium holds through round one and reads high at its cap, round 2.
        assert_eq!(capped(Level::Medium, std::slice::from_ref(&r1)), (Level::Medium, None));
        assert_eq!(capped(Level::Medium, &[r1.clone(), r2.clone()]), (Level::High, Some(2)));
        // High holds through rounds 2 and 3 and stays high past its cap: the human decides.
        assert_eq!(capped(Level::High, &[r1.clone(), r2.clone(), r3.clone(), r4]), (Level::High, None));
        // A verify round with no error row raises nothing.
        let clean = review("b", 2, Lens::Verify, vec![Row { error: false, ..Row::default() }]);
        assert_eq!(capped(Level::Low, &[r1, clean]), (Level::Low, None));
    }

    #[test]
    fn medium_at_cap_becomes_high() {
        let r1 = review("a", 1, Lens::Conformance, vec![error(false, Some(Answer::Fixed))]);
        let fixed = review("a", 1, Lens::FailurePaths, vec![]);
        let inside = review("b", 2, Lens::Verify, vec![error(false, None)]);
        assert_eq!(capped(Level::Medium, &[r1.clone(), fixed.clone(), inside.clone()]), (Level::High, Some(2)));
        // A late error outside the fix does not block medium, so it raises nothing.
        let late = review("b", 2, Lens::Verify, vec![error(true, None)]);
        assert_eq!(capped(Level::Medium, &[r1.clone(), fixed.clone(), late]), (Level::Medium, None));
        // The disposition does not matter: the review at the cap held the error row.
        let declined = review("b", 2, Lens::Verify, vec![error(false, Some(Answer::Declined))]);
        assert_eq!(capped(Level::Medium, &[r1.clone(), fixed, declined]).0, Level::High);
        // Once high, the next head is round 3, high's cap, and its cap is the one that holds.
        let r3 = review("c", 3, Lens::Verify, vec![]);
        assert_eq!(capped(Level::Medium, &[r1, inside, r3]), (Level::High, Some(2)));
    }

    #[test]
    fn late_error_outside_fix_does_not_block_medium() {
        let r1 = review("a", 1, Lens::Conformance, vec![error(false, Some(Answer::Fixed))]);
        let late = |answer| review("b", 2, Lens::Verify, vec![error(true, answer)]);
        // Unanswered, it needs a response; declined with a follow-up issue, it holds nothing.
        let open = [r1.clone(), late(None)];
        assert_eq!(unanswered(&open, "b").len(), 1);
        let followed = [r1.clone(), late(Some(Answer::FollowUp))];
        assert!(unanswered(&followed, "b").is_empty());
        assert!(blocking(Level::Medium, &followed, "b").is_empty(), "medium");
        assert!(blocking(Level::Low, &followed, "b").is_empty(), "low");
        // A decline that names no follow-up issue, or a fix not yet pushed, still holds the land.
        assert_eq!(blocking(Level::Medium, &[r1.clone(), late(Some(Answer::Declined))], "b")[0].1, [1]);
        assert_eq!(blocking(Level::Medium, &[r1.clone(), late(Some(Answer::Fixed))], "b")[0].1, [1]);
        // High: a late error blocks as any error does.
        assert_eq!(blocking(Level::High, &followed, "b")[0].1, [1]);
    }

    #[test]
    fn late_error_inside_fix_blocks() {
        let r1 = review("a", 1, Lens::Conformance, vec![error(false, Some(Answer::Fixed))]);
        let inside = [r1, review("b", 2, Lens::Verify, vec![Row::default(), error(false, Some(Answer::FollowUp))])];
        for level in Level::ALL {
            assert_eq!(blocking(*level, &inside, "b")[0].1, [2], "{level}");
        }
        assert_eq!(named(&blocking(Level::Low, &inside, "b")), "findings 2");
    }

    #[test]
    fn warn_and_risk_rows_hold_nothing() {
        assert!(is_error("error") && is_error("major") && is_error(""), "a word the tool does not know is an error");
        assert!(!is_error("warn") && !is_error("risk"));
        let warn = Row::default();
        let risk = Row { risk: true, ..Row::default() };
        let r = [review("a", 1, Lens::Conformance, vec![warn.clone(), risk])];
        assert_eq!(unanswered(&r, "a")[0].1, [1], "a warn row needs a disposition; a risk row does not");
        assert!(blocking(Level::High, &r, "a").is_empty());
        assert!(r[0].raises());
    }
}
