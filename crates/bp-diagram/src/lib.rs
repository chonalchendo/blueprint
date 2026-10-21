//! The diagram renderer (28, design changes 1 to 4, 12 and 13): a pure function from a typed spec to an annotated
//! SVG inside self-contained HTML. No file system, clock, thread or randomness, and integer geometry, so the same
//! spec gives the same bytes on every platform. The crate resolves no link: the caller says whether a `term` or
//! `noun` resolves.

mod diff;
mod font;
pub mod geom;
mod grid;
mod html;
mod label;
mod layout;
mod legend;
mod metrics;
mod nodes;
mod route;
mod rules;
mod scene;
mod seq;
mod spec;
mod svg;

use std::collections::BTreeSet;

use spec::{Spec, Type};

/// How bad a finding is: an error refuses the spec; a warning (a size budget) does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Refuses the spec: `diagram_invalid`.
    Error,
    /// Reported; the diagram still renders.
    Warn,
}

impl Severity {
    /// The word the verb prints.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

/// One broken rule, on one element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The rule, for example `route/through-box`.
    pub rule: &'static str,
    /// The node, group, edge, label or spec field it names.
    pub id: String,
    /// Error or warning.
    pub severity: Severity,
    /// What is wrong, in words.
    pub detail: String,
}

impl Finding {
    pub(crate) fn error(rule: &'static str, id: &str, detail: String) -> Self {
        Self { rule, id: id.to_string(), severity: Severity::Error, detail }
    }

    fn warn(rule: &'static str, id: &str, detail: String) -> Self {
        Self { rule, id: id.to_string(), severity: Severity::Warn, detail }
    }
}

/// A node's link to the vault: a glossary slug, or a noun its home design defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link<'a> {
    /// `term:`, a glossary slug.
    Term(&'a str),
    /// `noun:`, a noun of the home design.
    Noun(&'a str),
}

/// A rendered page and the warnings it carries.
#[derive(Clone, Debug)]
pub struct Render {
    /// The self-contained HTML.
    pub html: String,
    /// Warnings only: an error refuses the render.
    pub findings: Vec<Finding>,
}

/// The header fields the SVG and the page share.
pub(crate) struct Head {
    pub(crate) ty: &'static str,
    pub(crate) level: Option<String>,
    pub(crate) title: String,
    pub(crate) lede: String,
    pub(crate) eyebrow: String,
    pub(crate) prefix: String,
    pub(crate) focus: Vec<String>,
    pub(crate) diff: bool,
}

/// Renders `spec`, or, with `base`, the diff of the two. `resolve` answers whether a node's link resolves; without
/// it, each node still needs exactly one of `term` and `noun`. Returns every finding when any is an error.
pub fn render(
    spec: &str,
    base: Option<&str>,
    resolve: Option<&dyn Fn(Link<'_>) -> bool>,
) -> Result<Render, Vec<Finding>> {
    let head = spec::parse(spec)?;
    let mut findings = spec_rules(&head, resolve);
    let (drawn, classes, ghosts) = match base {
        Some(b) => {
            let base = spec::parse(b)
                .map_err(|f| f.into_iter().map(|mut x| (x.id = format!("base:{}", x.id), x).1).collect::<Vec<_>>())?;
            let m = diff::merge(&base, &head);
            (m.spec, m.classes, m.ghosts)
        }
        None => (head.clone(), std::collections::BTreeMap::new(), BTreeSet::new()),
    };
    let mut scene = match drawn.ty {
        Type::Sequence => seq::lay_out(&drawn),
        _ => match layout::lay_out(&drawn, &ghosts) {
            Ok(s) => s,
            Err(cells) => {
                findings.extend(cells);
                return Err(findings);
            }
        },
    };
    legend::lay_out(&mut scene, &drawn);
    findings.extend(rules::check(&scene.geometry()));
    if findings.iter().any(|f| f.severity == Severity::Error) {
        return Err(findings);
    }
    let head = Head {
        ty: head.ty.as_str(),
        level: head.level.clone(),
        eyebrow: eyebrow(&head),
        prefix: format!("bp-{}", slug(&head.title)),
        focus: head.focus.clone(),
        diff: base.is_some(),
        title: head.title,
        lede: head.lede,
    };
    let svg = svg::svg(&scene, &head, &|id| classes.get(id).cloned().unwrap_or_default());
    Ok(Render { html: html::page(&head, &svg), findings })
}

/// Runs the geometric rules on another tool's geometry: the same rules `render` applies to its own.
pub fn lint(g: &geom::Geometry) -> Vec<Finding> {
    rules::check(g)
}

/// The rules the spec alone decides: the focus count, the title and description, the links, the legend and the
/// size budgets.
fn spec_rules(s: &Spec, resolve: Option<&dyn Fn(Link<'_>) -> bool>) -> Vec<Finding> {
    let mut out = Vec::new();
    if s.focus.len() > 2 {
        out.push(Finding::error("focus/count", "focus", format!("{} focal elements; the most is two", s.focus.len())));
    }
    let known =
        |id: &str| s.node(id).is_some() || s.edges.iter().any(|e| e.id == id) || s.steps.iter().any(|t| t.id == id);
    for f in s.focus.iter().filter(|f| !known(f)) {
        out.push(Finding::error("focus/count", f, format!("focus `{f}` names no node, edge or step")));
    }
    if s.title.trim().is_empty() || s.lede.trim().is_empty() {
        out.push(Finding::error("a11y/title", "spec", "a diagram needs a `title` and a one-sentence `lede`".into()));
    }
    for n in &s.nodes {
        let link = match (&n.term, &n.noun) {
            (Some(t), None) => Link::Term(t),
            (None, Some(noun)) => Link::Noun(noun),
            _ => {
                out.push(Finding::error(
                    "nodes/resolve",
                    &n.id,
                    "give exactly one of `term` (a glossary slug) and `noun` (a noun of the home design)".into(),
                ));
                continue;
            }
        };
        if let Some(r) = resolve
            && !r(link)
        {
            let what = match link {
                Link::Term(t) => format!("term `{t}` is no glossary slug"),
                Link::Noun(noun) => format!("noun `{noun}` is not named by the home design"),
            };
            out.push(Finding::error("nodes/resolve", &n.id, what));
        }
    }
    out.extend(legend::rule(s));
    let (count, max, what) = match s.ty {
        Type::Architecture => (s.nodes.len(), 9, "nodes"),
        Type::Er => (s.nodes.len(), 8, "entities"),
        Type::Sequence => (s.nodes.len(), 5, "lifelines"),
        Type::State | Type::Workflow => (s.nodes.len(), 12, "nodes"),
    };
    let rule = match s.ty {
        Type::Architecture => "budget/architecture",
        Type::Er => "budget/er",
        Type::Sequence => "budget/sequence",
        Type::State => "budget/state",
        Type::Workflow => "budget/workflow",
    };
    if count > max {
        out.push(Finding::warn(rule, "spec", format!("{count} {what} against a budget of {max}: consider a split")));
    }
    if s.edges.len() > 12 {
        out.push(Finding::warn(
            rule,
            "spec",
            format!("{} edges against a budget of 12: consider a split", s.edges.len()),
        ));
    }
    out
}

fn eyebrow(s: &Spec) -> String {
    match (s.ty, s.level.as_deref()) {
        (Type::Architecture, Some("context")) => "System context · C4 level 1".into(),
        (Type::Architecture, Some("component")) => "Component view · C4 level 3".into(),
        (Type::Architecture, _) => "Container view · C4 level 2".into(),
        (Type::Er, level) => format!("Data model · {} level", level.unwrap_or("logical")),
        (Type::Sequence, _) => "Sequence".into(),
        (Type::State, _) => "State machine".into(),
        (Type::Workflow, _) => "Workflow".into(),
    }
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}
