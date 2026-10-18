//! Design domain: project, design, design kinds, concern answers, terms, diagrams, deltas, blueprint versions.
//! This milestone holds the status sets of the logical data model (13).

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
