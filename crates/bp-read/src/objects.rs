//! The objects `list`, `show` and `search` print, and the view serves (32, design changes 1 and 2). Each is typed, and
//! `ts-rs` exports it to the client. A verb prints one through `serde_json::to_value`, so its keys come out sorted, as
//! the envelopes printed before. A field the JSON skips is optional in TypeScript (`?`), not `null`.

use std::collections::{BTreeMap, BTreeSet};

use bp_change::intents::Intent;
use bp_change::{IssueStatus, MilestoneStatus};
use bp_design::{ConcernState, DeltaState, DesignStatus, ProjectStatus};
use bp_reasoning::{ConventionStatus, DecisionStatus, InboxStatus, LearningStatus, ThreadStatus};
use bp_vault::{Code, Counts, Filter, Finding, Hit, Id, Index, Kind, Refusal, Row, Vault};
use serde::Serialize;
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::Intents;

/// The rows of `list <kind>`: each with its intent where its kind has one, and the forge the intents read.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct List {
    /// The rows, at most the limit.
    pub rows: Vec<ListRow>,
    /// Every row the filters match.
    pub total: usize,
    /// `total` holds more rows than `rows`.
    pub truncated: bool,
    /// The forge the issues' intents read; only an issue's intent reads it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge: Option<Forge>,
    /// The seconds since the oldest forge read in view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_age: Option<i64>,
}

/// One row of `list`: the index's row, a learning's `stale`, and the intent with its note.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct ListRow {
    /// The index's row.
    #[serde(flatten)]
    pub row: Row,
    /// A learning's derived `stale` (13, Learning).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<bool>,
    /// The first true predicate of 21, for an issue, a milestone or a design.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    /// What the intent names; present with the intent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Where the intents read the forge from.
#[derive(Serialize, TS)]
pub enum Forge {
    /// The snapshot a repo verb wrote, or `BP_FORGE_SNAPSHOT`.
    #[serde(rename = "snapshot")]
    Snapshot,
    /// No snapshot exists for the projects in view.
    #[serde(rename = "not read")]
    NotRead,
}

/// The result of `list --count-only`: the counts, and the findings the reader keeps instead of ignoring.
#[derive(Serialize, TS)]
pub struct Census {
    /// Files, nodes per kind, records per kind, check blocks, and the files that do not parse.
    #[serde(flatten)]
    pub counts: Counts,
    /// Statuses outside their kind's set, link targets that are not ids, and each body finding kind.
    pub findings: BTreeMap<String, usize>,
}

/// One node or record, as `show` prints it.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Show {
    /// The path id.
    pub id: String,
    /// The kind is a record kind.
    pub record: bool,
    /// The blake3 hash of the file.
    pub hash: String,
    /// The header as the file holds it: the universal fields, the links and every other field.
    #[serde(flatten)]
    pub header: Map<String, Value>,
    /// A learning's derived `stale`; a past commit's learning has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<bool>,
    /// The text before the first `## ` heading.
    pub lead: String,
    /// What the body does that the vault should not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Finding>>")]
    pub findings: Vec<Finding>,
    /// The one section asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The section headings, when no one section is asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<String>>,
    /// The body, unless it is left out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// The rows of `search`.
#[derive(Serialize, TS)]
pub struct Search {
    /// The hits, best first.
    pub rows: Vec<Hit>,
}

/// The rows `filter` matches, with intents for an issue, a milestone, a design or a learning. `ready` keeps only the
/// open issues whose intent is `ready`, then limits.
pub fn list(vault: &Vault, index: &Index, filter: &Filter<'_>, ready: bool) -> Result<List, Refusal> {
    let kind = filter.kind;
    let read = Filter {
        kind,
        // `ready` reads every open issue, then limits after the intent filter.
        status: if ready { Some(IssueStatus::Open.as_str()) } else { filter.status },
        project: filter.project,
        under: filter.under,
        limit: if ready { crate::ALL } else { filter.limit },
    };
    let (rows, total) = index.list(&read)?;
    if !matches!(kind, Kind::Issue | Kind::Milestone | Kind::Design | Kind::Learning) {
        let rows: Vec<ListRow> =
            rows.into_iter().map(|row| ListRow { row, stale: None, intent: None, note: None }).collect();
        return Ok(List { truncated: total > rows.len(), rows, total, forge: None, forge_age: None });
    }
    // The projects in view: `project`, else the rows' own; a snapshot outside them is not read (27, design change 9).
    let projects: BTreeSet<&str> = match filter.project {
        Some(p) => BTreeSet::from([p]),
        None => rows.iter().filter_map(|r| crate::split(&r.id)).map(|(p, _)| p).collect(),
    };
    let intents = Intents::new(vault, index, (kind == Kind::Issue).then_some(&projects))?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let verdict = intents.row(kind, &row.id, row.status.as_deref())?;
        if ready && verdict.intent != Some(Intent::Ready) {
            continue;
        }
        let stale = match kind {
            Kind::Learning => Some(intents.stale(&row.id, &index.header(&row.id)?.unwrap_or_default())?),
            _ => None,
        };
        let note = verdict.intent.map(|_| verdict.note);
        out.push(ListRow { row, stale, intent: verdict.intent, note });
    }
    let total = if ready { out.len() } else { total };
    out.truncate(filter.limit);
    let (forge, forge_age) = match intents.world.as_ref().map(crate::World::forge_age) {
        Some(Some(age)) => (Some(Forge::Snapshot), Some(age)),
        Some(None) => (Some(Forge::NotRead), None),
        None => (None, None),
    };
    Ok(List { truncated: total > out.len(), rows: out, total, forge, forge_age })
}

/// The counts after the index takes in every file, and the findings: statuses outside their kind's set, link
/// targets that are not ids, the body findings, and files that do not parse.
pub fn census(vault: &Vault, index: &mut Index) -> Result<Census, Refusal> {
    index.reconcile(vault)?;
    let outside: usize = index
        .statuses()?
        .iter()
        .filter(|(kind, status, _)| !status_in_set(kind, status.as_deref()))
        .map(|(_, _, n)| n)
        .sum();
    let mut findings = BTreeMap::from([
        ("status_outside_set".to_string(), outside),
        ("unresolved_link".to_string(), index.unresolved()?),
    ]);
    findings.extend(index.findings()?.into_iter().map(|(kind, n)| (kind.to_string(), n)));
    Ok(Census { counts: index.counts()?, findings })
}

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

/// The node or record `id`: its header, lead and findings, then one `section`, or the headings and, unless `no_body`,
/// the body. `at` reads the file at a commit; `index` is today's, and `None` for a past commit.
pub fn show(
    vault: &Vault,
    index: Option<&Index>,
    id: &Id,
    at: Option<&str>,
    section: Option<&str>,
    no_body: bool,
) -> Result<Show, Refusal> {
    let file = match at {
        Some(at) => vault.read_at(id, at)?,
        None => vault.read(id)?,
    };
    // A refusal's fix reads the same commit the verb read.
    let at_flag = at.map_or_else(String::new, |a| format!(" --at {a}"));
    let (Some(head), Some(header)) = (&file.header, file.header_json()) else {
        return Err(Refusal::new(
            Code::NoNode,
            id.as_str(),
            "an id names a node or record: a file with a header",
            match at {
                Some(a) => format!(
                    "the file has no header: it is an asset; git -C {} show {a}:./{id}.md",
                    vault.root.display()
                ),
                None => format!("the file has no header: it is an asset; cat {}", id.path(&vault.root).display()),
            },
        ));
    };
    // `stale` is today's state, so a past commit's learning has none (13, Learning).
    let stale = match index.filter(|_| head.kind == Kind::Learning) {
        Some(index) => Some(Intents::new(vault, index, None)?.stale(id.as_str(), &header)?),
        None => None,
    };
    let mut shown = Show {
        id: id.to_string(),
        record: head.kind.is_record(),
        hash: file.hash.clone(),
        header,
        stale,
        lead: file.body.lead.clone(),
        findings: file.body.findings.clone(),
        section: None,
        sections: None,
        body: None,
    };
    let headings: Vec<&str> = file.body.sections.iter().map(|s| s.heading.as_str()).collect();
    let Some(heading) = section else {
        shown.sections = Some(headings.into_iter().map(str::to_string).collect());
        shown.body = (!no_body).then(|| file.body.text.clone());
        return Ok(shown);
    };
    let refuse = |fix: String| {
        Refusal::new(
            Code::NoSection,
            format!("{id}#{heading}"),
            "a section is one top-level `## ` heading of the body, matched as written",
            fix,
        )
    };
    match file.body.sections_named(heading)[..] {
        [one] => shown.section = Some(one.text.clone()),
        [] => {
            let mut unique: Vec<&str> = Vec::new();
            for h in &headings {
                if !unique.contains(h) {
                    unique.push(h);
                }
            }
            return Err(refuse(format!("bp show {id}{at_flag} --no-body; the headings are: {}", unique.join(", "))));
        }
        // Two sections share the heading: the whole body is the only read that holds both.
        _ => return Err(refuse(format!("bp show {id}{at_flag} --json"))),
    }
    Ok(shown)
}

/// BM25 over ids, titles and bodies, as `search` reads it.
pub fn search(
    index: &Index,
    terms: &[String],
    kind: Option<Kind>,
    project: Option<&str>,
    limit: usize,
) -> Result<Search, Refusal> {
    Ok(Search { rows: index.search(terms, kind, project, limit)? })
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
}
