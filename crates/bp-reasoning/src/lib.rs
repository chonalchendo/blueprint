//! Reasoning domain: thread, decision, convention, learning, evidence rows, inbox entry.
//! This milestone holds the status sets of 13.

pub use bp_vault;

/// A thread's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ThreadStatus {
    Open,
    Paused,
    Closed,
}

impl std::str::FromStr for ThreadStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "open" => Ok(Self::Open),
            "paused" => Ok(Self::Paused),
            "closed" => Ok(Self::Closed),
            _ => Err(format!("{s} is not a thread status")),
        }
    }
}

/// A decision's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Superseded,
    Rejected,
}

impl std::str::FromStr for DecisionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "proposed" => Ok(Self::Proposed),
            "accepted" => Ok(Self::Accepted),
            "superseded" => Ok(Self::Superseded),
            "rejected" => Ok(Self::Rejected),
            _ => Err(format!("{s} is not a decision status")),
        }
    }
}

/// A convention's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ConventionStatus {
    Active,
    Retired,
}

impl std::str::FromStr for ConventionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "active" => Ok(Self::Active),
            "retired" => Ok(Self::Retired),
            _ => Err(format!("{s} is not a convention status")),
        }
    }
}

/// A learning's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum LearningStatus {
    Draft,
    Verified,
    Stale,
    Retracted,
}

impl std::str::FromStr for LearningStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "draft" => Ok(Self::Draft),
            "verified" => Ok(Self::Verified),
            "stale" => Ok(Self::Stale),
            "retracted" => Ok(Self::Retracted),
            _ => Err(format!("{s} is not a learning status")),
        }
    }
}

/// An inbox entry's status (13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum InboxStatus {
    Raw,
    Promoted,
    Dropped,
}

impl std::str::FromStr for InboxStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "raw" => Ok(Self::Raw),
            "promoted" => Ok(Self::Promoted),
            "dropped" => Ok(Self::Dropped),
            _ => Err(format!("{s} is not a inbox status")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_closed_sets() {
        assert!("open".parse::<ThreadStatus>().is_ok());
        assert!("nope".parse::<ThreadStatus>().is_err());
        assert!("proposed".parse::<DecisionStatus>().is_ok());
        assert!("nope".parse::<DecisionStatus>().is_err());
        assert!("active".parse::<ConventionStatus>().is_ok());
        assert!("nope".parse::<ConventionStatus>().is_err());
        assert!("draft".parse::<LearningStatus>().is_ok());
        assert!("nope".parse::<LearningStatus>().is_err());
        assert!("raw".parse::<InboxStatus>().is_ok());
        assert!("nope".parse::<InboxStatus>().is_err());
    }
}
