//! Design domain: project, design, design kinds, concern answers, terms, diagrams, deltas, blueprint versions.
//! This milestone holds the status sets, their machines (25, design change 3), and the label and record vocabularies
//! of the logical data model (13).

bp_vault::closed_enum! {
    /// A project's status.
    pub enum ProjectStatus {
        Active = "active",
        Retired = "retired",
    }
}

bp_vault::closed_enum! {
    /// A design's status.
    pub enum DesignStatus {
        Draft = "draft",
        Active = "active",
        Deprecated = "deprecated",
        Retired = "retired",
    }
}

impl ProjectStatus {
    /// The moves `transition` makes (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] = &[(Self::Active, Self::Retired)];
}

impl DesignStatus {
    /// The moves `transition` makes (25, design change 3). A body changes through a delta, not a status.
    pub const MOVES: &'static [(Self, Self)] = &[
        (Self::Draft, Self::Active),
        (Self::Active, Self::Deprecated),
        (Self::Active, Self::Retired),
        (Self::Deprecated, Self::Retired),
    ];
}

bp_vault::closed_enum! {
    /// A delta's state. `record` writes `proposed`; acceptance and rejection come at milestone acceptance (13).
    pub enum DeltaState {
        Proposed = "proposed",
        Accepted = "accepted",
        Rejected = "rejected",
    }
}

bp_vault::closed_enum! {
    /// A concern answer's state (13, ConcernAnswer).
    pub enum ConcernState {
        Designed = "designed",
        Waived = "waived",
        Open = "open",
    }
}

bp_vault::closed_enum! {
    /// A project's class, which selects its concerns. The bundle will ship the list (13, reference); until then,
    /// the converted vault's values.
    pub enum ProjectClass {
        AgentFramework = "agent-framework",
        DataMlPlatform = "data-ml-platform",
        SmallCli = "small-cli",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_closed_sets() {
        assert_eq!("active".parse::<ProjectStatus>().map(ProjectStatus::as_str), Ok("active"));
        assert!("nope".parse::<ProjectStatus>().is_err());
        assert_eq!("draft".parse::<DesignStatus>().map(DesignStatus::as_str), Ok("draft"));
        assert!("nope".parse::<DesignStatus>().is_err());
    }
}
