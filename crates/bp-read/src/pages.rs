//! The page objects the view serves beside the verbs' own (32, design change 2). Each composes `list` reads, so a
//! row on a page is the row `list` prints, intent and note included. A field the view needs past the verb's row sits
//! on the page's own row type, so the verbs' envelopes do not change.

use std::collections::BTreeMap;

use bp_change::intents::Intent;
use bp_vault::{Code, Filter, Index, Kind, Refusal, Vault};
use serde::Serialize;
use ts_rs::TS;

use crate::{ALL, Forge, ListRow, list};

/// A project's home (18, Project home): what it holds by kind, its design tree, its milestones, and what moves in it
/// by intent.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Home {
    /// The project node's id.
    pub id: String,
    /// The project's newest blueprint version tag; absent before its first stamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Nodes per kind, a kind the project lacks left out.
    pub nodes: BTreeMap<String, usize>,
    /// Records per kind, a kind the project lacks left out.
    pub records: BTreeMap<String, usize>,
    /// The `list design` rows, each with its diagrams and its accepted version.
    pub designs: Vec<HomeDesign>,
    /// The conventions the product design cites, which stand beside the tree (18, Project home).
    pub conventions: Vec<String>,
    /// Every `list milestone` row, any status, each with its issues.
    pub milestones: Vec<HomeMilestone>,
    /// The project's nodes per intent: issues, milestones and designs, from the `Intents` calls `list` makes.
    pub intents: BTreeMap<Intent, usize>,
    /// The forge the issues' intents read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge: Option<Forge>,
    /// The seconds since the oldest forge read in view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_age: Option<i64>,
}

/// A design row of the home's tree.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct HomeDesign {
    /// The `list design` row.
    #[serde(flatten)]
    pub row: ListRow,
    /// The rendered diagrams in the design's `diagrams/` directory (28, design change 1).
    pub diagrams: usize,
    /// The newest version tag whose manifest holds the design's current hash; absent when none does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted: Option<String>,
}

/// A milestone row of the home's loop pane.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct HomeMilestone {
    /// The `list milestone` row.
    #[serde(flatten)]
    pub row: ListRow,
    /// The issues under it: a live one counted by its intent, any other by its status.
    pub issues: BTreeMap<String, usize>,
}

/// The view's root (18, Projects): the `list project` rows, each with its version and what moves in it.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Projects {
    /// One row per project.
    pub rows: Vec<ProjectRow>,
}

/// One project on the root page.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct ProjectRow {
    /// The `list project` row.
    #[serde(flatten)]
    pub row: ListRow,
    /// The project's newest blueprint version tag; absent before its first stamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The project's nodes per intent, as its home counts them.
    pub intents: BTreeMap<Intent, usize>,
}

/// A blueprint version record, reduced: its `n`, its tag, and its manifest of design id to hash.
type Version = (u64, String, serde_json::Map<String, serde_json::Value>);

/// What a project's pages read: its designs, milestones and issues as `list` prints them, and its versions.
struct Read {
    designs: Vec<ListRow>,
    milestones: Vec<ListRow>,
    issues: crate::List,
    /// The blueprint version records, newest first.
    versions: Vec<Version>,
}

impl Read {
    fn of(vault: &Vault, index: &Index, project: &str) -> Result<Self, Refusal> {
        let of = |kind| Filter { kind, status: None, project: Some(project), under: None, limit: ALL };
        let mut versions: Vec<_> = index
            .records(&format!("projects/{project}/project"), Kind::BlueprintVersion)?
            .into_iter()
            .filter_map(|r| {
                let n = r.header.get("n")?.as_u64()?;
                let tag = r.header.get("tag")?.as_str()?.to_string();
                Some((n, tag, r.header.get("manifest")?.as_object().cloned().unwrap_or_default()))
            })
            .collect();
        versions.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(Self {
            designs: list(vault, index, &of(Kind::Design), false)?.rows,
            milestones: list(vault, index, &of(Kind::Milestone), false)?.rows,
            issues: list(vault, index, &of(Kind::Issue), false)?,
            versions,
        })
    }

    fn version(&self) -> Option<String> {
        self.versions.first().map(|(_, tag, _)| tag.clone())
    }

    fn intents(&self) -> BTreeMap<Intent, usize> {
        let mut intents = BTreeMap::new();
        let rows = self.issues.rows.iter().chain(&self.milestones).chain(&self.designs);
        for intent in rows.filter_map(|r| r.intent) {
            *intents.entry(intent).or_default() += 1;
        }
        intents
    }
}

/// The refusal for a project the index lacks.
fn no_project(id: String) -> Refusal {
    Refusal::new(Code::NoNode, id, "a project is a directory under projects/ that holds project.md", "bp list project")
}

/// The home of `project`. A project the index lacks refuses `no_node`.
pub fn home(vault: &Vault, index: &Index, project: &str) -> Result<Home, Refusal> {
    let id = format!("projects/{project}/project");
    if index.header(&id)?.is_none() {
        return Err(no_project(id));
    }
    let (mut nodes, mut records) = (BTreeMap::new(), BTreeMap::new());
    for &kind in Kind::ALL {
        let (_, total) = index.list(&Filter { kind, status: None, project: Some(project), under: None, limit: 0 })?;
        if total > 0 {
            let into = if kind.is_record() { &mut records } else { &mut nodes };
            into.insert(kind.as_str().to_string(), total);
        }
    }
    let read = Read::of(vault, index, project)?;
    let (version, intents) = (read.version(), read.intents());
    let mut designs = Vec::with_capacity(read.designs.len());
    for row in read.designs {
        let accepted = accepted(vault, &row.row.id, &read.versions)?;
        designs.push(HomeDesign { diagrams: diagrams(vault, &row.row.id), accepted, row });
    }
    let product = format!("projects/{project}/designs/product");
    let conventions = index
        .header(&product)?
        .and_then(|h| h.get("cites").and_then(|c| c.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.as_str().filter(|c| c.starts_with("shared/conventions/")).map(str::to_string))
        .collect();
    let mut under: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    for issue in &read.issues.rows {
        let Some(m) = issue.row.under.as_deref() else { continue };
        // An intent serialises as its kebab-case text, the word the client prints.
        let intent = issue.intent.and_then(|i| serde_json::to_value(i).ok()?.as_str().map(str::to_string));
        let key = intent.unwrap_or_else(|| issue.row.status.clone().unwrap_or_default());
        *under.entry(m).or_default().entry(key).or_default() += 1;
    }
    let milestones = read
        .milestones
        .into_iter()
        .map(|row| HomeMilestone { issues: under.remove(row.row.id.as_str()).unwrap_or_default(), row })
        .collect();
    let (forge, forge_age) = (read.issues.forge, read.issues.forge_age);
    Ok(Home { id, version, nodes, records, designs, conventions, milestones, intents, forge, forge_age })
}

/// The root page: every project, with its version and its intents.
pub fn projects(vault: &Vault, index: &Index) -> Result<Projects, Refusal> {
    let filter = Filter { kind: Kind::Project, status: None, project: None, under: None, limit: ALL };
    let mut rows = Vec::new();
    for row in list(vault, index, &filter, false)?.rows {
        let Some((project, _)) = crate::split(&row.row.id) else { continue };
        let read = Read::of(vault, index, project)?;
        rows.push(ProjectRow { version: read.version(), intents: read.intents(), row });
    }
    Ok(Projects { rows })
}

/// The rendered diagrams of the design `id`: the `.html` files in its `diagrams/` directory.
fn diagrams(vault: &Vault, id: &str) -> usize {
    let dir = vault.root.join(id).join("diagrams");
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "html")).count()
}

/// The newest version whose manifest holds the design's current hash (32, design change 9). The file is read only
/// when a version exists.
fn accepted(vault: &Vault, id: &str, versions: &[Version]) -> Result<Option<String>, Refusal> {
    if versions.is_empty() {
        return Ok(None);
    }
    let hash = vault.read(&crate::node_id(id)?)?.hash;
    Ok(versions.iter().find(|(_, _, m)| m.get(id).and_then(|h| h.as_str()) == Some(hash.as_str())).map(|v| v.1.clone()))
}
