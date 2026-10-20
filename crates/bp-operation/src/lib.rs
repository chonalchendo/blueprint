//! Operation domain: role, skill, dispatch template, handoff; the bundle's metadata. This milestone holds the
//! actions an agent runs, what each one's walk returns (21, the action rows), and the roles a commit names.

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
const NODES: [Kind; 9] = [
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
            Self::Review | Self::Research => 8_000,
            Self::TriageDrift | Self::Distil | Self::Capture => 4_000,
        }
    }

    /// The kind the walk starts from.
    pub fn seed(self) -> &'static [Kind] {
        match self {
            Self::Complete | Self::Review | Self::Respond => &[Kind::Issue],
            Self::Derive => &[Kind::Milestone],
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
            Self::Distil => &[Kind::Learning],
            Self::Capture => &[],
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
}
