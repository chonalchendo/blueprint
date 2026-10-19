//! Design domain: project, design, design kinds, concern answers, terms, diagrams, deltas, blueprint versions.
//! This milestone holds the status sets and the label vocabularies of the logical data model (13).

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

bp_vault::closed_enum! {
    /// A design's C4 level.
    pub enum Scope {
        Product = "product",
        System = "system",
        Container = "container",
        Component = "component",
    }
}

bp_vault::closed_enum! {
    /// The concern a design answers. The bundle will ship the list (13, reference); until then, the converted
    /// vault's values and the glossary of the spine (12, Identity).
    pub enum DesignKind {
        Product = "product",
        System = "system",
        DataModel = "data-model",
        Glossary = "glossary",
        Container = "container",
        Component = "component",
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
