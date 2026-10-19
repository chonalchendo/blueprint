//! Change domain: milestone, issue, check rows, run, claim, review, disposition, waiver; the Intents function.
//! This milestone holds the status sets, their machines (25, design change 3), the issue's severity and the
//! disposition values of the logical data model (13), and the `Intents` function (21).

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

impl MilestoneStatus {
    /// The moves `transition` makes, each one human decision (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] = &[
        (Self::Planned, Self::Approved),
        (Self::Planned, Self::Abandoned),
        (Self::Approved, Self::Abandoned),
        (Self::Abandoned, Self::Planned),
    ];

    /// The verb that owns a move `transition` does not make: `done` waits on the finish line of an approved
    /// milestone.
    pub fn owner(from: Self, to: Self) -> Option<&'static str> {
        (from == Self::Approved && to == Self::Done).then_some("stamp")
    }
}

impl IssueStatus {
    /// The moves `transition` makes, each one human decision (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] = &[(Self::Open, Self::Abandoned), (Self::Abandoned, Self::Open)];

    /// The verb that owns a move `transition` does not make: the loop verbs, which also write the claim. Each owns
    /// one edge, so a fix never names a verb that cannot run from the current status.
    pub fn owner(from: Self, to: Self) -> Option<&'static str> {
        match (from, to) {
            (Self::Open, Self::InProgress) => Some("cut"),
            (Self::InProgress, Self::Resolved) => Some("land"),
            (Self::InProgress, Self::Open) => Some("release"),
            _ => None,
        }
    }
}

bp_vault::closed_enum! {
    /// The worker's answer to one review finding (13, Disposition).
    pub enum DispositionAs {
        Fixed = "fixed",
        Declined = "declined",
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
