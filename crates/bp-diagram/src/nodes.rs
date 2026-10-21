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

/// A role's tag, drawn beside the name only where the kind tells the reader something, and its legend words when the
/// author names none. The default kinds (an application, a component, an entity, a step) carry no tag.
pub(crate) fn role_words(role: &str) -> (Option<&'static str>, &'static str) {
    match role {
        "application" => (None, "Application"),
        "library" => (Some("Library"), "Library, linked in"),
        "store" => (Some("Store"), "Data store"),
        "deployable" => (Some("Deployable"), "Deployable unit"),
        "component" => (None, "Component"),
        "system" => (Some("System"), "Software system"),
        "person" => (Some("Person"), "Person"),
        "external" => (Some("External"), "Outside the system"),
        "node" => (Some("Node"), "Vault node"),
        "record" => (Some("Record"), "Record"),
        "row" => (Some("Row"), "Row of a node"),
        "elsewhere" => (None, "Owned by another domain"),
        "start" => (None, "Start"),
        "end" => (None, "End"),
        "choice" => (None, "Choice"),
        "decision" => (None, "Decision"),
        "step" => (None, "Step"),
        "state" => (None, "State"),
        _ => (None, "Entity"),
    }
}

/// The drill badge's width: a magnifier, then the child count in mono, in a pill.
pub(crate) fn drill_w(children: usize) -> i32 {
    25 + Font::Mono.width(&children.to_string(), 10, 0)
}

/// The name at the top left and, when the kind carries meaning, its tag after it. The tag's badge reaches 6 px past
/// its text on each side, so the returned right edge includes it, and the room the drill badge needs at the top right.
fn title(label: &str, tag: Option<&str>, children: usize) -> (Vec<Run>, i32) {
    let name = Run::new(Font::Strong, 14, 0, "name", label).at(PAD, 28);
    let mut right = PAD + name.w;
    let mut runs = vec![name];
    if let Some(t) = tag {
        let tag = Run::new(Font::Body, 11, 0, "tag", t).at(right + 14, 27);
        right = tag.x + tag.w + 6;
        runs.push(tag);
    }
    if children > 0 {
        right += 12 + drill_w(children);
    }
    (runs, right)
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
        children: 0,
    }
}

/// An architecture box: the name and its tag, then the description in muted text.
pub(crate) fn architecture(n: &Node) -> SNode {
    let (mut runs, right) = title(&n.label, role_words(&n.role).0, n.children);
    let mut y = 50;
    for line in n.note.as_deref().map(|t| Font::Body.wrap(t, 13, MEASURE)).unwrap_or_default() {
        runs.push(Run::new(Font::Body, 13, 0, "note", line).at(PAD, y));
        y += 18;
    }
    let w = runs.iter().map(|r| r.x + r.w).max().unwrap_or(0).max(right) + PAD;
    SNode { children: n.children, ..base(n, Shape::Box, w.max(176), y - 18 + 20, runs) }
}

/// An entity: the name and its tag, a rule, then one mono line per field and the muted time line.
pub(crate) fn entity(n: &Node, level: Option<&str>) -> SNode {
    let tag =
        if n.role == "elsewhere" { Some(n.owner.as_deref().unwrap_or("Elsewhere")) } else { role_words(&n.role).0 };
    let (mut runs, right) = title(&n.label, tag, n.children);
    let mut rules = Vec::new();
    let mut y = 52;
    if level != Some("conceptual") && (!n.fields.is_empty() || n.time.is_some()) {
        rules.push(44);
        y = 66;
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
    let w = runs.iter().map(|r| r.x + r.w).max().unwrap_or(0).max(right) + PAD;
    let mut node = base(n, Shape::Box, w.max(184), y - 4, runs);
    node.rules = rules;
    node.children = n.children;
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
