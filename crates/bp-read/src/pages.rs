//! The page objects the view serves beside the verbs' own (32, design change 2). Each composes `list` reads, so a
//! row on a page is the row `list` prints, intent and note included.

use std::collections::BTreeMap;

use bp_change::MilestoneStatus;
use bp_change::intents::Intent;
use bp_vault::{Code, Filter, Index, Kind, Refusal, Vault};
use serde::Serialize;
use ts_rs::TS;

use crate::{ALL, Forge, ListRow, list};

/// A project's home (18, Project home): what it holds by kind, its designs, its open milestones, and its live issues
/// by intent.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Home {
    /// The project node's id.
    pub id: String,
    /// Nodes per kind, a kind the project lacks left out.
    pub nodes: BTreeMap<String, usize>,
    /// Records per kind, a kind the project lacks left out.
    pub records: BTreeMap<String, usize>,
    /// The `list design` rows.
    pub designs: Vec<ListRow>,
    /// The `list milestone` rows that are `planned` or `approved`.
    pub milestones: Vec<ListRow>,
    /// The live issues per intent, from the `Intents` call `list issue` makes.
    pub intents: BTreeMap<Intent, usize>,
    /// The forge the issues' intents read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge: Option<Forge>,
    /// The seconds since the oldest forge read in view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_age: Option<i64>,
}

/// The home of `project`. A project the index lacks refuses `no_node`.
pub fn home(vault: &Vault, index: &Index, project: &str) -> Result<Home, Refusal> {
    let id = format!("projects/{project}/project");
    if index.header(&id)?.is_none() {
        return Err(Refusal::new(
            Code::NoNode,
            id,
            "a project is a directory under projects/ that holds project.md",
            "bp list project",
        ));
    }
    let of = |kind, status, limit| Filter { kind, status, project: Some(project), under: None, limit };
    let (mut nodes, mut records) = (BTreeMap::new(), BTreeMap::new());
    for &kind in Kind::ALL {
        let (_, total) = index.list(&of(kind, None, 0))?;
        if total > 0 {
            let into = if kind.is_record() { &mut records } else { &mut nodes };
            into.insert(kind.as_str().to_string(), total);
        }
    }
    let designs = list(vault, index, &of(Kind::Design, None, ALL), false)?.rows;
    let mut milestones = Vec::new();
    for status in [MilestoneStatus::Planned, MilestoneStatus::Approved] {
        milestones.extend(list(vault, index, &of(Kind::Milestone, Some(status.as_str()), ALL), false)?.rows);
    }
    milestones.sort_by(|a, b| a.row.id.cmp(&b.row.id));
    let issues = list(vault, index, &of(Kind::Issue, None, ALL), false)?;
    let mut intents = BTreeMap::new();
    for intent in issues.rows.iter().filter_map(|r| r.intent) {
        *intents.entry(intent).or_default() += 1;
    }
    Ok(Home { id, nodes, records, designs, milestones, intents, forge: issues.forge, forge_age: issues.forge_age })
}
