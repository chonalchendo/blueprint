//! Design domain: project, design, design kinds, concern answers, terms, diagrams, deltas, blueprint versions.
//! This milestone holds the status sets of 13; the rest lands with its milestones.

pub use bp_vault;

/// A project's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ProjectStatus {
    Active,
    Retired,
}

impl std::str::FromStr for ProjectStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "active" => Ok(Self::Active),
            "retired" => Ok(Self::Retired),
            _ => Err(format!("{s} is not a project status")),
        }
    }
}

/// A design's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DesignStatus {
    Draft,
    Active,
    Deprecated,
    Retired,
}

impl std::str::FromStr for DesignStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "deprecated" => Ok(Self::Deprecated),
            "retired" => Ok(Self::Retired),
            _ => Err(format!("{s} is not a design status")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_closed_sets() {
        assert!("active".parse::<ProjectStatus>().is_ok());
        assert!("nope".parse::<ProjectStatus>().is_err());
        assert!("draft".parse::<DesignStatus>().is_ok());
        assert!("nope".parse::<DesignStatus>().is_err());
    }
}
