//! The closed kind set and each kind's path pattern (12, the layout rule; 13, the entity classes). The kind decides
//! the class, node or record, and the path must fit the kind.

use crate::Id;

crate::closed_enum! {
    /// Every kind the vault stores.
    pub enum Kind {
        Project = "project",
        Design = "design",
        Milestone = "milestone",
        Issue = "issue",
        Thread = "thread",
        Decision = "decision",
        Convention = "convention",
        Learning = "learning",
        Inbox = "inbox",
        Claim = "claim",
        Run = "run",
        Review = "review",
        Disposition = "disposition",
        Delta = "delta",
        ConcernAnswer = "concern-answer",
        Waiver = "waiver",
        Handoff = "handoff",
        Evidence = "evidence",
        BlueprintVersion = "blueprint-version",
    }
}

impl Kind {
    /// A record: append-only evidence under a parent. Every other kind is a node.
    pub fn is_record(self) -> bool {
        matches!(
            self,
            Self::Claim
                | Self::Run
                | Self::Review
                | Self::Disposition
                | Self::Delta
                | Self::ConcernAnswer
                | Self::Waiver
                | Self::Handoff
                | Self::Evidence
                | Self::BlueprintVersion
        )
    }

    fn dir(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Design => "designs",
            Self::Milestone => "milestones",
            Self::Issue => "issues",
            Self::Thread => "threads",
            Self::Decision => "decisions",
            Self::Convention => "conventions",
            Self::Learning => "learnings",
            Self::Inbox => "inbox",
            Self::Claim => "claims",
            Self::Run => "runs",
            Self::Review => "reviews",
            Self::Disposition => "dispositions",
            Self::Delta => "deltas",
            Self::ConcernAnswer => "concerns",
            Self::Waiver => "waivers",
            Self::Handoff => "handoffs",
            Self::Evidence => "evidence",
            Self::BlueprintVersion => "versions",
        }
    }

    /// The parent a record of this kind at `id` must name in `under`: the node whose directory holds it, or the
    /// project node for the project's own records. `None` for nodes and for a shared handoff.
    pub fn parent(self, id: &Id) -> Option<Id> {
        let p: Vec<&str> = id.as_str().split('/').collect();
        match self {
            Self::Handoff | Self::ConcernAnswer | Self::BlueprintVersion => {
                (p[0] == "projects").then(|| Id::from_trusted(format!("projects/{}/project", p[1])))
            }
            k if k.is_record() => Some(Id::from_trusted(p[..p.len() - 2].join("/"))),
            _ => None,
        }
    }

    /// The path fits this kind's pattern.
    pub fn fits(self, id: &Id) -> bool {
        let p: Vec<&str> = id.as_str().split('/').collect();
        let project = |len: usize| p.len() == len && p[0] == "projects";
        let shared = |len: usize| p.len() == len && p[0] == "shared";
        let dir = self.dir();
        match self {
            Self::Project => project(3) && p[2] == dir,
            Self::Design => {
                (project(4) && p[2] == dir)
                    || (project(5) && p[2] == dir && matches!(p[3], "containers" | "components"))
            }
            Self::Issue | Self::Milestone | Self::ConcernAnswer | Self::BlueprintVersion => project(4) && p[2] == dir,
            Self::Decision | Self::Thread | Self::Inbox | Self::Handoff => {
                (project(4) && p[2] == dir) || (shared(3) && p[1] == dir)
            }
            Self::Convention | Self::Learning => shared(3) && p[1] == dir,
            Self::Disposition => {
                p.len() > 2 && p[p.len() - 2] == dir && Self::Review.fits(&Id::from_trusted(p[..p.len() - 2].join("/")))
            }
            _ => {
                let parent = Id::from_trusted(p[..p.len().saturating_sub(2)].join("/"));
                p.len() > 2 && p[p.len() - 2] == dir && Self::ALL.iter().any(|k| !k.is_record() && k.fits(&parent))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fits(kind: Kind, id: &str) -> bool {
        kind.fits(&Id::parse(id).unwrap())
    }

    #[test]
    fn nodes_fit_their_layout() {
        assert!(fits(Kind::Project, "projects/bp/project"));
        assert!(fits(Kind::Issue, "projects/bp/issues/0411.x"));
        assert!(!fits(Kind::Issue, "shared/issues/0411.x"), "an issue requires a project");
        assert!(fits(Kind::Design, "projects/bp/designs/system"));
        assert!(fits(Kind::Design, "projects/bp/designs/components/walk"));
        assert!(!fits(Kind::Design, "projects/bp/designs/other/walk"));
        assert!(fits(Kind::Decision, "shared/decisions/d") && fits(Kind::Decision, "projects/bp/decisions/d"));
        assert!(fits(Kind::Learning, "shared/learnings/l") && !fits(Kind::Learning, "projects/bp/learnings/l"));
        assert!(!fits(Kind::Issue, "projects/bp/milestones/m"), "the directory names the kind");
    }

    #[test]
    fn records_fit_under_a_node_or_their_project() {
        assert!(fits(Kind::Claim, "projects/bp/issues/0411.x/claims/s"));
        assert!(fits(Kind::Run, "projects/bp/designs/components/walk/runs/r"));
        assert!(!fits(Kind::Claim, "projects/bp/issues/0411.x/runs/s"));
        assert!(!fits(Kind::Claim, "projects/bp/issues/claims/s"), "the parent must be a node");
        assert!(fits(Kind::Disposition, "projects/bp/issues/0411.x/reviews/h/dispositions/1"));
        assert!(fits(Kind::Handoff, "projects/bp/handoffs/2026-05-31-7c08a6ac"));
        assert!(fits(Kind::Handoff, "shared/handoffs/2026-05-31-7c08a6ac"));
        let claim = Id::parse("projects/bp/issues/0411.x/claims/s").unwrap();
        assert_eq!(Kind::Claim.parent(&claim).unwrap().as_str(), "projects/bp/issues/0411.x");
        let handoff = Id::parse("projects/bp/handoffs/h").unwrap();
        assert_eq!(Kind::Handoff.parent(&handoff).unwrap().as_str(), "projects/bp/project");
        assert_eq!(Kind::Handoff.parent(&Id::parse("shared/handoffs/h").unwrap()), None);
        assert_eq!(Kind::Issue.parent(&Id::parse("projects/bp/issues/x").unwrap()), None);
    }

    #[test]
    fn the_class_follows_the_kind() {
        assert!(Kind::Handoff.is_record() && !Kind::Issue.is_record());
        assert_eq!("concern-answer".parse::<Kind>().unwrap(), Kind::ConcernAnswer);
        assert!("session".parse::<Kind>().is_err());
    }
}
