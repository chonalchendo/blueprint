//! The per-kind schema rules that live in the domain crates, composed for the census and for `set` (23, rule 2).

use bp_change::{IssueStatus, MilestoneStatus, Severity};
use bp_design::{DesignStatus, ProjectClass, ProjectStatus};
use bp_reasoning::{ConventionStatus, DecisionStatus, InboxStatus, LearningStatus, ThreadStatus};
use bp_vault::{Code, Id, Kind, LINKS, Refusal};
use serde_json::Value;

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

/// Every label field of any kind, for the fix that lists one kind's.
const LABELS: [&str; 9] =
    ["title", "tags", "applies", "class", "repository", "forge", "cue", "severity", "walkthrough"];

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
        (_, "title") | (Kind::Project, "repository" | "forge") | (Kind::Learning, "cue") => Vocab::Text,
        (_, "tags") | (Kind::Convention, "applies") => Vocab::Tags,
        (Kind::Project, "class") => closed(ProjectClass::ALL, ProjectClass::as_str),
        (Kind::Issue, "severity") => closed(Severity::ALL, Severity::as_str),
        (Kind::Issue, "walkthrough") => Vocab::Bool,
        _ => return None,
    })
}

/// The verb that owns a field `set` must not write: a status, a link, `under`, `aliases`, a derived field, a field
/// another verb writes, or any field of a record (25, design change 2). `None` for a field outside the schema.
fn owner(kind: Kind, id: &Id, field: &str) -> Option<String> {
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
        Vocab::Text if field == "title" && value.trim().is_empty() => Err(Refusal::new(
            Code::SchemaInvalid,
            format!("{id} title"),
            "a node has a title (12, the header schema)",
            "give a title with at least one character that is not a space",
        )),
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
    }
}
