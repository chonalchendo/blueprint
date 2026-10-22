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
pub const ASPECTS: [[&str; 2]; 2] = [["checks pass", "within scope"], ["all answered", "head moved"]];

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
pub fn score(g: &mut Graded, skill: &Skill, fx: &Fixture, trace: &Trace, log: &str) {
    let calls = trace.bp_calls();
    let allowed = skill.allowed();
    g.reached = !calls.is_empty();
    g.out_of_row = calls.iter().filter(|c| !allowed.contains(&c.verb.as_str())).count();
    match skill.action {
        Action::Review => review(g, fx, trace, &calls, log),
        _ => worker(g, skill, fx, &calls, log),
    }
}

/// `bp-review`: the steps are the skill's steps 1 to 5; the outcome is recall against the ground truth.
fn review(g: &mut Graded, fx: &Fixture, trace: &Trace, calls: &[BpCall], log: &str) {
    let first = |c: &BpCall| c.args.split_whitespace().next().unwrap_or_default().to_string();
    let head = crate::eval_grade::short(&fx.head);
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
/// the claim not escalated, and then for complete the checks and the file scope of the landed issue, for respond a
/// disposition for every finding, a reason for every decline and a moved head when a finding was fixed.
fn worker(g: &mut Graded, skill: &Skill, fx: &Fixture, calls: &[BpCall], log: &str) {
    let called = |verb: &str| calls.iter().any(|c| c.verb == verb);
    let walked = calls.iter().any(|c| c.verb == "walk" && c.args.contains(&format!("--for {}", skill.action)));
    g.end = End::parse(log);
    let stopped = g.end.done && !g.end.escalated();
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
        let answered = |n: usize| g.end.answers.iter().any(|a| a.finding == n && (a.fixed || a.reasoned));
        let all = (1..=fx.truth.len()).all(answered);
        steps.extend((1..=fx.truth.len()).map(|n| (format!("disposition {n}"), answered(n))));
        let fixed = g.end.answers.iter().any(|a| a.fixed);
        let commit = if fixed { g.end.moved } else { !g.end.answers.is_empty() };
        steps.push(("commit".to_string(), commit));
        g.aspects = [all, g.end.moved];
        g.pass = stopped && all && commit;
    }
    g.steps = steps;
}
