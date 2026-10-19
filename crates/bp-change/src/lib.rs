//! Change domain: milestone, issue, check rows, run, claim, review, disposition, waiver; the Intents function.
//! This milestone holds the status sets and the issue's severity of the logical data model (13), and the `Intents`
//! function (21).

pub mod intents;

bp_vault::closed_enum! {
    /// A milestone's status. Running is derived from a claim under it.
    pub enum MilestoneStatus {
        Planned = "planned",
        Approved = "approved",
        Done = "done",
        Abandoned = "abandoned",
    }
}

bp_vault::closed_enum! {
    /// An issue's status. Escalation is a claim release reason.
    pub enum IssueStatus {
        Open = "open",
        InProgress = "in-progress",
        Resolved = "resolved",
        Abandoned = "abandoned",
    }
}

bp_vault::closed_enum! {
    /// An issue's severity: a label `set` writes. The four values the converted vault holds.
    pub enum Severity {
        Low = "low",
        Medium = "medium",
        High = "high",
        Critical = "critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_closed_sets() {
        assert_eq!("planned".parse::<MilestoneStatus>().map(MilestoneStatus::as_str), Ok("planned"));
        assert!("nope".parse::<MilestoneStatus>().is_err());
        assert_eq!("open".parse::<IssueStatus>().map(IssueStatus::as_str), Ok("open"));
        assert!("nope".parse::<IssueStatus>().is_err());
    }
}
