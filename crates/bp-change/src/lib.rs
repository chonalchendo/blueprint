//! Change domain: milestone, issue, check rows, run, claim, review, disposition, waiver; the Intents function.
//! This milestone holds the status sets of 13; Intents lands with its issue.

pub use bp_vault;

/// A milestone's status (13). Running is derived from a claim under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum MilestoneStatus {
    Planned,
    Approved,
    Done,
    Abandoned,
}

impl std::str::FromStr for MilestoneStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "planned" => Ok(Self::Planned),
            "approved" => Ok(Self::Approved),
            "done" => Ok(Self::Done),
            "abandoned" => Ok(Self::Abandoned),
            _ => Err(format!("{s} is not a milestone status")),
        }
    }
}

/// An issue's status (13). Escalation is a claim release reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum IssueStatus {
    Open,
    InProgress,
    Resolved,
    Abandoned,
}

impl std::str::FromStr for IssueStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "open" => Ok(Self::Open),
            "in-progress" => Ok(Self::InProgress),
            "resolved" => Ok(Self::Resolved),
            "abandoned" => Ok(Self::Abandoned),
            _ => Err(format!("{s} is not a issue status")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_closed_sets() {
        assert!("planned".parse::<MilestoneStatus>().is_ok());
        assert!("nope".parse::<MilestoneStatus>().is_err());
        assert!("open".parse::<IssueStatus>().is_ok());
        assert!("nope".parse::<IssueStatus>().is_err());
    }
}
