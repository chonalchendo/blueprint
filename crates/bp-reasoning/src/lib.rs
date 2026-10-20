//! Reasoning domain: thread, decision, convention, learning, evidence rows, inbox entry.
//! This milestone holds the status sets, their machines (25, design change 3), and the evidence outcomes of the
//! logical data model (13).

bp_vault::closed_enum! {
    /// A thread's status.
    pub enum ThreadStatus {
        Open = "open",
        Paused = "paused",
        Closed = "closed",
    }
}

bp_vault::closed_enum! {
    /// A decision's status.
    pub enum DecisionStatus {
        Proposed = "proposed",
        Accepted = "accepted",
        Superseded = "superseded",
        Rejected = "rejected",
    }
}

bp_vault::closed_enum! {
    /// A convention's status.
    pub enum ConventionStatus {
        Active = "active",
        Retired = "retired",
    }
}

bp_vault::closed_enum! {
    /// A learning's status. `stale` is derived by the walk, never stored.
    pub enum LearningStatus {
        Draft = "draft",
        Verified = "verified",
        Retracted = "retracted",
    }
}

bp_vault::closed_enum! {
    /// An inbox entry's status.
    pub enum InboxStatus {
        Raw = "raw",
        Promoted = "promoted",
        Dropped = "dropped",
    }
}

impl ThreadStatus {
    /// The moves `transition` makes (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] = &[
        (Self::Open, Self::Paused),
        (Self::Paused, Self::Open),
        (Self::Open, Self::Closed),
        (Self::Paused, Self::Closed),
    ];
}

impl DecisionStatus {
    /// The moves `transition` makes (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] =
        &[(Self::Proposed, Self::Accepted), (Self::Proposed, Self::Rejected), (Self::Accepted, Self::Superseded)];

    /// A move that is the human's decision, so it writes `decided_at` (13, Decision).
    pub fn decides(to: Self) -> bool {
        matches!(to, Self::Accepted | Self::Rejected)
    }
}

impl ConventionStatus {
    /// The moves `transition` makes (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] = &[(Self::Active, Self::Retired)];
}

impl LearningStatus {
    /// The moves `transition` makes (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] =
        &[(Self::Draft, Self::Verified), (Self::Draft, Self::Retracted), (Self::Verified, Self::Retracted)];

    /// The derived `stale` (13, Learning): a design the learning cites changed version after its last evidence row.
    /// `last` is that row's `at`, and `changed` holds each cited design's latest `accepted_at`; both are UTC with `Z`,
    /// so text order is time order. A learning with no row has no verification to age, and a retracted one is out of
    /// use.
    pub fn stale(status: &str, last: Option<&str>, changed: &[String]) -> bool {
        status != Self::Retracted.as_str() && last.is_some_and(|l| changed.iter().any(|c| c.as_str() > l))
    }
}

impl InboxStatus {
    /// The moves `transition` makes (25, design change 3).
    pub const MOVES: &'static [(Self, Self)] = &[(Self::Raw, Self::Promoted), (Self::Raw, Self::Dropped)];
}

bp_vault::closed_enum! {
    /// What applying a learning did (13, EvidenceRow).
    pub enum Outcome {
        Helped = "helped",
        Hurt = "hurt",
        NoEffect = "no-effect",
        Confounded = "confounded",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_are_closed_sets() {
        assert_eq!("open".parse::<ThreadStatus>().map(ThreadStatus::as_str), Ok("open"));
        assert!("nope".parse::<ThreadStatus>().is_err());
        assert_eq!("proposed".parse::<DecisionStatus>().map(DecisionStatus::as_str), Ok("proposed"));
        assert!("nope".parse::<DecisionStatus>().is_err());
        assert_eq!("active".parse::<ConventionStatus>().map(ConventionStatus::as_str), Ok("active"));
        assert!("nope".parse::<ConventionStatus>().is_err());
        assert_eq!("draft".parse::<LearningStatus>().map(LearningStatus::as_str), Ok("draft"));
        assert!("nope".parse::<LearningStatus>().is_err());
        assert_eq!("raw".parse::<InboxStatus>().map(InboxStatus::as_str), Ok("raw"));
        assert!("nope".parse::<InboxStatus>().is_err());
    }

    #[test]
    fn a_learning_is_stale_when_a_cited_design_changed_after_its_last_evidence() {
        let changed = ["2026-10-20T10:00:00Z".to_string()];
        assert!(LearningStatus::stale("verified", Some("2026-10-19T10:00:00Z"), &changed));
        assert!(
            !LearningStatus::stale("verified", Some("2026-10-21T10:00:00Z"), &changed),
            "evidence after the change"
        );
        assert!(!LearningStatus::stale("draft", None, &changed), "no row, no verification to age");
        assert!(!LearningStatus::stale("retracted", Some("2026-10-19T10:00:00Z"), &changed), "out of use");
        assert!(!LearningStatus::stale("verified", Some("2026-10-19T10:00:00Z"), &[]), "no design changed");
    }
}
