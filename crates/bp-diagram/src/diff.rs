//! One diff render (28, design change 13): base and head paired by node `id` and edge `from→to` (or its `id`),
//! never by label or place. The head is drawn whole; a removed item stays as a ghost at its base cell (a party or a
//! step, at its place in the base order), and a moved node leaves a ghost there too. CSS alone then shows base,
//! head or diff.

use std::collections::{BTreeMap, BTreeSet};

use crate::spec::{Node, Spec, Type};

/// The spec to draw, each id's diff classes, and the ghosts, which the cell rules skip.
pub(crate) struct Merged {
    pub(crate) spec: Spec,
    pub(crate) classes: BTreeMap<String, Vec<&'static str>>,
    pub(crate) ghosts: BTreeSet<String>,
}

fn same(a: &Node, b: &Node) -> bool {
    (&a.label, &a.role, &a.term, &a.noun, &a.note, &a.fields, &a.time, &a.owner, a.span)
        == (&b.label, &b.role, &b.term, &b.noun, &b.note, &b.fields, &b.time, &b.owner, b.span)
}

pub(crate) fn merge(base: &Spec, head: &Spec) -> Merged {
    let mut spec = head.clone();
    let mut classes: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    let mut ghosts = BTreeSet::new();
    let mut taken: BTreeSet<(i32, i32)> = head.nodes.iter().filter_map(|n| n.at).collect();
    let free = |at: (i32, i32), taken: &mut BTreeSet<(i32, i32)>| {
        let cell = (at.0..at.0 + 64)
            .map(|c| (c, at.1))
            .find(|c| !taken.contains(c))
            .expect("a row has a free cell to its right");
        taken.insert(cell);
        cell
    };
    for n in &head.nodes {
        match base.node(&n.id) {
            None => classes.entry(n.id.clone()).or_default().push("added"),
            Some(b) => {
                if !same(b, n) {
                    classes.entry(n.id.clone()).or_default().push("changed");
                }
                if b.at != n.at && head.ty != Type::Sequence {
                    classes.entry(n.id.clone()).or_default().push("moved");
                    let mut ghost = b.clone();
                    ghost.id = format!("{}@base", b.id);
                    ghost.at = b.at.map(|at| free(at, &mut taken));
                    classes.insert(ghost.id.clone(), vec!["ghost", "moved"]);
                    ghosts.insert(ghost.id.clone());
                    spec.nodes.push(ghost);
                }
            }
        }
    }
    for (i, b) in base.nodes.iter().enumerate().filter(|(_, b)| head.node(&b.id).is_none()) {
        classes.insert(b.id.clone(), vec!["removed"]);
        ghosts.insert(b.id.clone());
        if head.ty == Type::Sequence {
            // A party takes no cell: its ghost keeps its place in the base order.
            let prev = i.checked_sub(1).map(|k| base.nodes[k].id.as_str());
            insert_after(&mut spec.nodes, prev, |n| &n.id, b.clone());
        } else {
            let mut ghost = b.clone();
            ghost.at = b.at.map(|at| free(at, &mut taken));
            spec.nodes.push(ghost);
        }
    }
    for e in &head.edges {
        match base.edges.iter().find(|b| b.id == e.id) {
            None => classes.entry(e.id.clone()).or_default().push("added"),
            Some(b) if (&b.label, b.dashed, b.both, &b.card) != (&e.label, e.dashed, e.both, &e.card) => {
                classes.entry(e.id.clone()).or_default().push("changed");
            }
            Some(_) => {}
        }
    }
    for b in base.edges.iter().filter(|b| head.edges.iter().all(|e| e.id != b.id)) {
        classes.insert(b.id.clone(), vec!["removed"]);
        spec.edges.push(b.clone());
    }
    for s in &head.steps {
        let class = match base.steps.iter().find(|b| b.id == s.id) {
            None => "added",
            Some(b) if (&b.from, &b.to, &b.say, &b.back) != (&s.from, &s.to, &s.say, &s.back) => "changed",
            Some(_) => continue,
        };
        classes.insert(s.id.clone(), vec![class]);
        classes.insert(format!("{}.back", s.id), vec![class]);
    }
    for (i, b) in base.steps.iter().enumerate().filter(|(_, b)| head.steps.iter().all(|s| s.id != b.id)) {
        classes.insert(b.id.clone(), vec!["removed"]);
        classes.insert(format!("{}.back", b.id), vec!["removed"]);
        let prev = i.checked_sub(1).map(|k| base.steps[k].id.as_str());
        insert_after(&mut spec.steps, prev, |s| &s.id, b.clone());
    }
    Merged { spec, classes, ghosts }
}

/// Puts a removed item back after the item that came before it in the base, which is already in place: the base is
/// walked in order. With none before it, it goes first.
fn insert_after<T>(items: &mut Vec<T>, prev: Option<&str>, id: impl Fn(&T) -> &String, item: T) {
    let at = prev.and_then(|p| items.iter().position(|x| id(x) == p)).map_or(0, |k| k + 1);
    items.insert(at, item);
}
