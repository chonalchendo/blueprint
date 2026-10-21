//! Node boxes at their natural size: text measured in the embedded faces, wrapped, and laid out from the top-left
//! corner. Placement moves and stretches them later.

use crate::font::Font;
use crate::geom::Rect;
use crate::scene::{Run, SNode, Shape};
use crate::spec::{Node, Type};

/// The inner padding of a box.
pub(crate) const PAD: i32 = 16;
/// The widest note line and field line, so a box never grows past about 280 px.
const MEASURE: i32 = 232;

pub(crate) const fn up4(v: i32) -> i32 {
    (v + 3) / 4 * 4
}

/// The words a role puts in the mono kind line, and in the legend when the author names none.
pub(crate) fn role_words(role: &str) -> (&'static str, &'static str) {
    match role {
        "application" => ("Application", "Application"),
        "library" => ("Library", "Library, linked in"),
        "store" => ("Data store", "Data store"),
        "deployable" => ("Deployable unit", "Deployable unit"),
        "component" => ("Component", "Component"),
        "system" => ("Software system", "Software system"),
        "person" => ("Person", "Person"),
        "external" => ("Outside", "Outside the system"),
        "node" => ("Node", "Vault node"),
        "record" => ("Record", "Record"),
        "row" => ("Row", "Row of a node"),
        "elsewhere" => ("Elsewhere", "Owned by another domain"),
        "start" => ("Start", "Start"),
        "end" => ("End", "End"),
        "choice" => ("Choice", "Choice"),
        "decision" => ("Decision", "Decision"),
        "step" => ("Step", "Step"),
        "state" => ("State", "State"),
        _ => ("Entity", "Entity"),
    }
}

fn kind_run(text: &str) -> Run {
    Run::new(Font::Mono, 10, 1, "kind", text.to_uppercase())
}

fn base(n: &Node, shape: Shape, w: i32, h: i32, runs: Vec<Run>) -> SNode {
    SNode {
        id: n.id.clone(),
        role: n.role.clone(),
        term: n.term.clone(),
        noun: n.noun.clone(),
        label: n.label.clone(),
        rect: Rect::new(0, 0, up4(w), up4(h)),
        shape,
        dashed: matches!(n.role.as_str(), "external" | "person" | "system" | "elsewhere"),
        runs,
        rules: Vec::new(),
        lifeline: None,
        bars: Vec::new(),
        class: Vec::new(),
    }
}

/// An architecture box: the kind in mono, the name, then the note in muted text.
pub(crate) fn architecture(n: &Node) -> SNode {
    let kind = kind_run(role_words(&n.role).0).at(PAD, 24);
    let name = Run::new(Font::Strong, 14, 0, "name", &n.label).at(PAD, 44);
    let mut runs = vec![kind, name];
    let mut y = 64;
    for line in n.note.as_deref().map(|t| Font::Body.wrap(t, 12, MEASURE)).unwrap_or_default() {
        runs.push(Run::new(Font::Body, 12, 0, "note", line).at(PAD, y));
        y += 17;
    }
    let w = runs.iter().map(|r| r.w).max().unwrap_or(0) + 2 * PAD;
    let shape = match n.role.as_str() {
        "store" => Shape::Store,
        "deployable" => Shape::Deployable,
        _ => Shape::Box,
    };
    base(n, shape, w.max(176), y - 17 + 22, runs)
}

/// An entity: the kind, the name, a rule, then one mono line per field and the muted time line.
pub(crate) fn entity(n: &Node, level: Option<&str>) -> SNode {
    let kind = if n.role == "elsewhere" {
        n.owner.clone().unwrap_or_else(|| "elsewhere".into())
    } else {
        role_words(&n.role).0.into()
    };
    let mut runs = vec![kind_run(&kind).at(PAD, 22), Run::new(Font::Strong, 14, 0, "name", &n.label).at(PAD, 42)];
    let mut rules = Vec::new();
    let mut y = 56;
    if level != Some("conceptual") && (!n.fields.is_empty() || n.time.is_some()) {
        rules.push(52);
        y = 74;
        for f in &n.fields {
            let class = if f.starts_with('#') || f.starts_with('→') { "field key" } else { "field" };
            runs.push(Run::new(Font::Mono, 11, 0, class, f).at(PAD, y));
            y += 18;
        }
        if let Some(t) = &n.time {
            runs.push(Run::new(Font::Mono, 10, 0, "time", format!("time: {t}")).at(PAD, y + 2));
            y += 20;
        }
    }
    let w = runs.iter().map(|r| r.w).max().unwrap_or(0) + 2 * PAD;
    let mut node = base(n, Shape::Box, w.max(184), y - 4, runs);
    node.rules = rules;
    node
}

/// A state or a workflow step: one centred name in a pill or box, or a start dot, end ring or choice diamond.
pub(crate) fn ordered(n: &Node, ty: Type) -> SNode {
    match n.role.as_str() {
        "start" => base(n, Shape::Dot, 24, 24, Vec::new()),
        "end" => base(n, Shape::Ring, 24, 24, Vec::new()),
        "choice" | "decision" => {
            let run = Run::new(Font::Body, 12, 0, "name", &n.label);
            let w = (run.w + 56).max(96);
            base(n, Shape::Diamond, w, 56, vec![run.centred(up4(w) / 2, 28 + Font::Body.cap(12) / 2)])
        }
        _ => {
            let lines = Font::Body.wrap(&n.label, 13, 152);
            let h = 20 + 17 * i32::try_from(lines.len()).unwrap_or(1) + 4;
            let w = lines.iter().map(|l| Font::Body.width(l, 13, 0)).max().unwrap_or(0) + 40;
            let w = up4(w.max(if ty == Type::State { 104 } else { 128 }));
            let first = up4(h) / 2 - (17 * (i32::try_from(lines.len()).unwrap_or(1) - 1)) / 2 + Font::Body.cap(13) / 2;
            let runs = lines
                .into_iter()
                .zip(0..)
                .map(|(l, i)| Run::new(Font::Body, 13, 0, "name", l).centred(w / 2, first + 17 * i))
                .collect();
            base(n, if ty == Type::State { Shape::Pill } else { Shape::Box }, w, h, runs)
        }
    }
}

/// Moves a node and its marks by `(dx, dy)`.
pub(crate) fn shift(n: &mut SNode, dx: i32, dy: i32) {
    n.rect.x += dx;
    n.rect.y += dy;
    for r in &mut n.runs {
        r.x += dx;
        r.y += dy;
    }
    for y in &mut n.rules {
        *y += dy;
    }
}
