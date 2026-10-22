//! Operation domain: role, skill, dispatch template, handoff; the bundle's metadata. This milestone holds the
//! actions an agent runs, what each one's walk returns (21, the action rows), and the roles a commit names.

use std::collections::BTreeSet;

use bp_vault::Kind;
/// The required links moved to the vault, so the integrity check reads the same ones (21, rule 3).
pub use bp_vault::Requires;

bp_vault::closed_enum! {
    /// The actions an agent runs from a walk (21). Human acts (approve, land, stamp) read a page, not a walk.
    pub enum Action {
        Design = "design",
        Shape = "shape",
        Derive = "derive",
        Complete = "complete",
        Review = "review",
        Respond = "respond",
        TriageDrift = "triage-drift",
        Deliberate = "deliberate",
        Distil = "distil",
        Capture = "capture",
        Research = "research",
    }
}

bp_vault::closed_enum! {
    /// Who made a commit: the `Bp-Role` trailer (12, delta of 2026-10-18), from `BP_ROLE`, `human` when unset.
    pub enum Role {
        Session = "session",
        Worker = "worker",
        Reviewer = "reviewer",
        Researcher = "researcher",
        Human = "human",
    }
}

/// Every node kind: a record is evidence about its parent, so no walk starts from one.
pub const NODES: [Kind; 9] = [
    Kind::Project,
    Kind::Design,
    Kind::Milestone,
    Kind::Issue,
    Kind::Thread,
    Kind::Decision,
    Kind::Convention,
    Kind::Learning,
    Kind::Inbox,
];

impl Action {
    /// The token budget (21, rule 6).
    pub fn budget(self) -> usize {
        match self {
            Self::Complete | Self::Respond => 15_000,
            Self::Design | Self::Shape | Self::Derive => 10_000,
            Self::Review | Self::Research | Self::Deliberate => 8_000,
            Self::TriageDrift | Self::Distil | Self::Capture => 4_000,
        }
    }

    /// The kind the walk starts from.
    pub fn seed(self) -> &'static [Kind] {
        match self {
            Self::Complete | Self::Review | Self::Respond => &[Kind::Issue],
            Self::Derive => &[Kind::Milestone],
            Self::Deliberate => &[Kind::Thread],
            Self::Design | Self::Shape | Self::TriageDrift | Self::Research => &[Kind::Design, Kind::Project],
            Self::Distil | Self::Capture => &NODES,
        }
    }

    /// The kinds the walk returns as bodies besides the seed, which is always one (21, the context column). Every
    /// other reached node is an id line; a sibling issue is never a body.
    pub fn bodies(self) -> &'static [Kind] {
        match self {
            Self::Complete => &[Kind::Milestone, Kind::Design, Kind::Learning],
            Self::Review | Self::Respond | Self::TriageDrift => &[Kind::Design],
            Self::Design | Self::Research => &[Kind::Design, Kind::Project],
            Self::Shape => &[Kind::Design, Kind::Learning],
            Self::Derive => &[Kind::Milestone, Kind::Design, Kind::Issue],
            Self::Deliberate => &[Kind::Decision, Kind::Thread],
            Self::Distil => &[Kind::Learning],
            Self::Capture => &[],
        }
    }

    /// The record kinds under the seed whose latest record the walk returns as a body, ahead of the fill: respond
    /// answers the latest review, which no link reaches (21, the context column).
    pub fn records(self) -> &'static [Kind] {
        match self {
            Self::Respond => &[Kind::Review],
            _ => &[],
        }
    }

    /// The links from the seed whose targets the action cannot run without: an issue's milestone and the designs
    /// it targets.
    pub fn requires(self) -> &'static [Requires] {
        match self {
            Self::Complete => &bp_vault::ISSUE_CONTEXT,
            Self::Review | Self::Respond => &bp_vault::ISSUE_CONTEXT[1..],
            _ => &[],
        }
    }
}

/// The read verbs, open to every role and to any subagent: a read writes nothing, and the role is a label, not a gate
/// (27, design change 3), so a denied read would cost a turn and protect nothing.
pub const READ_VERBS: [&str; 5] = ["where", "show", "list", "search", "walk"];

impl Action {
    /// The role whose label the row's write verbs run under. A reviewer returns its review and the session records
    /// it (21, the review row), so review belongs to the session and no action belongs to the reviewer.
    pub fn role(self) -> Role {
        match self {
            Self::Complete | Self::Respond => Role::Worker,
            Self::Research => Role::Researcher,
            Self::Design
            | Self::Shape
            | Self::Derive
            | Self::Review
            | Self::TriageDrift
            | Self::Deliberate
            | Self::Distil
            | Self::Capture => Role::Session,
        }
    }

    /// The write verbs of the action's row in 21: the source of the role's verbs and of the bundle test that holds a
    /// skill body to its row. The read verbs are left out, because every role may run them.
    pub fn verbs(self) -> &'static [&'static str] {
        match self {
            Self::Design => &["create", "set", "link", "diagram", "check", "record"],
            Self::Derive => &["create", "link"],
            Self::Complete => &["cut", "check", "record", "pr", "release"],
            Self::Review => &["record"],
            Self::Respond => &["check", "record", "pr", "release"],
            Self::Shape | Self::TriageDrift => &["create", "record"],
            Self::Deliberate => &["create", "set", "link", "transition"],
            Self::Distil => &["create", "set", "link", "record", "transition"],
            Self::Capture => &["create"],
            Self::Research => &[],
        }
    }
}

impl Role {
    /// The write verbs the role runs: the union of its actions' rows, plus `release` for the session, which 21 gives
    /// its own row because a resuming session runs it with no walk. The role hook denies any verb outside these and
    /// `READ_VERBS`; a dispatch template's forbidden list is the complement of both. `None` for the human, whom no
    /// list bounds: the forge is the gate.
    pub fn verbs(self) -> Option<Vec<&'static str>> {
        if self == Self::Human {
            return None;
        }
        let rows = Action::ALL.iter().filter(|a| a.role() == self).flat_map(|a| a.verbs());
        let release = (self == Self::Session).then_some(&"release");
        Some(rows.chain(release).copied().collect::<BTreeSet<_>>().into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_budget_and_a_seed() {
        for a in Action::ALL {
            assert!(a.budget() >= 4_000 && !a.seed().is_empty(), "{a}");
        }
        assert_eq!("complete".parse::<Action>().unwrap().budget(), 15_000);
        assert!("land".parse::<Action>().is_err(), "a human act reads a page, not a walk");
    }

    #[test]
    fn only_the_human_runs_the_human_acts() {
        for &r in Role::ALL {
            let verbs = r.verbs();
            assert_eq!(verbs.is_none(), r == Role::Human, "{r}");
            let verbs = verbs.unwrap_or_default();
            assert!(verbs.iter().all(|v| !["stamp", "land"].contains(v) && !READ_VERBS.contains(v)), "{r}");
        }
        assert!(Role::Reviewer.verbs().unwrap().is_empty(), "the session records the review (21)");
        assert!(Role::Session.verbs().unwrap().contains(&"check"), "the design action runs check (21)");
    }

    #[test]
    fn role_verbs_are_the_union_of_action_rows() {
        // The expected lists are 21's text: the worker's tool list, and the session's rows with the release row.
        let want: [(Role, &[&str]); 4] = [
            (Role::Session, &["check", "create", "diagram", "link", "record", "release", "set", "transition"]),
            (Role::Worker, &["check", "cut", "pr", "record", "release"]),
            (Role::Reviewer, &[]),
            (Role::Researcher, &[]),
        ];
        for (role, verbs) in want {
            assert_eq!(role.verbs().unwrap(), verbs, "{role}");
        }
        for a in Action::ALL {
            assert!(a.verbs().iter().all(|v| !READ_VERBS.contains(v)), "{a}: a row holds write verbs only");
        }
        assert_eq!(Action::Review.verbs(), ["record"], "the session records the review; the reviewer writes nothing");
    }
}
