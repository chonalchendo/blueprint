//! The score of one trial (29, design change 2): the steps its transcript shows, the calls outside the role's row and
//! the outcome. The steps are named calls and records, not verb order, because verb order carried only dispatch shape
//! (17 E7). Each skill's steps come from its row in 21.

use std::collections::BTreeSet;

use bp_operation::Action;

use crate::eval::Skill;
use crate::eval_end::End;
use crate::eval_fixture::{Fixture, Review};
use crate::eval_trace::{BpCall, Trace};

/// A review passes when its review is recorded (`walk --done`) and finds at least this share of the ground truth.
pub const RECALL: f64 = 0.5;

/// The names of [`Graded::aspects`] for complete and for respond.
pub const ASPECTS: [[&str; 2]; 2] = [["checks pass", "within scope"], ["all answered", "fixes observed"]];

#[derive(Default)]
pub struct Graded {
    pub arm: String,
    pub fixture: String,
    pub number: usize,
    pub trial: String,
    pub sha: String,
    pub log_sha: String,
    /// The hash of the role file and skill body this trial ran on, from `<trial>.hash`.
    pub hash: Option<String>,
    /// The fixture and arm protocol this trial ran on, from the second line of `<trial>.hash`; empty when absent.
    pub stamp: String,
    pub models: BTreeSet<String>,
    /// The skill is where the arm puts it: preloaded in `with`, absent in `without`, filler in `filler`.
    pub loaded: bool,
    /// The run made at least one `bp` call: only such a run is scored on adherence.
    pub reached: bool,
    pub steps: Vec<(String, bool)>,
    pub out_of_row: usize,
    pub pass: bool,
    /// Review: ground-truth items some finding of the table matches, and items the final text names in any form.
    pub hits: usize,
    pub prose_hits: usize,
    pub truth: usize,
    /// Each finding's location, its text and the ground-truth item it matches (a position), if any.
    pub findings: Vec<(String, String, Option<usize>)>,
    /// What the sandbox held at the end. `end.done` is the skill's stop: the review is recorded, or the worker's
    /// `walk --done` is true.
    pub end: End,
    /// The two facts that the outcome adds to the stop, in the order of [`ASPECTS`]: complete's checks passed and its
    /// files stay in the issue's crates; respond's findings are all answered and its head moved.
    pub aspects: [bool; 2],
    pub failure: Option<String>,
}

impl Graded {
    pub fn adherence(&self) -> f64 {
        crate::eval_grade::share(self.steps.iter().filter(|s| s.1).count(), self.steps.len())
    }
}

/// Fills the scores of a trial from its transcript and its vault log, which holds the `end:` lines of a worker.
/// `head` is the head of the trial's pull request: a mutant's planted head, not the fixture's.
pub fn score(g: &mut Graded, skill: &Skill, fx: &Fixture, head: &str, trace: &Trace, log: &str) {
    let calls = trace.bp_calls();
    let allowed = skill.allowed();
    g.reached = !calls.is_empty();
    g.out_of_row = calls.iter().filter(|c| !allowed.contains(&c.verb.as_str())).count();
    match skill.action {
        Action::Review => review(g, fx, head, trace, &calls, log),
        _ => worker(g, skill, fx, &calls, log),
    }
    // A trial the runner killed, or that left no transcript, did not finish: whatever the sandbox holds is not its result.
    g.pass &= g.failure.is_none();
}

/// `bp-review`: the steps are the skill's steps 1 to 5; the outcome is recall against the ground truth.
fn review(g: &mut Graded, fx: &Fixture, head: &str, trace: &Trace, calls: &[BpCall], log: &str) {
    let first = |c: &BpCall| c.args.split_whitespace().next().unwrap_or_default().to_string();
    let head = crate::eval_grade::short(head);
    let review = Review::parse(&trace.final_text());
    let steps = [
        ("walk", calls.iter().any(|c| c.verb == "walk" && c.args.contains("--for review"))),
        ("claim", calls.iter().any(|c| c.verb == "list" && first(c) == "claim")),
        ("diff", trace.commands().iter().any(|c| c.contains("git") && c.contains("diff") && c.contains(head))),
        ("deltas", calls.iter().any(|c| c.verb == "list" && first(c) == "delta")),
        ("review", review.as_ref().is_some_and(|r| r.tables == 1 && r.head.starts_with(head))),
    ];
    g.steps = steps.into_iter().map(|(n, ok)| (n.to_string(), ok)).collect();
    g.end.done = log.lines().any(|l| l.starts_with("record review "));
    let findings = review.map(|r| r.findings).unwrap_or_default();
    let assigned = fx.assign(&findings);
    g.hits = fx.hits(&assigned);
    g.prose_hits = fx.named_in(&trace.final_text());
    g.truth = fx.truth.len();
    g.findings = findings.iter().zip(assigned).map(|(f, item)| (f.location.clone(), f.text.clone(), item)).collect();
    g.pass = g.end.done && g.truth > 0 && crate::eval_grade::share(g.hits, g.truth) >= RECALL;
}

/// `bp-complete` and `bp-respond`: the steps are the calls of 21's row. The outcome is the stop (`walk --done`) with
/// the claim not escalated and the issue's body as the fixture gave it, and then for complete the checks and the file
/// scope of the landed issue. For respond each finding must be closed as the past review closed it: a `fixed` item
/// needs a fixed answer, its file in the diff against the review's head and its probe when it has one; a `declined`
/// item needs a reasoned decline and no fix. Otherwise the outcome would measure the disposition syntax only.
fn worker(g: &mut Graded, skill: &Skill, fx: &Fixture, calls: &[BpCall], log: &str) {
    let called = |verb: &str| calls.iter().any(|c| c.verb == verb);
    let walked = calls.iter().any(|c| c.verb == "walk" && c.args.contains(&format!("--for {}", skill.action)));
    g.end = End::parse(log);
    let stopped = g.end.done && !g.end.escalated() && g.end.kept(fx);
    let mut steps = vec![("walk".to_string(), walked)];
    if skill.action == Action::Complete {
        steps.extend(
            [("cut", called("cut")), ("check", called("check")), ("pr", called("pr"))]
                .map(|(n, ok)| (n.to_string(), ok)),
        );
        g.aspects = [g.end.checks, !g.end.files.is_empty() && g.end.within(&fx.scope)];
        g.pass = stopped && g.aspects == [true; 2];
    } else {
        steps.extend([("check", called("check")), ("pr", called("pr"))].map(|(n, ok)| (n.to_string(), ok)));
        let of = |n: usize| g.end.answers.iter().filter(move |a| a.finding == n);
        let probed = |n: usize| g.end.probes.iter().filter(|p| p.0 == n).all(|p| p.1);
        let observed = |n: usize| of(n).any(|a| a.fixed) && fx.truth[n - 1].touched(&g.end.files) && probed(n);
        let closed = |n: usize| {
            if fx.truth[n - 1].fixed { observed(n) } else { of(n).all(|a| !a.fixed) && of(n).any(|a| a.reasoned) }
        };
        let items = 1..=fx.truth.len();
        steps.extend(items.clone().map(|n| (format!("disposition {n}"), closed(n))));
        let fixed = g.end.answers.iter().any(|a| a.fixed);
        let commit = if fixed { g.end.moved } else { !g.end.answers.is_empty() };
        steps.push(("commit".to_string(), commit));
        let answered = items.clone().all(|n| of(n).any(|a| a.fixed || a.reasoned));
        g.aspects = [answered, items.clone().filter(|n| fx.truth[n - 1].fixed).all(observed)];
        g.pass = stopped && commit && items.into_iter().all(closed);
    }
    g.steps = steps;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mutant trial's reviewer diffs and names the planted head, so the two steps that check the head read it.
    #[test]
    fn the_review_steps_read_the_pull_requests_head() {
        let fx = Fixture::parse("m", "base: b0000000\nhead: 1111111aaaa\ntitle: T\ntruth:\n- a.rs:1 | x\n---\nbody")
            .unwrap();
        let planted = "2222222bbbb";
        let lines = [
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"git diff b0000000 2222222"}}]}}"#.to_string(),
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"head: 2222222\n| severity | location | finding |\n|---|---|---|\n| warn | a.rs:1 | x |"}]}}"#.to_string(),
        ];
        let trace = Trace::parse(&lines.join("\n"));
        let step = |g: &Graded, n: &str| g.steps.iter().find(|s| s.0 == n).unwrap().1;
        let skill = Skill::parse("bp-review").unwrap();
        let mut g = Graded::default();
        score(&mut g, &skill, &fx, planted, &trace, "");
        assert!(step(&g, "diff") && step(&g, "review"), "{:?}", g.steps);
        let mut landed = Graded::default();
        score(&mut landed, &skill, &fx, &fx.head, &trace, "");
        assert!(!step(&landed, "diff") && !step(&landed, "review"));
    }
}
