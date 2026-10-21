//! The spec (28, design change 1): YAML in, a typed spec out, or the findings that name each bad field. The agent
//! gives cells and order, never pixels.

use yaml_rust2::{Yaml, YamlLoader};

use crate::Finding;

/// The five types (28, design change 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Type {
    Architecture,
    Er,
    Sequence,
    State,
    Workflow,
}

impl Type {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::Er => "er",
            Self::Sequence => "sequence",
            Self::State => "state",
            Self::Workflow => "workflow",
        }
    }

    fn levels(self) -> &'static [&'static str] {
        match self {
            Self::Architecture => &["context", "container", "component"],
            Self::Er => &["conceptual", "logical", "physical"],
            _ => &[],
        }
    }

    /// The closed role set; the first is the default.
    pub(crate) fn roles(self) -> &'static [&'static str] {
        match self {
            Self::Architecture => {
                &["application", "library", "store", "deployable", "component", "system", "person", "external"]
            }
            Self::Er => &["entity", "node", "record", "row", "elsewhere"],
            Self::Sequence => &["party"],
            Self::State => &["state", "start", "end", "choice"],
            Self::Workflow => &["step", "start", "end", "decision"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) id: String,
    pub(crate) term: Option<String>,
    pub(crate) noun: Option<String>,
    pub(crate) label: String,
    pub(crate) role: String,
    pub(crate) at: Option<(i32, i32)>,
    pub(crate) span: (i32, i32),
    pub(crate) note: Option<String>,
    pub(crate) fields: Vec<String>,
    pub(crate) time: Option<String>,
    pub(crate) owner: Option<String>,
    pub(crate) lane: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub(crate) id: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) label: String,
    pub(crate) dashed: bool,
    pub(crate) both: bool,
    pub(crate) exit: Option<Side>,
    pub(crate) enter: Option<Side>,
    pub(crate) card: Option<(String, String)>,
}

/// One sequence message, and the dashed return `back` writes on the next row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) id: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) say: String,
    pub(crate) back: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) ret: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) members: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Spec {
    pub(crate) ty: Type,
    pub(crate) level: Option<String>,
    pub(crate) title: String,
    pub(crate) lede: String,
    pub(crate) focus: Vec<String>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) groups: Vec<Group>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) steps: Vec<Step>,
    /// Workflow lanes, `(id, label)`, top to bottom.
    pub(crate) lanes: Vec<(String, String)>,
    /// The author's legend, `(role or style, label)`; none draws the legend from what is in use.
    pub(crate) legend: Option<Vec<(String, String)>>,
}

impl Spec {
    pub(crate) fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

/// Collects the findings of one parse, so a spec with three faults names all three.
struct Reader {
    findings: Vec<Finding>,
}

impl Reader {
    fn bad(&mut self, id: &str, detail: String) {
        self.findings.push(Finding::error("spec/schema", id, detail));
    }

    fn text(&mut self, map: &Yaml, key: &str, owner: &str) -> Option<String> {
        match &map[key] {
            Yaml::BadValue => None,
            Yaml::String(s) => Some(s.clone()),
            Yaml::Integer(i) => Some(i.to_string()),
            Yaml::Real(r) => Some(r.clone()),
            Yaml::Boolean(b) => Some(b.to_string()),
            _ => {
                self.bad(owner, format!("`{key}` is text"));
                None
            }
        }
    }

    fn texts(&mut self, map: &Yaml, key: &str, owner: &str) -> Vec<String> {
        match &map[key] {
            Yaml::BadValue => Vec::new(),
            Yaml::Array(items) => items
                .iter()
                .filter_map(|i| match i {
                    Yaml::String(s) => Some(s.clone()),
                    Yaml::Integer(n) => Some(n.to_string()),
                    _ => {
                        self.bad(owner, format!("`{key}` holds text items"));
                        None
                    }
                })
                .collect(),
            _ => {
                self.bad(owner, format!("`{key}` is a list"));
                Vec::new()
            }
        }
    }

    fn pair(&mut self, map: &Yaml, key: &str, owner: &str) -> Option<(i32, i32)> {
        match &map[key] {
            Yaml::BadValue => None,
            Yaml::Array(a) if a.len() == 2 => match (a[0].as_i64(), a[1].as_i64()) {
                (Some(c), Some(r)) if (0..64).contains(&c) && (0..64).contains(&r) => {
                    Some((i32::try_from(c).ok()?, i32::try_from(r).ok()?))
                }
                _ => {
                    self.bad(owner, format!("`{key}` is two whole numbers from 0 to 63"));
                    None
                }
            },
            _ => {
                self.bad(owner, format!("`{key}` is `[col, row]`"));
                None
            }
        }
    }

    fn keys(&mut self, map: &Yaml, allowed: &[&str], owner: &str) {
        let Yaml::Hash(h) = map else {
            self.bad(owner, "is a map".into());
            return;
        };
        for k in h.keys() {
            let k = k.as_str().unwrap_or_default();
            if !allowed.contains(&k) {
                self.bad(owner, format!("`{k}` is not a field here; the fields are {}", allowed.join(", ")));
            }
        }
    }

    fn side(&mut self, map: &Yaml, key: &str, owner: &str) -> Option<Side> {
        Some(match self.text(map, key, owner)?.as_str() {
            "top" => Side::Top,
            "right" => Side::Right,
            "bottom" => Side::Bottom,
            "left" => Side::Left,
            other => {
                self.bad(owner, format!("`{key}: {other}` is one of top, right, bottom, left"));
                return None;
            }
        })
    }
}

impl Reader {
    fn list<'a>(&mut self, root: &'a Yaml, key: &str) -> &'a [Yaml] {
        match &root[key] {
            Yaml::Array(items) => items,
            Yaml::BadValue => &[],
            _ => {
                self.bad(key, format!("`{key}` is a list"));
                &[]
            }
        }
    }
}

const NODE_KEYS: &[&str] =
    &["id", "term", "noun", "label", "role", "at", "span", "note", "sub", "fields", "time", "owner", "lane"];
const EDGE_KEYS: &[&str] = &["id", "from", "to", "label", "style", "both", "exit", "enter", "card"];
const STEP_KEYS: &[&str] = &["id", "from", "to", "say", "back", "note", "kind"];
const TOP_KEYS: &[&str] =
    &["type", "level", "title", "lede", "focus", "nodes", "parties", "groups", "edges", "steps", "lanes", "legend"];

/// Parses `text`. Every fault becomes a finding; a spec with any error-severity finding does not render.
pub(crate) fn parse(text: &str) -> Result<Spec, Vec<Finding>> {
    let mut r = Reader { findings: Vec::new() };
    let docs = match YamlLoader::load_from_str(text) {
        Ok(docs) => docs,
        Err(e) => return Err(vec![Finding::error("spec/schema", "spec", format!("the YAML does not parse: {e}"))]),
    };
    let Some(root) = docs.first() else {
        return Err(vec![Finding::error("spec/schema", "spec", "the spec is empty".into())]);
    };
    r.keys(root, TOP_KEYS, "spec");
    let ty = match r.text(root, "type", "spec").as_deref() {
        Some("architecture") => Type::Architecture,
        Some("er") => Type::Er,
        Some("sequence") => Type::Sequence,
        Some("state") => Type::State,
        Some("workflow") => Type::Workflow,
        other => {
            r.bad(
                "spec",
                format!("`type: {}` is one of architecture, er, sequence, state, workflow", other.unwrap_or("")),
            );
            return Err(r.findings);
        }
    };
    let level = r.text(root, "level", "spec");
    match (&level, ty.levels()) {
        (Some(l), levels) if !levels.contains(&l.as_str()) => {
            r.bad(
                "spec",
                format!("`level: {l}` is not a level of {}; the levels are: {}", ty.as_str(), levels.join(", ")),
            );
        }
        (None, levels) if !levels.is_empty() => {
            r.bad("spec", format!("{} needs `level`: one of {}", ty.as_str(), levels.join(", ")));
        }
        _ => {}
    }
    let mut spec = Spec {
        ty,
        level,
        title: r.text(root, "title", "spec").unwrap_or_default(),
        lede: r.text(root, "lede", "spec").unwrap_or_default(),
        focus: r.texts(root, "focus", "spec"),
        nodes: Vec::new(),
        groups: Vec::new(),
        edges: Vec::new(),
        steps: Vec::new(),
        lanes: Vec::new(),
        legend: None,
    };
    spec.nodes = nodes(&mut r, root, ty);
    for g in r.list(root, "groups") {
        let id = r.text(g, "id", "groups").unwrap_or_default();
        spec.groups.push(Group {
            label: r.text(g, "label", &id).unwrap_or_default(),
            members: r.texts(g, "members", &id),
            id,
        });
    }
    spec.edges = edges(&mut r, root);
    spec.steps = steps(&mut r, root);
    for l in r.list(root, "lanes") {
        let id = r.text(l, "id", "lanes").unwrap_or_default();
        spec.lanes.push((id.clone(), r.text(l, "label", &id).unwrap_or(id)));
    }
    if !root["legend"].is_badvalue() {
        let mut legend = Vec::new();
        for item in r.list(root, "legend") {
            let key = r.text(item, "role", "legend").or_else(|| r.text(item, "style", "legend")).unwrap_or_default();
            legend.push((key, r.text(item, "label", "legend").unwrap_or_default()));
        }
        spec.legend = Some(legend);
    }
    references(&mut r, &spec);
    if r.findings.is_empty() { Ok(spec) } else { Err(r.findings) }
}

fn nodes(r: &mut Reader, root: &Yaml, ty: Type) -> Vec<Node> {
    let mut out = Vec::new();
    let node_key = if ty == Type::Sequence { "parties" } else { "nodes" };
    for (i, n) in r.list(root, node_key).iter().enumerate() {
        let id = r.text(n, "id", node_key).unwrap_or_else(|| format!("{node_key}[{i}]"));
        r.keys(n, NODE_KEYS, &id);
        let role = r.text(n, "role", &id).unwrap_or_else(|| ty.roles()[0].to_string());
        if !ty.roles().contains(&role.as_str()) {
            r.bad(
                &id,
                format!("`role: {role}` is not a {} role; the roles are: {}", ty.as_str(), ty.roles().join(", ")),
            );
        }
        let (term, noun) = (r.text(n, "term", &id), r.text(n, "noun", &id));
        let label = r.text(n, "label", &id).or_else(|| noun.clone()).or_else(|| term.clone()).unwrap_or_default();
        let mut note = r.text(n, "note", &id);
        if let Some(sub) = r.text(n, "sub", &id) {
            note = Some(sub);
        }
        out.push(Node {
            at: r.pair(n, "at", &id),
            span: r.pair(n, "span", &id).unwrap_or((1, 1)),
            fields: r.texts(n, "fields", &id),
            time: r.text(n, "time", &id),
            owner: r.text(n, "owner", &id),
            lane: r.text(n, "lane", &id),
            id,
            term,
            noun,
            label,
            role,
            note,
        });
    }
    out
}

fn edges(r: &mut Reader, root: &Yaml) -> Vec<Edge> {
    let mut out = Vec::new();
    for e in r.list(root, "edges") {
        let from = r.text(e, "from", "edges").unwrap_or_default();
        let to = r.text(e, "to", "edges").unwrap_or_default();
        let id = r.text(e, "id", "edges").unwrap_or_else(|| format!("{from}--{to}"));
        r.keys(e, EDGE_KEYS, &id);
        let dashed = match r.text(e, "style", &id).as_deref() {
            None | Some("solid") => false,
            Some("dashed") => true,
            Some(s) => {
                r.bad(&id, format!("`style: {s}` is solid or dashed"));
                false
            }
        };
        let card = r.texts(e, "card", &id);
        out.push(Edge {
            label: r.text(e, "label", &id).unwrap_or_default(),
            both: e["both"].as_bool().unwrap_or(false),
            exit: r.side(e, "exit", &id),
            enter: r.side(e, "enter", &id),
            card: (card.len() == 2).then(|| (card[0].clone(), card[1].clone())),
            dashed,
            id,
            from,
            to,
        });
    }
    out
}

fn steps(r: &mut Reader, root: &Yaml) -> Vec<Step> {
    let mut out = Vec::new();
    for (i, s) in r.list(root, "steps").iter().enumerate() {
        let id = r.text(s, "id", "steps").unwrap_or_else(|| format!("s{}", i + 1));
        r.keys(s, STEP_KEYS, &id);
        let ret = match r.text(s, "kind", &id).as_deref() {
            None | Some("call") => false,
            Some("return") => true,
            Some(k) => {
                r.bad(&id, format!("`kind: {k}` is call or return"));
                false
            }
        };
        out.push(Step {
            from: r.text(s, "from", &id).unwrap_or_default(),
            to: r.text(s, "to", &id).unwrap_or_default(),
            say: r.text(s, "say", &id).unwrap_or_default(),
            back: r.text(s, "back", &id),
            note: r.text(s, "note", &id),
            ret,
            id,
        });
    }
    out
}

/// Ids are unique and every reference names one.
fn references(r: &mut Reader, spec: &Spec) {
    let mut seen = std::collections::BTreeSet::new();
    let ids = spec
        .nodes
        .iter()
        .map(|n| &n.id)
        .chain(spec.edges.iter().map(|e| &e.id))
        .chain(spec.steps.iter().map(|s| &s.id));
    for id in ids.chain(spec.groups.iter().map(|g| &g.id)) {
        if !seen.insert(id.as_str()) {
            r.bad(id, format!("`{id}` is used twice; give each node, group and edge its own id"));
        }
    }
    let ends = spec
        .edges
        .iter()
        .map(|e| (&e.id, [&e.from, &e.to]))
        .chain(spec.steps.iter().map(|s| (&s.id, [&s.from, &s.to])));
    for (id, ends) in ends {
        for end in ends {
            if spec.node(end).is_none() {
                r.bad(id, format!("`{end}` names no node"));
            }
        }
    }
    for g in &spec.groups {
        for m in g.members.iter().filter(|m| spec.node(m).is_none()) {
            r.bad(&g.id, format!("member `{m}` names no node"));
        }
    }
    let cells = matches!(spec.ty, Type::Architecture | Type::Er);
    for n in &spec.nodes {
        if cells && n.at.is_none() {
            r.bad(&n.id, format!("{} places each node in a cell: add `at: [col, row]`", spec.ty.as_str()));
        }
        if spec.ty == Type::Workflow && !spec.lanes.iter().any(|(l, _)| Some(l) == n.lane.as_ref()) {
            r.bad(&n.id, "a workflow step names its `lane`, one of `lanes`".into());
        }
    }
}
