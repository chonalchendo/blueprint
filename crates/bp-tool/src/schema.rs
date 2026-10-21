//! The per-kind schema rules that live in the domain crates, composed for the census and for `set` (23, rule 2).

use bp_change::{IssueStatus, MilestoneStatus, Severity};
use bp_design::{ConcernState, DeltaState, DesignStatus, ProjectClass, ProjectStatus};
use bp_reasoning::{ConventionStatus, DecisionStatus, InboxStatus, LearningStatus, ThreadStatus};
use bp_vault::{Code, Id, Kind, LINKS, Refusal};
use serde_json::Value;

/// A node's status is in its kind's closed set. Every node carries one. A record's status is its `state`, where its
/// kind has one (13); `list --status` filters on it.
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
        Kind::Delta => s.parse::<DeltaState>().is_ok(),
        Kind::ConcernAnswer => s.parse::<ConcernState>().is_ok(),
        _ => false,
    }
}

/// The first status of a node kind's machine, which `create` writes (13). Each status set declares it first.
pub fn first_status(kind: Kind) -> Option<&'static str> {
    Some(match kind {
        Kind::Project => ProjectStatus::ALL[0].as_str(),
        Kind::Design => DesignStatus::ALL[0].as_str(),
        Kind::Milestone => MilestoneStatus::ALL[0].as_str(),
        Kind::Issue => IssueStatus::ALL[0].as_str(),
        Kind::Thread => ThreadStatus::ALL[0].as_str(),
        Kind::Decision => DecisionStatus::ALL[0].as_str(),
        Kind::Convention => ConventionStatus::ALL[0].as_str(),
        Kind::Learning => LearningStatus::ALL[0].as_str(),
        Kind::Inbox => InboxStatus::ALL[0].as_str(),
        _ => return None,
    })
}

/// What one link type of a kind names (13, each kind's table).
pub struct Link {
    /// The kinds a target may be.
    pub targets: &'static [Kind],
    /// More than one target; else zero or one.
    pub many: bool,
}

/// The link types of a node kind; `None` for a type the kind does not hold. A record holds none: its references are
/// its own fields, written once by `record` (13, rule 4).
pub fn link(kind: Kind, field: &str) -> Option<Link> {
    use Kind::{Convention, Decision, Delta, Design, Issue, Learning, Thread};
    let (targets, many): (&'static [Kind], bool) = match (kind, field) {
        (Design, "cites") => (&[Design, Decision, Convention], true),
        (Issue, "cites") => (&[Design, Convention, Learning], true),
        (Issue, "depends") => (&[Issue], true),
        (Thread, "cites") => (&[Design, Decision, Learning], true),
        (Decision, "answers") => (&[Thread], false),
        (Decision, "supersedes") => (&[Decision], false),
        (Decision, "authorizes") => (&[Delta], true),
        (Learning, "cites") => (&[Design, Issue], true),
        (Kind::Inbox, "targets") => (&[Thread, Issue, Learning], false),
        _ => return None,
    };
    Some(Link { targets, many })
}

/// The kinds a node of `kind` may sit under; it needs one when any is named (13): the spine runs product under
/// project, system under product, and so down; a milestone sits under the design it moves; an issue under its
/// milestone.
pub fn parents(kind: Kind) -> &'static [Kind] {
    match kind {
        Kind::Design => &[Kind::Design, Kind::Project],
        Kind::Milestone => &[Kind::Design],
        Kind::Issue => &[Kind::Milestone],
        _ => &[],
    }
}

/// Every label field of any kind, for the fix that lists one kind's.
const LABELS: [&str; 10] =
    ["title", "tags", "applies", "class", "repository", "forge", "reviewer", "cue", "severity", "walkthrough"];

/// What a label holds.
enum Vocab {
    /// One value of a closed set.
    Closed(Vec<&'static str>),
    /// Any text.
    Text,
    /// A list of tags, comma-separated on the command line.
    Tags,
    /// `true` or `false`.
    Bool,
}

/// The labels of a node kind (12, the header schema; 13, each kind's table): the fields `set` writes because no other
/// verb owns them. `title` is one: a rename moves the slug, never the title.
fn vocab(kind: Kind, field: &str) -> Option<Vocab> {
    fn closed<T: Copy>(all: &[T], text: fn(T) -> &'static str) -> Vocab {
        Vocab::Closed(all.iter().map(|v| text(*v)).collect())
    }
    Some(match (kind, field) {
        (_, "title") | (Kind::Project, "repository" | "forge" | "reviewer") | (Kind::Learning, "cue") => Vocab::Text,
        (_, "tags") | (Kind::Convention, "applies") => Vocab::Tags,
        (Kind::Project, "class") => closed(ProjectClass::ALL, ProjectClass::as_str),
        (Kind::Issue, "severity") => closed(Severity::ALL, Severity::as_str),
        (Kind::Issue, "walkthrough") => Vocab::Bool,
        _ => return None,
    })
}

/// The verb that owns a field `set` must not write: a status, a link, `under`, `aliases`, a derived field, a field
/// another verb writes, or any field of a record (25, design change 2). `None` for a field outside the schema.
pub fn owner(kind: Kind, id: &Id, field: &str) -> Option<String> {
    if kind.is_record() {
        let parent = kind.parent(id).map_or_else(|| "<parent>".into(), |p| p.to_string());
        let verb = match kind {
            Kind::Run => format!("bp check {parent}"),
            Kind::Claim => format!("bp release {parent} --reason <reason>"),
            Kind::Waiver => format!("bp stamp {parent} --waive <predicate> --reason <why>"),
            Kind::BlueprintVersion => "bp stamp <milestone>".into(),
            _ => format!("bp record {kind} {parent}"),
        };
        return Some(format!("a record is never edited (13, rule 6); a correction is a new one: {verb}"));
    }
    Some(match (kind, field) {
        (_, "status") => format!("bp transition {id} <status>"),
        (_, "kind") => format!("the path names the kind: bp create <kind>, then retire {id}"),
        (_, "under") => "`under` changes only by a re-parenting verb, not yet built (15, Open)".into(),
        (_, "aliases") => format!("bp rename {id} <slug> keeps the old slug as an alias"),
        (Kind::Design, "scope" | "design_kind") => "the path names it (12, Layout); rename moves a design".into(),
        (_, "carries") => "`carries` derives from the diagram sidecars and check blocks (13); edit the body".into(),
        (_, f) if LINKS.contains(&f) => format!("bp link {id} {f} <id>, or bp link {id} {f} <id> --remove"),
        (Kind::Learning, "confidence") => {
            format!("confidence derives from evidence rows (13): bp record evidence {id}")
        }
        (Kind::Decision, "superseded_by") => format!("`superseded_by` derives (13): bp link <newer> supersedes {id}"),
        (Kind::Decision, "decided_at") => {
            format!("the decision time is written with the decision: bp transition {id} accepted")
        }
        (Kind::Milestone, "approved_by") => {
            format!("the approval lives on the forge (13): bp transition {id} approved")
        }
        _ => return None,
    })
}

/// The title rule `set` and `create` share: a node has a title with at least one character that is not a space.
pub fn title(subject: &str, value: &str) -> Result<(), Refusal> {
    if value.trim().is_empty() {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            subject,
            "a node has a title (12, the header schema)",
            "give a title with at least one character that is not a space",
        ));
    }
    Ok(())
}

/// The value `set` writes to `field` of the node `id` of `kind`. Refuses `not_a_label` with the owning verb,
/// `schema_invalid` for a field outside the kind's schema, and `unknown_value` for a value outside a closed set.
pub fn label(kind: Kind, id: &Id, field: &str, value: &str) -> Result<Value, Refusal> {
    let Some(vocab) = vocab(kind, field).filter(|_| !kind.is_record()) else {
        if let Some(fix) = owner(kind, id, field) {
            return Err(Refusal::new(
                Code::NotALabel,
                format!("{id} {field}"),
                "set writes one label; a status, a link, under, aliases, a derived field and a record field each have \
                 their own verb (15, set)",
                fix,
            ));
        }
        let labels: Vec<&str> = LABELS.into_iter().filter(|f| vocab(kind, f).is_some()).collect();
        return Err(Refusal::new(
            Code::SchemaInvalid,
            format!("{id} {field}"),
            "a header holds only its kind's fields (12, the kind schemas)",
            format!("the labels of kind {kind} are: {}", labels.join(", ")),
        ));
    };
    let outside = |values: &[&str]| {
        Refusal::new(
            Code::UnknownValue,
            format!("{field} {value}"),
            "a label with a closed set takes one of its values (13)",
            format!("use one of: {}", values.join(", ")),
        )
    };
    match vocab {
        Vocab::Closed(values) if values.contains(&value) => Ok(value.into()),
        Vocab::Closed(values) => Err(outside(&values)),
        Vocab::Text if field == "title" => title(&format!("{id} title"), value).map(|()| value.into()),
        Vocab::Text => Ok(value.into()),
        Vocab::Tags => {
            let mut tags: Vec<&str> = Vec::new();
            for tag in value.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                if !tag.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
                    || !tag.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '/')
                {
                    return Err(Refusal::new(
                        Code::SchemaInvalid,
                        format!("{field} {tag}"),
                        "a tag is lowercase letters, digits, `-` and `/` (13)",
                        format!("rewrite {tag:?} in that shape, for example domain/write-path"),
                    ));
                }
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
            Ok(tags.into_iter().map(Value::from).collect())
        }
        Vocab::Bool => value.parse::<bool>().map(Value::from).map_err(|_| outside(&["true", "false"])),
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
        assert!(status_in_set("concern-answer", Some("open")) && status_in_set("delta", Some("proposed")));
    }

    #[test]
    fn a_node_is_born_in_its_machine_s_first_status() {
        let first: Vec<_> = [Kind::Project, Kind::Design, Kind::Milestone, Kind::Issue, Kind::Decision, Kind::Inbox]
            .into_iter()
            .map(first_status)
            .collect();
        assert_eq!(first, ["active", "draft", "planned", "open", "proposed", "raw"].map(Some));
        assert_eq!(first_status(Kind::Claim), None);
    }
}
