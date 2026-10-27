//! The landing gates (32, design change 5; 26, design change 4): every part of needs landing in `land`'s refusal order,
//! each in words, and the first that does not hold. [`landing`](super::landing) is that first unmet gate alone, so
//! `land`, the needs-landing intent and the Change page read one function (21, rule 1).

use serde::Serialize;
use ts_rs::TS;

use super::{Base, Block, PullRequest, Run, Unmet, evidence, latest_at};

/// One gate's state. A word, never a colour alone (19, principle 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub enum GateState {
    /// It holds at the head.
    #[serde(rename = "pass")]
    Pass,
    /// It does not hold, and no wait makes it hold: a fix and a push do.
    #[serde(rename = "fail")]
    Fail,
    /// It does not hold yet: it waits on an act of another, or on a run.
    #[serde(rename = "pending")]
    Pending,
    /// `install` has not set it up (18, Change 1).
    #[serde(rename = "not configured")]
    NotConfigured,
}

/// One gate: its name, its state, and the head, run or branch it refers to, in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Gate {
    /// What it tests: `base`, `approving review`, `forge checks`, or `run <block>`.
    pub name: String,
    /// Whether it holds.
    pub state: GateState,
    /// What it read: the sha, the reviewer, the run.
    pub detail: String,
}

/// Every gate, in order, and the first that does not hold.
#[derive(Debug)]
pub struct Gates<'a> {
    /// The base, the approving review, the forge checks, then one gate per `error` block, and one per failed run of a
    /// check the issue no longer holds.
    pub gates: Vec<Gate>,
    /// The first part of needs landing that does not hold; `None` when the pull request may land.
    pub unmet: Option<Unmet<'a>>,
}

/// The gates of the pull request `pr` on the claim's `base`, with the issue's runs and its `error` blocks now. Pure.
pub fn gates<'a>(pr: &'a PullRequest, base: &'a Base, runs: &'a [Run], blocks: &'a [Block]) -> Gates<'a> {
    let head = &pr.head[..pr.head.len().min(7)];
    let gate = |name: &str, state, detail: String| Gate { name: name.into(), state, detail };
    let (based, unmerged) = match base {
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
        Base::Unknown => (gate("base", GateState::Pass, "the claim records no base".into()), None),
    };
    let review = match (pr.reviewer.as_str(), pr.approved) {
        ("", _) => gate(
            "approving review",
            GateState::NotConfigured,
            "set the project's reviewer; land refuses reviewer_invalid".into(),
        ),
        (who, true) => gate("approving review", GateState::Pass, format!("approved at {head} by {who}")),
        (who, false) => gate("approving review", GateState::Pending, format!("no approval by {who} at {head}")),
    };
    let checks = match (pr.green, pr.failed) {
        (true, _) => gate("forge checks", GateState::Pass, format!("green at {head}")),
        (false, true) => gate("forge checks", GateState::Fail, format!("a check failed at {head}")),
        (false, false) => gate("forge checks", GateState::Pending, format!("pending at {head}")),
    };
    let latest = latest_at(runs, &pr.head);
    let mut out = vec![based, review, checks];
    for b in blocks {
        let (state, detail) = match latest.get(b.name.as_str()) {
            None => (GateState::Pending, format!("no run at {head}")),
            Some(r) if !r.passed => (GateState::Fail, format!("fail at {head}, attempt {}", r.attempt)),
            Some(r) if r.check_hash != b.hash => (GateState::Pending, format!("run at {head} is of an older text")),
            Some(r) => (GateState::Pass, format!("pass at {head}, attempt {}", r.attempt)),
        };
        out.push(gate(&format!("run {}", b.name), state, detail));
    }
    // A failed run of a block the issue no longer holds still refuses the land, so it shows.
    for r in latest.values().filter(|r| !r.passed && !blocks.iter().any(|b| b.name == r.check)) {
        out.push(gate(&format!("run {}", r.check), GateState::Fail, format!("fail at {head}; the block is gone")));
    }
    let unmet = unmerged
        .or_else(|| (!pr.approved).then_some(Unmet::Review))
        .or_else(|| (!pr.green).then_some(Unmet::Checks))
        .or_else(|| evidence(runs, &pr.head, blocks).err());
    Gates { gates: out, unmet }
}

#[cfg(test)]
mod tests {
    use super::super::landing;
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

    /// A table row: the pull request, the base, the runs, the unmet gate, and each gate's state.
    type Row<'a> = (PullRequest, &'a Base, &'a [Run], Option<Unmet<'a>>, Vec<GateState>);

    fn states(g: &Gates) -> Vec<(String, GateState)> {
        g.gates.iter().map(|g| (g.name.clone(), g.state)).collect()
    }

    #[test]
    fn gates_hold_every_unmet_in_land_order_and_landing_is_the_first() {
        use GateState::{Fail, NotConfigured, Pass, Pending};
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
        let table: Vec<Row> = vec![
            (green.clone(), &master, &pass, None, vec![Pass, Pass, Pass, Pass]),
            (
                green.clone(),
                &below,
                &pass,
                Some(Unmet::Unmerged("projects/bp/issues/a")),
                vec![Pending, Pass, Pass, Pass],
            ),
            (green.clone(), &stacked, &pass, Some(Unmet::Mismatch("bp/a")), vec![Fail, Pass, Pass, Pass]),
            (pr(true, false, false), &master, &pass, Some(Unmet::Review), vec![Pass, Pending, Pass, Pass]),
            (unreviewed, &master, &pass, Some(Unmet::Review), vec![Pass, NotConfigured, Pass, Pass]),
            (pr(false, false, true), &master, &pass, Some(Unmet::Checks), vec![Pass, Pass, Pending, Pass]),
            (pr(false, true, true), &master, &pass, Some(Unmet::Checks), vec![Pass, Pass, Fail, Pass]),
            (green.clone(), &master, &fail, Some(Unmet::Failed(&fail[0])), vec![Pass, Pass, Pass, Fail]),
            (green.clone(), &master, &[], Some(Unmet::Evidence("a")), vec![Pass, Pass, Pass, Pending]),
            (green.clone(), &master, &old, Some(Unmet::Evidence("a")), vec![Pass, Pass, Pass, Pending]),
            (green.clone(), &master, &gone, Some(Unmet::Failed(&gone[1])), vec![Pass, Pass, Pass, Pass, Fail]),
            (
                pr(false, true, false),
                &below,
                &fail,
                Some(Unmet::Unmerged("projects/bp/issues/a")),
                vec![Pending, Pending, Fail, Fail],
            ),
        ];
        for (pr, base, runs, unmet, want) in &table {
            let g = gates(pr, base, runs, &blocks);
            assert_eq!(&g.gates.iter().map(|g| g.state).collect::<Vec<_>>(), want, "{:?}", states(&g));
            assert_eq!(landing(pr, base, runs, &blocks).err().as_ref(), unmet.as_ref(), "landing is the first unmet");
            assert_eq!(g.unmet.as_ref(), unmet.as_ref());
        }
    }

    #[test]
    fn gates_name_the_head_the_reviewer_and_the_run() {
        let (green, master, runs, blocks) =
            (pr(true, false, true), Base::Branch("master".into()), [run("a", "#a", true)], [block("a")]);
        let g = gates(&green, &master, &runs, &blocks);
        let words: Vec<(&str, &str)> = g.gates.iter().map(|g| (g.name.as_str(), g.detail.as_str())).collect();
        assert_eq!(
            words,
            [
                ("base", "on master"),
                ("approving review", "approved at abcdef0 by quokka-reviewer"),
                ("forge checks", "green at abcdef0"),
                ("run a", "pass at abcdef0, attempt 1"),
            ]
        );
        let unset = PullRequest { reviewer: String::new(), ..pr(true, false, false) };
        let g = gates(&unset, &Base::Unknown, &[], &[]);
        assert_eq!(g.gates[1].detail, "set the project's reviewer; land refuses reviewer_invalid");
        assert_eq!(g.gates[0].detail, "the claim records no base");
    }
}
