//! Model and effort routing (36, section 3; 37, design change 9). One table, in [`route`]: the intent decides the
//! task, and the task, the issue's level and its `complex` label decide the route. The session passes both fields to
//! the dispatch, so no role file fixes a model.

use serde::Serialize;

use crate::risk::Level;

/// The model a dispatch runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[allow(missing_docs)]
pub enum Model {
    Fable,
    Opus,
    Sonnet,
}

/// The reasoning effort a dispatch sets. Every route sets one, since a request that omits it gets the model's default.
/// No `xhigh`: it returns only on measured evidence (36, section 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[allow(missing_docs)]
pub enum Effort {
    Medium,
    High,
}

/// What an intent sends to a dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Route {
    /// The model.
    pub model: Model,
    /// The effort.
    pub effort: Effort,
}

/// The task an intent leads to, the dispatch rows of the table in 36, section 3. Design, shape and derive are not
/// here: the session runs them itself, on its own model. Research and escalation are not here either: research is
/// no intent, and no role diagnoses an escalation yet (milestone 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Build a ready issue, or fix the findings of its review.
    Build,
    /// A full review of one lens: the whole of round one, or a lens that a later round adds when the level rises.
    ReviewFirst,
    /// A review of a later round: the fix diff and the open rows.
    ReviewVerify,
}

/// The route of `task` at `level` (36, section 3). A `complex` issue sends its build and fix to Fable at high effort
/// whatever its level.
pub fn route(task: Task, level: Level, complex: bool) -> Route {
    use Effort::{High, Medium};
    use Model::{Fable, Opus, Sonnet};
    let at = |model, effort| Route { model, effort };
    // One row for each task: the cells for low, medium and high risk.
    let row = match task {
        Task::Build if complex => [at(Fable, High); 3],
        Task::Build => [at(Sonnet, Medium), at(Sonnet, High), at(Opus, High)],
        Task::ReviewFirst => [at(Sonnet, Medium), at(Opus, High), at(Fable, High)],
        // A low issue with a fixed round-one error has a verify round too (37, design change 6).
        Task::ReviewVerify => [at(Sonnet, Medium), at(Sonnet, Medium), at(Opus, Medium)],
    };
    row[level as usize]
}

#[cfg(test)]
mod tests {
    use super::Effort::{High, Medium};
    use super::Model::{Fable, Opus, Sonnet};
    use super::*;
    use crate::intents::{Claim, Intent, Issue, Milestone, PullRequest, Verdict, design, issue, milestone};
    use crate::rounds::{self, Review, Row};

    const NOW: i64 = 1_792_281_600;
    const LEVELS: [Level; 3] = [Level::Low, Level::Medium, Level::High];

    fn held(level: Level, complex: bool) -> Issue {
        Issue {
            status: "in-progress".into(),
            claims: vec![Claim::default()],
            declared: Some(level),
            complex,
            ..Issue::default()
        }
    }

    /// The run that `pr` writes at `h`: the level it computed. With none, an intent reads medium, never low (37,
    /// design change 1).
    fn computed(level: Level) -> Vec<crate::intents::Run> {
        let risk = Some(crate::intents::Recorded { level, security: false });
        vec![crate::intents::Run { commit: "h".into(), risk, ..Default::default() }]
    }

    fn pr(head: &str) -> PullRequest {
        PullRequest { head: head.into(), base: Some("master".into()), green: true, ..PullRequest::default() }
    }

    /// Round one of `level` read `h`, each lens holding `rows`.
    fn round_one(level: Level, rows: &[Row]) -> Vec<Review> {
        let lens = rounds::lenses(level, false);
        lens.into_iter()
            .map(|lens| Review { id: format!("r/{lens}"), head: "h".into(), round: 1, lens, rows: rows.to_vec() })
            .collect()
    }

    fn ready(level: Level, complex: bool) -> Option<Route> {
        let open = Issue { status: "open".into(), declared: Some(level), complex, ..Issue::default() };
        let v = issue(&open, NOW);
        assert_eq!(v.intent, Some(Intent::Ready));
        v.route
    }

    fn first_review(level: Level) -> Option<Route> {
        let v = issue(&Issue { pr: Some(pr("h")), runs: computed(level), ..held(level, false) }, NOW);
        assert_eq!((v.intent, v.note.starts_with("round 1")), (Some(Intent::NeedsReview), true), "{v:?}");
        v.route
    }

    fn verify_review(level: Level) -> Option<Route> {
        let i =
            Issue { pr: Some(pr("h2")), runs: computed(level), reviews: round_one(level, &[]), ..held(level, false) };
        let v = issue(&i, NOW);
        assert_eq!((v.intent, v.note.starts_with("round 2")), (Some(Intent::NeedsReview), true), "{v:?}");
        v.route
    }

    fn response(level: Level, complex: bool) -> Option<Route> {
        let reviews = round_one(level, &[Row::default()]);
        let i = Issue { pr: Some(pr("h")), runs: computed(level), reviews, ..held(level, complex) };
        let v = issue(&i, NOW);
        assert_eq!(v.intent, Some(Intent::NeedsResponse), "{v:?}");
        v.route
    }

    fn escalated(level: Level) -> Verdict {
        let claim = Claim { at: Some(NOW), released: Some(NOW + 60), escalated: true };
        let i = Issue { status: "open".into(), claims: vec![claim], declared: Some(level), ..Issue::default() };
        issue(&i, NOW)
    }

    #[test]
    fn intent_carries_model_and_effort() {
        let r = |model, effort| Some(Route { model, effort });
        // The table of 36, section 3, written out cell by cell: low, medium, high.
        let build = [r(Sonnet, Medium), r(Sonnet, High), r(Opus, High)];
        let first = [r(Sonnet, Medium), r(Opus, High), r(Fable, High)];
        let verify = [r(Sonnet, Medium), r(Sonnet, Medium), r(Opus, Medium)];
        for (n, level) in LEVELS.into_iter().enumerate() {
            assert_eq!(ready(level, false), build[n], "build at {level}");
            assert_eq!(response(level, false), build[n], "fix at {level}");
            assert_eq!(first_review(level), first[n], "review round one at {level}");
            assert_eq!(verify_review(level), verify[n], "review verification at {level}");
            // No role diagnoses an escalation yet (milestone 13), so it dispatches no one.
            let v = escalated(level);
            assert_eq!((v.intent, v.note.as_str(), v.route), (Some(Intent::Blocked), "escalated", None));
        }
        // Design, shape and derive run in the session itself: the intents carry no route.
        let shape = design("d", &[], &["c open".to_string()], false);
        assert_eq!((shape.intent, shape.route), (Some(Intent::NeedsShaping), None));
        let approved = Milestone {
            status: "approved".into(),
            issues: vec!["open".into()],
            finish: vec![crate::intents::Block { name: "b".into(), hash: "#b".into() }],
            ..Milestone::default()
        };
        let derive = milestone("m", &approved);
        assert_eq!((derive.intent, derive.route), (Some(Intent::NeedsDerivation), None));
        // `complex` sends a build and a fix to Fable at high, and nothing else.
        for level in LEVELS {
            assert_eq!(ready(level, true), r(Fable, High), "complex build at {level}");
            assert_eq!(response(level, true), r(Fable, High), "complex fix at {level}");
        }
        let held_complex = Issue { pr: Some(pr("h")), runs: computed(Level::Medium), ..held(Level::Medium, true) };
        assert_eq!(issue(&held_complex, NOW).route, r(Opus, High), "complex leaves a review alone");
        // An intent that dispatches no one carries no route.
        let working = Issue { status: "in-progress".into(), claims: vec![Claim::default()], ..Issue::default() };
        assert_eq!(issue(&working, NOW).route, None);
    }

    #[test]
    fn a_low_issue_with_a_fixed_error_routes_its_verify_round() {
        let fixed = Row { error: true, answer: Some(rounds::Answer::Fixed), ..Row::default() };
        let i = Issue {
            pr: Some(pr("h2")),
            runs: computed(Level::Low),
            reviews: round_one(Level::Low, &[fixed]),
            ..held(Level::Low, false)
        };
        let v = issue(&i, NOW);
        assert_eq!((v.intent, v.note.as_str()), (Some(Intent::NeedsReview), "round 2: missing lenses: verify"));
        assert_eq!(v.route, Some(Route { model: Sonnet, effort: Medium }));
    }

    #[test]
    fn a_lens_a_level_rise_adds_routes_as_a_full_review() {
        // Round one at medium; each lens recorded a `risk` row, which raises the issue to high.
        let raise = Row { risk: true, ..Row::default() };
        let i = Issue {
            pr: Some(pr("h2")),
            runs: computed(Level::Medium),
            reviews: round_one(Level::Medium, &[raise]),
            ..held(Level::Medium, false)
        };
        let v = issue(&i, NOW);
        assert!(v.note.contains("tests-and-checks"), "{v:?}");
        assert_eq!(v.route, Some(Route { model: Fable, effort: High }), "a full lens at high is round one");
    }

    #[test]
    fn a_ready_issue_routes_by_the_level_the_review_path_reads() {
        let open = Issue { status: "open".into(), declared: Some(Level::Medium), ..Issue::default() };
        let routed = |i: &Issue| issue(i, NOW).route;
        assert_eq!(routed(&open), Some(Route { model: Sonnet, effort: High }));
        // A pending delta that the issue caused raises it with no pull request in view.
        assert_eq!(routed(&Issue { delta: true, ..open.clone() }), Some(Route { model: Opus, effort: High }));
        // A `risk` row of an earlier pull request raises it too.
        let raise = Row { risk: true, ..Row::default() };
        let earlier = Issue { reviews: round_one(Level::Medium, &[raise]), ..open.clone() };
        assert_eq!(routed(&earlier), Some(Route { model: Opus, effort: High }));
        // A label alone sets the floor: low stays low with no run.
        let low = Issue { declared: Some(Level::Low), ..open };
        assert_eq!(routed(&low), Some(Route { model: Sonnet, effort: Medium }));
    }

    #[test]
    fn a_label_the_reader_could_not_parse_adds_its_note() {
        let open = Issue { status: "open".into(), unread: vec!["risk \"High\" is bad".into()], ..Issue::default() };
        let v = issue(&open, NOW);
        assert_eq!((v.intent, v.note.as_str()), (Some(Intent::Ready), "risk \"High\" is bad"));
        let none = Issue { status: "done".into(), unread: open.unread, ..Issue::default() };
        assert_eq!(issue(&none, NOW).note, "", "an issue with no intent keeps its note empty");
    }
}
