//! The landing gates (32, design change 5; 26, design change 4): every part of needs landing in `land`'s refusal order,
//! each in words, and the first that does not hold. [`landing`](super::landing) is that first unmet gate alone, so
//! `land`, the needs-landing intent and the Change page read one function (21, rule 1).

use serde::Serialize;
use ts_rs::TS;

use super::{Base, Block, PullRequest, Run, Unmet, latest_at};

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
    /// What it tests: `base`, `approving review`, `forge checks`, `run <block>`, or `check set`.
    pub name: String,
    /// Whether it holds.
    pub state: GateState,
    /// What it read: the sha, the reviewer, the run.
    pub detail: String,
}

/// Every gate, in order, and the first that does not hold.
#[derive(Debug)]
pub struct Gates<'a> {
    /// The base, the approving review, the forge checks, one gate per run `evidence` reads, then the check set.
    pub gates: Vec<Gate>,
    /// The first part of needs landing that does not hold; `None` when the pull request may land. It is the first
    /// gate that is not `pass`, past a gate the caller could not read: that one withholds the command, not refuses.
    pub unmet: Option<Unmet<'a>>,
}

/// The gates of the pull request `pr` on the claim's `base`, with the issue's runs, its `error` blocks now, and
/// whether the head names the check set now (`named`; `None` when no clone was read). Pure.
///
/// The run gates follow the order of [`evidence`](super::evidence): each failed latest run by block name, a gone
/// block's too, then each block in body order. So the first gate that is not `pass` is the refusal `land` gives.
pub fn gates<'a>(
    pr: &'a PullRequest,
    base: &'a Base,
    runs: &'a [Run],
    blocks: &'a [Block],
    named: Option<bool>,
) -> Gates<'a> {
    let head = &pr.head[..pr.head.len().min(7)];
    let gate = |name: &str, state, detail: String| Gate { name: name.into(), state, detail };
    let mut out: Vec<(Gate, Option<Unmet>)> = Vec::new();
    out.push(match base {
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
    let review = match (pr.reviewer.as_str(), pr.approved) {
        ("", _) => gate(
            "approving review",
            GateState::NotConfigured,
            "set the project's reviewer; land refuses reviewer_invalid".into(),
        ),
        (who, true) => gate("approving review", GateState::Pass, format!("approved at {head} by {who}")),
        (who, false) => gate("approving review", GateState::Pending, format!("no approval by {who} at {head}")),
    };
    out.push((review, (!pr.approved).then_some(Unmet::Review)));
    out.push(match (pr.green, pr.failed) {
        (true, _) => (gate("forge checks", GateState::Pass, format!("green at {head}")), None),
        (false, true) => {
            (gate("forge checks", GateState::Fail, format!("a check failed at {head}")), Some(Unmet::Checks))
        }
        (false, false) => (gate("forge checks", GateState::Pending, format!("pending at {head}")), Some(Unmet::Checks)),
    });
    let latest = latest_at(runs, &pr.head);
    // A failed run of a block the issue no longer holds still refuses the land, so it shows.
    for r in latest.values().filter(|r| !r.passed) {
        let detail = if blocks.iter().any(|b| b.name == r.check) {
            format!("fail at {head}, attempt {}", r.attempt)
        } else {
            format!("fail at {head}; the block is gone")
        };
        out.push((gate(&format!("run {}", r.check), GateState::Fail, detail), Some(Unmet::Failed(r))));
    }
    for b in blocks.iter().filter(|b| latest.get(b.name.as_str()).is_none_or(|r| r.passed)) {
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

#[cfg(test)]
mod tests {
    use super::super::{evidence, landing};
    use super::*;

    fn pr(green: bool, failed: bool, approved: bool) -> PullRequest {
        PullRequest {
            head: "abcdef0123".into(),
            base: Some("master".into()),
            green,
            failed,
            approved,
            reviewer: "quokka-reviewer".into(),
        }
    }

    fn run(check: &str, hash: &str, passed: bool) -> Run {
        Run {
            id: format!("{check}-1"),
            check: check.into(),
            check_hash: hash.into(),
            commit: "abcdef0123".into(),
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

    /// A table row: the pull request, the base, the runs, whether the head names the check set, the unmet gate, and
    /// each gate's name and state.
    type Row<'a> = (PullRequest, &'a Base, &'a [Run], Option<bool>, Option<Unmet<'a>>, Vec<(&'a str, GateState)>);

    fn states(g: &Gates) -> Vec<(String, GateState)> {
        g.gates.iter().map(|g| (g.name.clone(), g.state)).collect()
    }

    #[test]
    fn gates_hold_every_unmet_in_land_order_and_landing_is_the_first() {
        use GateState::{Fail, NotConfigured, NotRead, Pass, Pending};
        let master = Base::Branch("master".into());
        let below = Base::Unmerged("projects/bp/issues/a".into());
        let stacked = Base::Branch("bp/a".into());
        let blocks = [block("a")];
        let pass = [run("a", "#a", true)];
        let fail = [run("a", "#a", false)];
        let old = [run("a", "#old", true)];
        let gone = [run("a", "#a", true), run("z", "#z", false)];
        let green = pr(true, false, true);
        let unreviewed = PullRequest { reviewer: String::new(), ..pr(true, false, false) };
        let gates4 = |b, r, c, a| vec![("base", b), ("approving review", r), ("forge checks", c), ("run a", a)];
        let with = |mut g: Vec<(&'static str, GateState)>, s| {
            g.push(("check set", s));
            g
        };
        let ok = |b, r, c, a| with(gates4(b, r, c, a), Pass);
        let table: Vec<Row> = vec![
            (green.clone(), &master, &pass, Some(true), None, ok(Pass, Pass, Pass, Pass)),
            (
                green.clone(),
                &below,
                &pass,
                Some(true),
                Some(Unmet::Unmerged("projects/bp/issues/a")),
                ok(Pending, Pass, Pass, Pass),
            ),
            (green.clone(), &stacked, &pass, Some(true), Some(Unmet::Mismatch("bp/a")), ok(Fail, Pass, Pass, Pass)),
            (pr(true, false, false), &master, &pass, Some(true), Some(Unmet::Review), ok(Pass, Pending, Pass, Pass)),
            (unreviewed, &master, &pass, Some(true), Some(Unmet::Review), ok(Pass, NotConfigured, Pass, Pass)),
            (pr(false, false, true), &master, &pass, Some(true), Some(Unmet::Checks), ok(Pass, Pass, Pending, Pass)),
            (pr(false, true, true), &master, &pass, Some(true), Some(Unmet::Checks), ok(Pass, Pass, Fail, Pass)),
            (green.clone(), &master, &fail, Some(true), Some(Unmet::Failed(&fail[0])), ok(Pass, Pass, Pass, Fail)),
            (green.clone(), &master, &[], Some(true), Some(Unmet::Evidence("a")), ok(Pass, Pass, Pass, Pending)),
            (green.clone(), &master, &old, Some(true), Some(Unmet::Evidence("a")), ok(Pass, Pass, Pass, Pending)),
            (
                green.clone(),
                &master,
                &gone,
                Some(true),
                Some(Unmet::Failed(&gone[1])),
                vec![
                    ("base", Pass),
                    ("approving review", Pass),
                    ("forge checks", Pass),
                    ("run z", Fail),
                    ("run a", Pass),
                    ("check set", Pass),
                ],
            ),
            (
                pr(false, true, false),
                &below,
                &fail,
                Some(true),
                Some(Unmet::Unmerged("projects/bp/issues/a")),
                ok(Pending, Pending, Fail, Fail),
            ),
            // The check set is the last gate: a changed set refuses only when every other gate holds.
            (
                green.clone(),
                &master,
                &pass,
                Some(false),
                Some(Unmet::ChecksChanged),
                with(gates4(Pass, Pass, Pass, Pass), Fail),
            ),
            (
                pr(true, false, false),
                &master,
                &pass,
                Some(false),
                Some(Unmet::Review),
                with(gates4(Pass, Pending, Pass, Pass), Fail),
            ),
            // Not read: no unmet, so the page withholds the command and `land`, which reads it, decides.
            (green.clone(), &master, &pass, None, None, with(gates4(Pass, Pass, Pass, Pass), NotRead)),
            // A claim that records no base: `land` tests origin's default branch, which this read cannot.
            (green.clone(), &Base::Unknown, &pass, Some(true), None, ok(Pending, Pass, Pass, Pass)),
        ];
        for (pr, base, runs, named, unmet, want) in &table {
            let g = gates(pr, base, runs, &blocks, *named);
            let got: Vec<(&str, GateState)> = g.gates.iter().map(|g| (g.name.as_str(), g.state)).collect();
            assert_eq!(&got, want, "{:?}", states(&g));
            assert_eq!(g.unmet.as_ref(), unmet.as_ref());
            let without = gates(pr, base, runs, &blocks, None).unmet;
            assert_eq!(landing(pr, base, runs, &blocks).err(), without, "landing is the first unmet, the set unread");
        }
    }

    /// Two blocks in body order `b`, `a`: no run of `b`, a failed run of `a`. `evidence` refuses on the failed run
    /// first, so its gate comes first, and the first gate that is not `pass` is the refusal.
    #[test]
    fn the_first_run_gate_that_is_not_pass_is_the_one_evidence_refuses() {
        let (green, master) = (pr(true, false, true), Base::Branch("master".into()));
        let (blocks, runs) = ([block("b"), block("a")], [run("a", "#a", false)]);
        let g = gates(&green, &master, &runs, &blocks, Some(true));
        let names: Vec<(&str, GateState)> = g.gates[3..].iter().map(|g| (g.name.as_str(), g.state)).collect();
        assert_eq!(names, [("run a", GateState::Fail), ("run b", GateState::Pending), ("check set", GateState::Pass)]);
        assert_eq!(g.unmet, Some(Unmet::Failed(&runs[0])));
        assert_eq!(evidence(&runs, &green.head, &blocks).err(), g.unmet);
        let first = g.gates.iter().find(|g| g.state != GateState::Pass).map(|g| g.name.as_str());
        assert_eq!(first, Some("run a"));
    }

    #[test]
    fn gates_name_the_head_the_reviewer_and_the_run() {
        let (green, master, runs, blocks) =
            (pr(true, false, true), Base::Branch("master".into()), [run("a", "#a", true)], [block("a")]);
        let g = gates(&green, &master, &runs, &blocks, Some(true));
        let words: Vec<(&str, &str)> = g.gates.iter().map(|g| (g.name.as_str(), g.detail.as_str())).collect();
        assert_eq!(
            words,
            [
                ("base", "on master"),
                ("approving review", "approved at abcdef0 by quokka-reviewer"),
                ("forge checks", "green at abcdef0"),
                ("run a", "pass at abcdef0, attempt 1"),
                ("check set", "abcdef0 names the error blocks carried now"),
            ]
        );
        let unset = PullRequest { reviewer: String::new(), ..pr(true, false, false) };
        let g = gates(&unset, &Base::Unknown, &[], &[], None);
        assert_eq!(g.gates[1].detail, "set the project's reviewer; land refuses reviewer_invalid");
        assert_eq!(g.gates[0].detail, "the claim records no base; land tests origin's default branch");
        assert_eq!(g.gates[3].detail, "no clone holds abcdef0, so its check set is not read");
    }
}
