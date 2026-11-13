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

/// The severity words `record review` accepts (15, record). A word outside them is refused at record; one already in
/// a record counts as an error ([`is_error`]).
pub const SEVERITIES: [&str; 3] = ["error", "warn", crate::risk::RISK_ROW];

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
    /// `declined`, with a reason that names no live follow-up issue.
    Declined,
    /// `declined`, with a reason that names a live follow-up issue under the same milestone (37, design change 7).
    FollowUp,
}

/// One row of a review's findings table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    /// The severity cell is neither `warn` nor `risk`: a word the tool does not know counts as an error.
    pub error: bool,
    /// A `risk` row: it raises the level to high and needs no disposition (37, design change 2).
    pub risk: bool,
    /// A new error row of a `verify` review at a path the fix diff does not show; `record` writes the list.
    pub late: bool,
    /// The disposition that answers it; `None` while none does.
    pub answer: Option<Answer>,
    /// The earlier row it re-reports, as the review id and the row counted from 1: its `of` cell.
    pub of: Option<(String, u64)>,
    /// Its location cell: `<path>:<line>` or a node id. A new error row at an earlier error row's location is a
    /// re-report, so it is never late (37, design change 7).
    pub at: String,
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

/// Round one is still open (37, design change 6): no review of a later round exists, and no head holds a review of
/// each lens of `level`. A later round, once recorded, keeps round one closed whatever the level reads after it.
pub fn round_one_open(level: Level, security: bool, reviews: &[Review]) -> bool {
    let complete = |head: &str| {
        lenses(level, security).iter().all(|l| reviews.iter().any(|r| r.round == 1 && r.head == head && r.lens == *l))
    };
    !reviews.iter().any(|r| r.round > 1) && !reviews.iter().any(|r| complete(&r.head))
}

/// The round of `head` (37, design change 6): the round of the reviews at it while that is the latest round; else round
/// one while it is open, so a new head before round one completes needs every lens again; else the round after the
/// latest. A head whose reviews are of an earlier round, such as a branch reset to it, is a new head: an error row
/// that a later round cleared at another head is read again (review of #98, row 30).
pub fn round_of(level: Level, security: bool, reviews: &[Review], head: &str) -> u64 {
    let latest = reviews.iter().map(|r| r.round).max();
    if let Some(round) = reviews.iter().filter(|r| r.head == head).map(|r| r.round).max().filter(|r| Some(*r) == latest)
    {
        return round;
    }
    if round_one_open(level, security, reviews) {
        return 1;
    }
    latest.map_or(1, |r| r + 1)
}

/// The mode a review of round `round` reads in: `full` at round one, `verify` after it.
pub const fn mode(round: u64) -> Mode {
    if round > 1 { Mode::Verify } else { Mode::Full }
}

/// The head the last round before `round` reviewed, by time: the base of the fix diff that a `verify` review reads.
pub fn prior_head(reviews: &[Review], round: u64) -> Option<&str> {
    let before = reviews.iter().filter(|r| r.round < round);
    before.max_by_key(|r| r.round).map(|r| r.head.as_str())
}

/// The level after the round caps (37, design change 6), and the round that raised it last with the level whose cap
/// that round passed. From `base`, each `verify` round at or past the cap of the level so far, whose reviews hold an
/// error row that blocks, raises the level one step. Round one never raises: a low issue fixes its round-one errors,
/// and the next head needs one `verify` round. High stays high past its cap: the intent reads blocked, and the human
/// decides.
pub fn capped(base: Level, reviews: &[Review]) -> (Level, Option<(u64, Level)>) {
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
                (level, at) = (up, Some((round, level)));
            }
        }
    }
    (level, at)
}

/// The reviews needs review still waits for at `head` (37, design change 8): in round one, each lens of `level` with
/// no review of the round at this head; in a later round, one `verify` review of the round at this head, and each lens
/// of `level` that no review of the issue has read. So a level rise after round one still adds its lenses (review of
/// #98, row 32).
pub fn missing(level: Level, security: bool, reviews: &[Review], head: &str) -> Vec<Lens> {
    let round = round_of(level, security, reviews, head);
    let here = |l: Lens| reviews.iter().any(|r| r.head == head && r.round == round && r.lens == l);
    let lenses = lenses(level, security);
    if round == 1 {
        return lenses.into_iter().filter(|l| !here(*l)).collect();
    }
    let verify = (!here(Lens::Verify)).then_some(Lens::Verify);
    verify.into_iter().chain(lenses.into_iter().filter(|l| !reviews.iter().any(|r| r.lens == *l))).collect()
}

/// Row `n` of `review` still holds (37, design change 6): no error row of a later review at another head re-reports
/// it, and no `verify` review of a later round exists. A moved head alone clears nothing; a later `verify` review that
/// does not re-report the row clears it, and one that does carries it on as its own row. A `warn` row, or a row of a
/// second lens at the same head, re-reports nothing, so it cannot clear an error row in place of the fix.
fn holds(reviews: &[Review], review: &Review, n: u64) -> bool {
    let reported = reviews
        .iter()
        .skip_while(|r| r.id != review.id)
        .filter(|r| r.head != review.head)
        .flat_map(|r| &r.rows)
        .any(|row| row.error && row.of.as_ref().is_some_and(|(id, m)| *id == review.id && *m == n));
    let verified = reviews.iter().any(|r| r.lens == Lens::Verify && r.round > review.round);
    !reported && !verified
}

/// The rows that need a disposition at `head`, counted from 1, less each `risk` row (21, needs response): each row of
/// a review at `head` with no answer, and each error row of an earlier head that still holds with no answer. Reviews
/// with every row answered are left out.
pub fn unanswered<'a>(reviews: &'a [Review], head: &str) -> Vec<(&'a Review, Vec<u64>)> {
    rows(reviews, |r, n, row| {
        !row.risk && row.answer.is_none() && (r.head == head || (row.error && holds(reviews, r, n)))
    })
}

/// The error rows that hold the land at `level` (37, design changes 6 and 7; 36, section 1: no error row): each error
/// row that still holds, at any head, but a late one of a low or medium issue declined with a live follow-up issue. A
/// row marked `fixed` still holds until a later `verify` review does not re-report it.
pub fn blocking(level: Level, reviews: &[Review]) -> Vec<(&Review, Vec<u64>)> {
    let followed = |row: &Row| row.late && level < Level::High && row.answer == Some(Answer::FollowUp);
    rows(reviews, |r, n, row| row.error && !followed(row) && holds(reviews, r, n))
}

fn rows(reviews: &[Review], keep: impl Fn(&Review, u64, &Row) -> bool) -> Vec<(&Review, Vec<u64>)> {
    reviews
        .iter()
        .map(|r| (r, (1..).zip(&r.rows).filter(|(n, row)| keep(r, *n, row)).map(|(n, _)| n).collect::<Vec<u64>>()))
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
}

/// The rows of `by` in words, for a note: `findings 2, 3`, with each review's lens when more than one review at
/// `head` holds rows, and the review's id for a review of another head.
pub fn named(by: &[(&Review, Vec<u64>)], head: &str) -> String {
    let one = |rows: &[u64]| rows.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
    match by {
        [(r, rows)] if r.head == head => format!("findings {}", one(rows)),
        _ => by
            .iter()
            .map(|(r, rows)| {
                let whose = if r.head == head { r.lens.to_string() } else { r.id.clone() };
                format!("{whose} findings {}", one(rows))
            })
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// The rows of `by` with their review, for the note of an error row that holds the land: `<review> findings 1, 3`.
pub fn cited(by: &[(&Review, Vec<u64>)]) -> String {
    let one = |rows: &[u64]| rows.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
    by.iter().map(|(r, rows)| format!("{} findings {}", r.id, one(rows))).collect::<Vec<_>>().join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(head: &str, round: u64, lens: Lens, rows: Vec<Row>) -> Review {
        Review { id: format!("r/{head}-{lens}"), head: head.into(), round, lens, rows }
    }

    fn error(late: bool, answer: Option<Answer>) -> Row {
        Row { error: true, late, answer, ..Row::default() }
    }

    /// An error row of a later review that re-reports row `n` of `of`.
    fn again(of: &Review, n: u64) -> Row {
        Row { of: Some((of.id.clone(), n)), ..error(false, None) }
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
        let m = Level::Medium;
        let reviews = [
            review("a", 1, Lens::Conformance, vec![]),
            review("a", 1, Lens::FailurePaths, vec![]),
            review("b", 2, Lens::Verify, vec![]),
        ];
        assert_eq!(round_of(m, false, &[], "a"), 1, "the first head");
        let of = |head| round_of(m, false, &reviews, head);
        assert_eq!((of("b"), of("c")), (2, 3));
        // A head whose reviews are of an earlier round is a new head (review of #98, row 30): a branch reset to `a`
        // needs one `verify` review, and the round-one lenses there do not stand in for it.
        assert_eq!(of("a"), 3);
        assert_eq!(missing(m, false, &reviews, "a"), [Lens::Verify]);
        assert!(missing(m, false, &reviews, "b").is_empty());
        assert_eq!((mode(1), mode(2)), (Mode::Full, Mode::Verify));
        assert_eq!(
            (prior_head(&reviews, 1), prior_head(&reviews, 2), prior_head(&reviews, 3)),
            (None, Some("a"), Some("b"))
        );
    }

    /// A branch reset to a head whose `verify` review is of an earlier round needs a `verify` review of the head's
    /// own round: the old one counts for its round only (review of #98, row 39).
    #[test]
    fn a_reset_head_needs_a_verify_review_of_its_own_round() {
        let reviews = [
            review("a", 1, Lens::Conformance, vec![]),
            review("a", 1, Lens::FailurePaths, vec![]),
            review("b", 2, Lens::Verify, vec![]),
            review("c", 3, Lens::Verify, vec![]),
        ];
        assert_eq!(round_of(Level::Medium, false, &reviews, "b"), 4);
        assert_eq!(missing(Level::Medium, false, &reviews, "b"), [Lens::Verify]);
        assert!(missing(Level::Medium, false, &reviews, "c").is_empty(), "the latest round's own head");
    }

    #[test]
    fn round_one_holds_until_each_lens_is_recorded_at_one_head() {
        let m = Level::Medium;
        let one = [review("a", 1, Lens::Conformance, vec![error(false, None)])];
        // A new head before round one completes is still round one, and needs every lens again.
        assert!(round_one_open(m, false, &one));
        assert_eq!(round_of(m, false, &one, "b"), 1);
        assert_eq!(missing(m, false, &one, "b"), [Lens::Conformance, Lens::FailurePaths]);
        // The lenses split over two heads do not complete it.
        let split = [one[0].clone(), review("b", 1, Lens::FailurePaths, vec![])];
        assert_eq!(round_of(m, false, &split, "c"), 1);
        // Each lens at one head does; the next head is round two. Low needs conformance alone.
        let whole = [split[0].clone(), split[1].clone(), review("b", 1, Lens::Conformance, vec![])];
        assert_eq!((round_of(m, false, &whole, "c"), round_of(Level::Low, false, &one, "b")), (2, 2));
        // A level that rises after round two exists leaves round one closed.
        let two = [whole[0].clone(), whole[1].clone(), whole[2].clone(), review("c", 2, Lens::Verify, vec![])];
        assert_eq!(round_of(Level::High, true, &two, "d"), 3);
        assert_eq!(round_of(Level::High, true, &whole, "c"), 1, "high asks for its lenses until round two exists");
    }

    #[test]
    fn a_level_rise_after_round_one_adds_its_missing_lenses() {
        use Lens::*;
        let one = [review("a", 1, Conformance, vec![]), review("a", 1, FailurePaths, vec![])];
        // Before round two, a rise reopens round one with each lens of the new level.
        assert_eq!(missing(Level::High, true, &one, "b"), [Conformance, FailurePaths, TestsAndChecks, Security]);
        assert_eq!(missing(Level::Medium, true, &one, "b"), [Verify]);
        // After a verify review at b, round two exists, and round one stays closed. A rise read at b, or at a later
        // head, still asks for each high lens no review has read, beside the verify review (review of #98, row 32).
        let two = [one[0].clone(), one[1].clone(), review("b", 2, Verify, vec![])];
        assert_eq!(missing(Level::High, true, &two, "b"), [TestsAndChecks, Security]);
        assert_eq!(missing(Level::High, true, &two, "c"), [Verify, TestsAndChecks, Security]);
        // A lens review at any head of the issue reads that lens; the next head needs its verify review alone.
        let three = [two[0].clone(), two[1].clone(), two[2].clone(), review("b", 2, TestsAndChecks, vec![])];
        let four = [three.to_vec(), vec![review("b", 2, Security, vec![])]].concat();
        assert_eq!(missing(Level::High, true, &three, "b"), [Security]);
        assert!(missing(Level::High, true, &four, "b").is_empty());
        assert_eq!(missing(Level::High, true, &four, "c"), [Verify]);
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
        // An error in the verify round is past low's cap, so low reads medium; round 3 is past medium's. Each raise
        // names the level whose cap it passed.
        assert_eq!(capped(Level::Low, &[r1.clone(), r2.clone()]), (Level::Medium, Some((2, Level::Low))));
        assert_eq!(capped(Level::Low, &[r1.clone(), r2.clone(), r3.clone()]), (Level::High, Some((3, Level::Medium))));
        // Medium holds through round one and reads high at its cap, round 2.
        assert_eq!(capped(Level::Medium, std::slice::from_ref(&r1)), (Level::Medium, None));
        assert_eq!(capped(Level::Medium, &[r1.clone(), r2.clone()]), (Level::High, Some((2, Level::Medium))));
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
        let raised = Some((2, Level::Medium));
        assert_eq!(capped(Level::Medium, &[r1.clone(), fixed.clone(), inside.clone()]), (Level::High, raised));
        // A late error outside the fix does not block medium, so it raises nothing.
        let late = review("b", 2, Lens::Verify, vec![error(true, None)]);
        assert_eq!(capped(Level::Medium, &[r1.clone(), fixed.clone(), late]), (Level::Medium, None));
        // The disposition does not matter: the review at the cap held the error row.
        let declined = review("b", 2, Lens::Verify, vec![error(false, Some(Answer::Declined))]);
        assert_eq!(capped(Level::Medium, &[r1.clone(), fixed, declined]).0, Level::High);
        // Once high, the next head is round 3, high's cap, and its cap is the one that holds.
        let r3 = review("c", 3, Lens::Verify, vec![]);
        assert_eq!(capped(Level::Medium, &[r1, inside, r3]), (Level::High, raised));
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
        assert!(blocking(Level::Medium, &followed).is_empty(), "medium");
        assert!(blocking(Level::Low, &followed).is_empty(), "low");
        // A decline that names no follow-up issue, or a fix not yet pushed, still holds the land.
        assert_eq!(blocking(Level::Medium, &[r1.clone(), late(Some(Answer::Declined))])[0].1, [1]);
        assert_eq!(blocking(Level::Medium, &[r1.clone(), late(Some(Answer::Fixed))])[0].1, [1]);
        // High: a late error blocks as any error does.
        assert_eq!(blocking(Level::High, &followed)[0].1, [1]);
    }

    #[test]
    fn late_error_inside_fix_blocks() {
        let r1 = review("a", 1, Lens::Conformance, vec![error(false, Some(Answer::Fixed))]);
        let inside = [r1, review("b", 2, Lens::Verify, vec![Row::default(), error(false, Some(Answer::FollowUp))])];
        for level in Level::ALL {
            assert_eq!(blocking(*level, &inside)[0].1, [2], "{level}");
        }
        assert_eq!(named(&blocking(Level::Low, &inside), "b"), "findings 2");
        assert_eq!(cited(&blocking(Level::Low, &inside)), "r/b-verify findings 2");
    }

    #[test]
    fn an_error_row_holds_until_a_later_verify_review_does_not_re_report_it() {
        let r1 = review("a", 1, Lens::Conformance, vec![error(false, Some(Answer::Declined)), error(false, None)]);
        let paths = review("a", 1, Lens::FailurePaths, vec![]);
        let round_one = [r1.clone(), paths.clone()];
        assert_eq!(blocking(Level::Medium, &round_one)[0].1, [1, 2]);
        // A moved head alone clears nothing: the declined row still blocks, and the unanswered one needs an answer.
        assert_eq!(blocking(Level::Low, &round_one)[0].1, [1, 2], "at a new head too");
        assert_eq!(unanswered(&round_one, "b")[0].1, [2]);
        assert_eq!(named(&unanswered(&round_one, "b"), "b"), "r/a-conformance findings 2");
        // A row of a second lens at the same head re-reports nothing: rows 1 and 2 both hold (review of #98, row 20).
        let echo = review("a", 1, Lens::FailurePaths, vec![again(&r1, 2)]);
        let echoed = [r1.clone(), echo];
        let held: Vec<_> = blocking(Level::Medium, &echoed).into_iter().map(|(r, n)| (&*r.id, n)).collect();
        assert_eq!(held, [("r/a-conformance", vec![1, 2]), ("r/a-failure-paths", vec![1])]);
        // Nor does a `warn` row at a later head: only an error row carries an earlier one.
        let warned =
            [r1.clone(), paths.clone(), review("b", 1, Lens::Conformance, vec![Row { error: false, ..again(&r1, 2) }])];
        assert_eq!(blocking(Level::Medium, &warned)[0].1, [1, 2]);
        // An error row at a later head carries row 2 as its own row; row 1 still holds.
        let carried = [r1.clone(), paths.clone(), review("b", 1, Lens::Conformance, vec![again(&r1, 2)])];
        let held: Vec<_> = blocking(Level::Medium, &carried).into_iter().map(|(r, n)| (&*r.id, n)).collect();
        assert_eq!(held, [("r/a-conformance", vec![1]), ("r/b-conformance", vec![1])]);
        // A verify review that re-reports row 1 carries it as its own row; row 2 clears.
        let v2 = review("b", 2, Lens::Verify, vec![again(&r1, 1)]);
        let two = [r1.clone(), paths.clone(), v2.clone()];
        let held = blocking(Level::Medium, &two);
        assert_eq!((held.len(), held[0].0.id.as_str(), &held[0].1), (1, "r/b-verify", &vec![1]));
        assert_eq!(unanswered(&two, "b")[0].1, [1]);
        // The next verify review that does not re-report it clears it.
        let three = [r1, paths, v2, review("c", 3, Lens::Verify, vec![])];
        assert!(blocking(Level::High, &three).is_empty());
        assert!(unanswered(&three, "c").is_empty());
    }

    #[test]
    fn warn_and_risk_rows_hold_nothing() {
        assert!(is_error("error") && is_error("major") && is_error(""), "a word the tool does not know is an error");
        assert!(!is_error("warn") && !is_error("risk"));
        assert_eq!(SEVERITIES, ["error", "warn", "risk"]);
        let warn = Row::default();
        let risk = Row { risk: true, ..Row::default() };
        let r = [review("a", 1, Lens::Conformance, vec![warn.clone(), risk])];
        assert_eq!(unanswered(&r, "a")[0].1, [1], "a warn row needs a disposition; a risk row does not");
        assert!(unanswered(&r, "b").is_empty(), "nor does a warn row of another head");
        assert!(blocking(Level::High, &r).is_empty());
        assert!(r[0].raises());
    }
}
