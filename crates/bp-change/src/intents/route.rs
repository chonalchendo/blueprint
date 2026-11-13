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

/// The task an intent leads to, the rows of the table in 36, section 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// Design, shape or derive.
    Design,
    /// Build a ready issue, or fix the findings of its review.
    Build,
    /// A review of round one: one dispatch for each lens.
    ReviewFirst,
    /// A review of a later round: the fix diff and the open rows.
    ReviewVerify,
    /// Diagnose an escalated claim.
    Escalation,
}

/// The route of `task` at `level` (36, section 3); `None` where the table reads n/a. A `complex` issue sends its
/// build and fix to Fable at high effort whatever its level.
pub fn route(task: Task, level: Level, complex: bool) -> Option<Route> {
    use Effort::{High, Medium};
    use Model::{Fable, Opus, Sonnet};
    let at = |model, effort| Some(Route { model, effort });
    // One row for each task: the cells for low, medium and high risk.
    let row = match task {
        Task::Design => [at(Fable, High); 3],
        Task::Build if complex => [at(Fable, High); 3],
        Task::Build => [at(Sonnet, Medium), at(Sonnet, High), at(Opus, High)],
        Task::ReviewFirst => [at(Sonnet, Medium), at(Opus, High), at(Fable, High)],
        // A low issue has one round, so it has no verification review.
        Task::ReviewVerify => [None, at(Sonnet, Medium), at(Opus, Medium)],
        Task::Escalation => [None, at(Fable, High), at(Fable, High)],
    };
    row[level as usize]
}

#[cfg(test)]
mod tests {
    use super::Effort::{High, Medium};
    use super::Model::{Fable, Opus, Sonnet};
    use super::*;
    use crate::intents::{Claim, Intent, Issue, Milestone, PullRequest, design, issue, milestone};
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

    fn escalated(level: Level) -> Option<Route> {
        let claim = Claim { at: Some(NOW), released: Some(NOW + 60), escalated: true };
        let i = Issue { status: "open".into(), claims: vec![claim], declared: Some(level), ..Issue::default() };
        let v = issue(&i, NOW);
        assert_eq!((v.intent, v.note.as_str()), (Some(Intent::Blocked), "escalated"));
        v.route
    }

    #[test]
    fn intent_carries_model_and_effort() {
        let r = |model, effort| Some(Route { model, effort });
        // The table of 36, section 3, written out cell by cell: low, medium, high.
        let build = [r(Sonnet, Medium), r(Sonnet, High), r(Opus, High)];
        let first = [r(Sonnet, Medium), r(Opus, High), r(Fable, High)];
        let verify = [None, r(Sonnet, Medium), r(Opus, Medium)];
        let escalation = [None, r(Fable, High), r(Fable, High)];
        for (n, level) in LEVELS.into_iter().enumerate() {
            assert_eq!(ready(level, false), build[n], "build at {level}");
            assert_eq!(response(level, false), build[n], "fix at {level}");
            assert_eq!(first_review(level), first[n], "review round one at {level}");
            assert_eq!(verify_review(level), verify[n], "review verification at {level}");
            assert_eq!(escalated(level), escalation[n], "escalation at {level}");
        }
        // Design, shape and derive are Fable at high at every level; the intents hold no level.
        let shape = design("d", &[], &["c open".to_string()], false);
        assert_eq!((shape.intent, shape.route), (Some(Intent::NeedsShaping), r(Fable, High)));
        let approved = Milestone {
            status: "approved".into(),
            issues: vec!["open".into()],
            finish: vec![crate::intents::Block { name: "b".into(), hash: "#b".into() }],
            ..Milestone::default()
        };
        let derive = milestone("m", &approved);
        assert_eq!((derive.intent, derive.route), (Some(Intent::NeedsDerivation), r(Fable, High)));
        for level in LEVELS {
            assert_eq!(route(Task::Design, level, false), r(Fable, High));
        }
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
}
