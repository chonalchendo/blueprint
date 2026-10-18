//! The per-kind schema rules that live in the domain crates, composed for the census (23, rule 2).

use bp_change::{IssueStatus, MilestoneStatus};
use bp_design::{DesignStatus, ProjectStatus};
use bp_reasoning::{ConventionStatus, DecisionStatus, InboxStatus, LearningStatus, ThreadStatus};
use bp_vault::Kind;

/// A node's status is in its kind's closed set. Every node carries one; a record carries none.
pub fn status_in_set(kind: &str, status: Option<&str>) -> bool {
    let (Ok(kind), Some(s)) = (kind.parse::<Kind>(), status) else { return false };
    match kind {
        Kind::Project => s.parse::<ProjectStatus>().is_ok(),
        Kind::Design => s.parse::<DesignStatus>().is_ok(),
        Kind::Milestone => s.parse::<MilestoneStatus>().is_ok(),
        Kind::Issue => s.parse::<IssueStatus>().is_ok(),
        Kind::Thread => s.parse::<ThreadStatus>().is_ok(),
        Kind::Decision => s.parse::<DecisionStatus>().is_ok(),
        Kind::Convention => s.parse::<ConventionStatus>().is_ok(),
        Kind::Learning => s.parse::<LearningStatus>().is_ok(),
        Kind::Inbox => s.parse::<InboxStatus>().is_ok(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_reads_its_own_set() {
        assert!(status_in_set("issue", Some("in-progress")));
        assert!(!status_in_set("milestone", Some("in-progress")), "running is derived, not stored");
        assert!(!status_in_set("learning", Some("stale")));
        assert!(!status_in_set("issue", None));
        assert!(!status_in_set("claim", Some("open")));
    }
}
