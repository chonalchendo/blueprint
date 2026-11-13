//! The landing gates (32, design change 5; 26, design change 4; 37, design change 3): every part of needs landing in
//! `land`'s refusal order, each in words, and the first that does not hold. [`landing`] is that first unmet gate alone,
//! so `land`, the needs-landing intent and the Change page read one function (21, rule 1).

use serde::Serialize;
use ts_rs::TS;

use super::{Base, Block, Issue, PullRequest, Run, latest_at, level, missing, round};
use crate::risk::Level;
use crate::rounds::{self, Lens, Review};

/// One gate's state. A word, never a colour alone (19, principle 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub enum GateState {
    /// It holds at the head.
    #[serde(rename = "pass")]
    Pass,
    /// It does not hold, and no wait makes it hold: a fix and a push do.
    #[serde(rename = "fail")]
    Fail,
    /// It does not hold yet: it waits on an act of another, on a run, or on a read the caller could not make.
    #[serde(rename = "pending")]
    Pending,
    /// `install` has not set it up (18, Change 1).
    #[serde(rename = "not configured")]
    NotConfigured,
    /// The caller holds no clone to read it from: `land` reads it, the page cannot.
    #[serde(rename = "not read")]
    NotRead,
}

/// One gate: its name, its state, and the head, run or branch it refers to, in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Gate {
    /// What it tests: `base`, `forge checks`, `run <block>`, `review`, `dispositions`, `error rows`, `signed approval`
    /// or `check set`.
    pub name: String,
    /// Whether it holds.
    pub state: GateState,
    /// What it read: the sha, the run, the rows.
    pub detail: String,
}

/// Every gate, in order, and the first that does not hold.
#[derive(Debug)]
pub struct Gates<'a> {
    /// The base, the forge checks, one gate per run `evidence` reads, the review gates, then the check set.
    pub gates: Vec<Gate>,
    /// The first part of needs landing that does not hold; `None` when the pull request may land. It is the first
    /// gate that is not `pass`, past a gate the caller could not read: that one withholds the command, not refuses.
    pub unmet: Option<Unmet<'a>>,
}

/// The first part of needs landing that does not hold, in `land`'s refusal order (26, design change 4).
#[derive(Debug, PartialEq, Eq)]
pub enum Unmet<'a> {
    /// The claim was cut from this dependency's head, which has not landed (`base_unmerged`).
    Unmerged(&'a str),
    /// The pull request does not target this branch, the claim's base (`base_mismatch`).
    Mismatch(&'a str),
    /// The forge check at the head is not green.
    Checks,
    /// The latest run of this `error` block at the head did not pass.
    Failed(&'a Run),
    /// This `error` block has no run at the head of its current text: none, or one made before the block changed.
    Evidence(&'a str),
    /// The head's round lacks these reviews (`review_missing`; 37, design changes 6 and 8).
    Lenses {
        /// The head's round.
        round: u64,
        /// The lenses with no review in it.
        missing: Vec<Lens>,
    },
    /// These rows have no disposition (`findings_open`; 21, needs response).
    Unanswered(Vec<(&'a Review, Vec<u64>)>),
    /// These error rows still hold the land (`findings_open`; 37, design change 6).
    Holding(Vec<(&'a Review, Vec<u64>)>),
    /// The level is high, and no approval record names the head (`approval_missing`; 37, design change 3). `land`
    /// also verifies the signature of one that does.
    Approval,
    /// The head names another check set than the one the pull request carries now (`checks_changed`; 27, design
    /// change 8): a block changed after the push the reviews read.
    ChecksChanged,
}

/// Needs landing (21; 37, design change 3): every gate of [`gates`] but the check set, in `land`'s order. `land`
/// refuses on the first part that does not hold, and the `Intents` function reads the same answer, so the two never
/// disagree (21, rule 1). The check set needs the clone, which `land` reads and passes to [`gates`] itself.
pub fn landing<'a>(i: &'a Issue, pr: &'a PullRequest) -> Result<(), Unmet<'a>> {
    gates(i, pr, None).unmet.map_or(Ok(()), Err)
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

/// The gates of the issue's pull request `pr`: its base, the forge checks, its runs and its `error` blocks now, its
/// reviews at the level in force, an approval record at the head for high, and whether the head names the check set
/// now (`named`; `None` when no clone was read). Pure: no forge approval counts (37, design change 3).
///
/// The run gates follow the order of [`evidence`]: each failed latest run by block name, a gone block's too, then
/// each block in body order. So the first gate that is not `pass` is the refusal `land` gives.
pub fn gates<'a>(i: &'a Issue, pr: &'a PullRequest, named: Option<bool>) -> Gates<'a> {
    let head = &pr.head[..pr.head.len().min(7)];
    let gate = |name: &str, state, detail: String| Gate { name: name.into(), state, detail };
    let mut out: Vec<(Gate, Option<Unmet>)> = Vec::new();
    out.push(match &i.base {
        Base::Unmerged(below) => {
            (gate("base", GateState::Pending, format!("lands after {below}")), Some(Unmet::Unmerged(below)))
        }
        Base::Branch(b) => match &pr.base {
            Some(into) if into != b => (
                gate("base", GateState::Fail, format!("targets {into}, not the claim's base {b}")),
                Some(Unmet::Mismatch(b)),
            ),
            Some(_) => (gate("base", GateState::Pass, format!("on {b}")), None),
            None => (gate("base", GateState::Pass, format!("the claim's base {b}; the snapshot names none")), None),
        },
        Base::Unknown => (
            gate("base", GateState::Pending, "the claim records no base; land tests origin's default branch".into()),
            None,
        ),
    });
    out.push(match (pr.green, pr.failed) {
        (true, _) => (gate("forge checks", GateState::Pass, format!("green at {head}")), None),
        (false, true) => {
            (gate("forge checks", GateState::Fail, format!("a check failed at {head}")), Some(Unmet::Checks))
        }
        (false, false) => (gate("forge checks", GateState::Pending, format!("pending at {head}")), Some(Unmet::Checks)),
    });
    let latest = latest_at(&i.runs, &pr.head);
    // A failed run of a block the issue no longer holds still refuses the land, so it shows.
    for r in latest.values().filter(|r| !r.passed) {
        let detail = if i.blocks.iter().any(|b| b.name == r.check) {
            format!("fail at {head}, attempt {}", r.attempt)
        } else {
            format!("fail at {head}; the block is gone")
        };
        out.push((gate(&format!("run {}", r.check), GateState::Fail, detail), Some(Unmet::Failed(r))));
    }
    for b in i.blocks.iter().filter(|b| latest.get(b.name.as_str()).is_none_or(|r| r.passed)) {
        let name = format!("run {}", b.name);
        out.push(match latest.get(b.name.as_str()) {
            None => (gate(&name, GateState::Pending, format!("no run at {head}")), Some(Unmet::Evidence(&b.name))),
            Some(r) if r.check_hash != b.hash => (
                gate(&name, GateState::Pending, format!("run at {head} is of an older text")),
                Some(Unmet::Evidence(&b.name)),
            ),
            Some(r) => (gate(&name, GateState::Pass, format!("pass at {head}, attempt {}", r.attempt)), None),
        });
    }
    out.extend(reviewed(i, pr, head));
    out.push(match named {
        Some(true) => (gate("check set", GateState::Pass, format!("{head} names the error blocks carried now")), None),
        Some(false) => (
            gate("check set", GateState::Fail, format!("an error block changed after {head} was pushed")),
            Some(Unmet::ChecksChanged),
        ),
        None => (
            gate("check set", GateState::NotRead, format!("no clone holds {head}, so its check set is not read")),
            None,
        ),
    });
    let unmet = out.iter_mut().find_map(|(_, u)| u.take());
    Gates { gates: out.into_iter().map(|(g, _)| g).collect(), unmet }
}

/// The review gates at the level in force (37, design changes 3 and 6): each lens of the head's round, a disposition
/// of each row, no error row that holds, and for high an approval record at the head. `head` is the short head.
fn reviewed<'a>(i: &'a Issue, pr: &PullRequest, head: &str) -> [(Gate, Option<Unmet<'a>>); 4] {
    let gate = |name: &str, state, detail: String| Gate { name: name.into(), state, detail };
    let level = level(i, &pr.head).0;
    let (round, missing) = (round(i, &pr.head), missing(i, &pr.head));
    let lenses = if missing.is_empty() {
        (gate("review", GateState::Pass, format!("round {round} at {head}: each review of {level} recorded")), None)
    } else {
        let names: Vec<&str> = missing.iter().map(|l| l.as_str()).collect();
        let detail = format!("round {round} at {head}: missing lenses: {}", names.join(", "));
        (gate("review", GateState::Pending, detail), Some(Unmet::Lenses { round, missing }))
    };
    let open = rounds::unanswered(&i.reviews);
    let answered = if open.is_empty() {
        (gate("dispositions", GateState::Pass, "each row answered".into()), None)
    } else {
        let detail = format!("no disposition: {}", rounds::named(&open, &pr.head));
        (gate("dispositions", GateState::Pending, detail), Some(Unmet::Unanswered(open)))
    };
    let (blocking, _) = super::holding(i, &pr.head);
    let rows = if blocking.is_empty() {
        (gate("error rows", GateState::Pass, "no error row holds".into()), None)
    } else {
        let detail = format!("{} hold the land", rounds::cited(&blocking));
        (gate("error rows", GateState::Fail, detail), Some(Unmet::Holding(blocking)))
    };
    let signed = match level {
        Level::High if i.approvals.contains(&pr.head) => {
            (gate("signed approval", GateState::Pass, format!("recorded at {head}; land verifies it")), None)
        }
        Level::High => {
            let detail = format!("high: the human runs bp approve {} at {head}", i.id);
            (gate("signed approval", GateState::Pending, detail), Some(Unmet::Approval))
        }
        _ => (gate("signed approval", GateState::Pass, format!("not needed at {level}")), None),
    };
    [lenses, answered, rows, signed]
}

#[cfg(test)]
mod tests {
    use super::super::evidence;
    use super::*;
    use crate::rounds::{Answer, Row};

    const H: &str = "abcdef0123";

    fn pr(green: bool, failed: bool) -> PullRequest {
        PullRequest { head: H.into(), base: Some("master".into()), green, failed }
    }

    fn run(check: &str, hash: &str, passed: bool) -> Run {
        Run {
            id: format!("{check}-1"),
            check: check.into(),
            check_hash: hash.into(),
            commit: H.into(),
            attempt: 1,
            error: true,
            failed: !passed,
            passed,
            ..Run::default()
        }
    }

    fn block(name: &str) -> Block {
        Block { name: name.into(), hash: format!("#{name}") }
    }

    fn review(id: &str, lens: Lens, rows: Vec<Row>) -> Review {
        Review { id: id.into(), head: H.into(), round: 1, lens, rows }
    }

    /// A medium issue on `master` with one block `a`, a passing run of it at the head, and a review of each medium
    /// lens there with no row.
    fn medium() -> Issue {
        Issue {
            id: "projects/bp/issues/a".into(),
            base: Base::Branch("master".into()),
            runs: vec![run("a", "#a", true)],
            blocks: vec![block("a")],
            reviews: vec![review("c", Lens::Conformance, vec![]), review("f", Lens::FailurePaths, vec![])],
            ..Issue::default()
        }
    }

    /// [`medium`], declared high, with the third lens read.
    fn high() -> Issue {
        let mut i = Issue { declared: Some(Level::High), ..medium() };
        i.reviews.push(review("t", Lens::TestsAndChecks, vec![]));
        i
    }

    fn error(answer: Option<Answer>) -> Row {
        Row { error: true, answer, at: "f.rs:1".into(), ..Row::default() }
    }

    /// The unmet gate as a word, so a table row need not borrow from the issue it builds.
    fn word(u: Option<&Unmet>) -> &'static str {
        match u {
            None => "none",
            Some(Unmet::Unmerged(_)) => "unmerged",
            Some(Unmet::Mismatch(_)) => "mismatch",
            Some(Unmet::Checks) => "checks",
            Some(Unmet::Failed(_)) => "failed",
            Some(Unmet::Evidence(_)) => "evidence",
            Some(Unmet::Lenses { .. }) => "lenses",
            Some(Unmet::Unanswered(_)) => "unanswered",
            Some(Unmet::Holding(_)) => "holding",
            Some(Unmet::Approval) => "approval",
            Some(Unmet::ChecksChanged) => "checks changed",
        }
    }

    /// One row of a gate table: why, the pull request, the issue, the check set read, the unmet as a word, the gates.
    type Case = (&'static str, PullRequest, Issue, Option<bool>, &'static str, Vec<(&'static str, GateState)>);

    const NAMES: [&str; 7] =
        ["base", "forge checks", "run a", "review", "dispositions", "error rows", "signed approval"];

    /// The gates in order with `s`, then the check set at `set`.
    fn states(s: [GateState; 7], set: GateState) -> Vec<(&'static str, GateState)> {
        NAMES.iter().copied().zip(s).chain([("check set", set)]).collect()
    }

    /// Every gate passes but the one at `at`.
    fn with(at: usize, state: GateState) -> [GateState; 7] {
        let mut s = [GateState::Pass; 7];
        s[at] = state;
        s
    }

    /// [`medium`] with one review, of `lens`.
    fn only(lens: Lens) -> Issue {
        Issue { reviews: vec![review("c", lens, vec![])], ..medium() }
    }

    /// [`medium`] with `row` on its first review.
    fn rows(row: Row) -> Issue {
        let mut i = medium();
        i.reviews[0].rows.push(row);
        i
    }

    /// Each case's gates and unmet, and `landing` as the first unmet with the check set unread.
    fn hold(table: &[Case]) {
        for (why, pr, issue, named, unmet, want) in table {
            let g = gates(issue, pr, *named);
            let got: Vec<(&str, GateState)> = g.gates.iter().map(|g| (g.name.as_str(), g.state)).collect();
            assert_eq!(&got, want, "{why}");
            assert_eq!(word(g.unmet.as_ref()), *unmet, "{why}");
            let without = gates(issue, pr, None).unmet;
            assert_eq!(landing(issue, pr).err(), without, "{why}: landing is the first unmet, the set unread");
        }
    }

    #[test]
    fn gates_hold_every_unmet_in_land_order_and_landing_is_the_first() {
        use GateState::{Fail, NotRead, Pass, Pending};
        let (ok, green) = ([Pass; 7], pr(true, false));
        hold(&[
            ("all hold", green.clone(), medium(), Some(true), "none", states(ok, Pass)),
            (
                "the layer below is open",
                green.clone(),
                Issue { base: Base::Unmerged("projects/bp/issues/b".into()), ..medium() },
                Some(true),
                "unmerged",
                states(with(0, Pending), Pass),
            ),
            (
                "another base",
                green.clone(),
                Issue { base: Base::Branch("bp/b".into()), ..medium() },
                Some(true),
                "mismatch",
                states(with(0, Fail), Pass),
            ),
            ("checks pending", pr(false, false), medium(), Some(true), "checks", states(with(1, Pending), Pass)),
            ("a check failed", pr(false, true), medium(), Some(true), "checks", states(with(1, Fail), Pass)),
            (
                "the run failed",
                green.clone(),
                Issue { runs: vec![run("a", "#a", false)], ..medium() },
                Some(true),
                "failed",
                states(with(2, Fail), Pass),
            ),
            (
                "no run",
                green.clone(),
                Issue { runs: vec![], ..medium() },
                Some(true),
                "evidence",
                states(with(2, Pending), Pass),
            ),
            (
                "a run of an older text",
                green.clone(),
                Issue { runs: vec![run("a", "#old", true)], ..medium() },
                Some(true),
                "evidence",
                states(with(2, Pending), Pass),
            ),
            // The check set is the last gate: a changed set refuses only when every other gate holds.
            ("the check set changed", green.clone(), medium(), Some(false), "checks changed", states(ok, Fail)),
            ("and after an open gate", green.clone(), high(), Some(false), "approval", states(with(6, Pending), Fail)),
            // Not read: no unmet, so the page withholds the command and `land`, which reads it, decides.
            ("the check set unread", green.clone(), medium(), None, "none", states(ok, NotRead)),
            // A claim that records no base: `land` tests origin's default branch, which this read cannot.
            (
                "no base recorded",
                green.clone(),
                Issue { base: Base::Unknown, ..medium() },
                Some(true),
                "none",
                states(with(0, Pending), Pass),
            ),
            (
                "the base comes first",
                pr(false, true),
                Issue {
                    base: Base::Unmerged("b".into()),
                    runs: vec![run("a", "#a", false)],
                    ..only(Lens::Conformance)
                },
                Some(true),
                "unmerged",
                states([Pending, Fail, Fail, Pending, Pass, Pass, Pass], Pass),
            ),
        ]);
    }

    #[test]
    fn the_review_gates_hold_by_level() {
        use GateState::{Fail, Pass, Pending};
        let (ok, green) = ([Pass; 7], pr(true, false));
        let late = Row { late: true, ..error(Some(Answer::FollowUp)) };
        let lowered = Issue { lowerings: vec![(H.into(), Level::Medium)], ..high() };
        hold(&[
            (
                "a lens unread",
                green.clone(),
                only(Lens::Conformance),
                Some(true),
                "lenses",
                states(with(3, Pending), Pass),
            ),
            (
                "an error row with no answer",
                green.clone(),
                rows(error(None)),
                Some(true),
                "unanswered",
                states(
                    {
                        let mut s = with(4, Pending);
                        s[5] = Fail;
                        s
                    },
                    Pass,
                ),
            ),
            (
                "a declined error row holds",
                green.clone(),
                rows(error(Some(Answer::Declined))),
                Some(true),
                "holding",
                states(with(5, Fail), Pass),
            ),
            (
                "a fixed row holds until a verify review",
                green.clone(),
                rows(error(Some(Answer::Fixed))),
                Some(true),
                "holding",
                states(with(5, Fail), Pass),
            ),
            ("a late row with a follow-up, at medium", green.clone(), rows(late), Some(true), "none", states(ok, Pass)),
            (
                "a warn row answered",
                green.clone(),
                rows(Row { answer: Some(Answer::Declined), ..Row::default() }),
                Some(true),
                "none",
                states(ok, Pass),
            ),
            ("high, no approval", green.clone(), high(), Some(true), "approval", states(with(6, Pending), Pass)),
            (
                "high, approved at the head",
                green.clone(),
                Issue { approvals: vec![H.into()], ..high() },
                Some(true),
                "none",
                states(ok, Pass),
            ),
            (
                "high, approved at another head",
                green.clone(),
                Issue { approvals: vec!["0123".into()], ..high() },
                Some(true),
                "approval",
                states(with(6, Pending), Pass),
            ),
            ("high, lowered at the head", green.clone(), lowered, Some(true), "none", states(ok, Pass)),
            (
                "high, lowered at another head",
                green.clone(),
                Issue { lowerings: vec![("0123".into(), Level::Low)], ..high() },
                Some(true),
                "approval",
                states(with(6, Pending), Pass),
            ),
        ]);
    }

    /// Two blocks in body order `b`, `a`: no run of `b`, a failed run of `a`. `evidence` refuses on the failed run
    /// first, so its gate comes first, and the first gate that is not `pass` is the refusal.
    #[test]
    fn the_first_run_gate_that_is_not_pass_is_the_one_evidence_refuses() {
        let i = Issue { blocks: vec![block("b"), block("a")], runs: vec![run("a", "#a", false)], ..medium() };
        let green = pr(true, false);
        let g = gates(&i, &green, Some(true));
        let names: Vec<(&str, GateState)> = g.gates[2..4].iter().map(|g| (g.name.as_str(), g.state)).collect();
        assert_eq!(names, [("run a", GateState::Fail), ("run b", GateState::Pending)]);
        assert_eq!(g.unmet, Some(Unmet::Failed(&i.runs[0])));
        assert_eq!(evidence(&i.runs, &green.head, &i.blocks).err(), g.unmet);
        let first = g.gates.iter().find(|g| g.state != GateState::Pass).map(|g| g.name.as_str());
        assert_eq!(first, Some("run a"));
    }

    /// A run of an old block hash is no evidence, a failed run of a gone block names it gone, and a lowering above
    /// the level in force does not raise it.
    #[test]
    fn a_stale_run_a_gone_block_and_a_higher_lowering() {
        let green = pr(true, false);
        let stale = Issue { runs: vec![run("a", "#old", true)], ..medium() };
        assert_eq!(evidence(&stale.runs, &green.head, &stale.blocks), Err(Unmet::Evidence("a")));
        let gone = Issue { runs: vec![run("z", "#z", false)], ..medium() };
        let g = gates(&gone, &green, Some(true));
        assert_eq!(g.gates[2].detail, "fail at abcdef0; the block is gone");
        let raised = Issue { lowerings: vec![(H.into(), Level::High)], ..medium() };
        assert_eq!(super::super::review::level(&raised, H).0, Level::Medium);
    }

    #[test]
    fn gates_name_the_head_the_rows_and_the_run() {
        let (green, i) = (pr(true, false), medium());
        let g = gates(&i, &green, Some(true));
        let words: Vec<(&str, &str)> = g.gates.iter().map(|g| (g.name.as_str(), g.detail.as_str())).collect();
        assert_eq!(
            words,
            [
                ("base", "on master"),
                ("forge checks", "green at abcdef0"),
                ("run a", "pass at abcdef0, attempt 1"),
                ("review", "round 1 at abcdef0: each review of medium recorded"),
                ("dispositions", "each row answered"),
                ("error rows", "no error row holds"),
                ("signed approval", "not needed at medium"),
                ("check set", "abcdef0 names the error blocks carried now"),
            ]
        );
        let mut open = high();
        open.reviews.truncate(1);
        open.reviews[0].rows.push(error(Some(Answer::Declined)));
        let g = gates(&open, &green, None);
        let detail = |name: &str| g.gates.iter().find(|g| g.name == name).unwrap().detail.clone();
        assert_eq!(detail("review"), "round 1 at abcdef0: missing lenses: failure-paths, tests-and-checks");
        assert_eq!(detail("error rows"), "c findings 1 hold the land");
        assert_eq!(detail("signed approval"), "high: the human runs bp approve projects/bp/issues/a at abcdef0");
        assert_eq!(detail("check set"), "no clone holds abcdef0, so its check set is not read");
        let unknown = Issue { base: Base::Unknown, ..medium() };
        assert_eq!(
            gates(&unknown, &green, None).gates[0].detail,
            "the claim records no base; land tests origin's default branch"
        );
        let approved = Issue { approvals: vec![H.into()], ..high() };
        let g = gates(&approved, &green, None);
        assert_eq!(g.gates[6].detail, "recorded at abcdef0; land verifies it");
    }
}
