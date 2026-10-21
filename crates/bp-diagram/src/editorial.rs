//! The editorial test of a spec (28, design change 9): an allow-list, so a part the list does not name must stay equal.

use crate::spec::{self, Spec};

/// Whether `new` differs from `old` only in wording and layout, else the first other change. Wording is the spec's
/// free text: `title`, `lede`, a node's `label` and `note`, an edge's `label`, a step's `say`, `back` text and
/// `note`, and the labels of groups, lanes and the legend. Layout is a node's `at` and `span` and an edge's `exit`
/// and `enter`. Everything else must be equal, list order included. A spec that does not parse is a change.
pub fn editorial_change(old: &str, new: &str) -> Option<String> {
    let (a, b) = match (spec::parse(old), spec::parse(new)) {
        (Ok(a), Ok(b)) => (masked(a), masked(b)),
        (Err(f), _) | (_, Err(f)) => return Some(format!("does not parse: {}", f.first().map_or("", |f| &f.detail))),
    };
    let field = |what: &str, same: bool| (!same).then(|| format!("changes the spec's {what}"));
    field("type", a.ty == b.ty)
        .or_else(|| field("level", a.level == b.level))
        .or_else(|| field("focus", a.focus == b.focus))
        .or_else(|| {
            sequence(
                "node",
                &a.nodes,
                &b.nodes,
                |n| &n.id,
                |x, y| {
                    [
                        ("id", x.id == y.id),
                        ("term", x.term == y.term),
                        ("noun", x.noun == y.noun),
                        ("role", x.role == y.role),
                        ("fields", x.fields == y.fields),
                        ("time", x.time == y.time),
                        ("owner", x.owner == y.owner),
                        ("lane", x.lane == y.lane),
                    ]
                },
            )
        })
        .or_else(|| {
            sequence(
                "group",
                &a.groups,
                &b.groups,
                |g| &g.id,
                |x, y| [("id", x.id == y.id), ("members", x.members == y.members)],
            )
        })
        .or_else(|| {
            sequence(
                "edge",
                &a.edges,
                &b.edges,
                |e| &e.id,
                |x, y| {
                    [
                        ("id", x.id == y.id),
                        ("from", x.from == y.from),
                        ("to", x.to == y.to),
                        ("dashed", x.dashed == y.dashed),
                        ("both", x.both == y.both),
                        ("card", x.card == y.card),
                    ]
                },
            )
        })
        .or_else(|| {
            sequence(
                "step",
                &a.steps,
                &b.steps,
                |s| &s.id,
                |x, y| {
                    [
                        ("id", x.id == y.id),
                        ("from", x.from == y.from),
                        ("to", x.to == y.to),
                        ("ret", x.ret == y.ret),
                        ("return", x.back.is_some() == y.back.is_some()),
                    ]
                },
            )
        })
        .or_else(|| sequence("lane", &a.lanes, &b.lanes, |l| &l.0, |x, y| [("id", x.0 == y.0)]))
        .or_else(|| {
            let (x, y) = (a.legend.as_deref().unwrap_or(&[]), b.legend.as_deref().unwrap_or(&[]));
            let same = a.legend.is_some() == b.legend.is_some() && x.iter().map(|l| &l.0).eq(y.iter().map(|l| &l.0));
            field("legend entries", same)
        })
        // The named fields give the message; this catches a field added to the spec later, so it stays an allow-list.
        .or_else(|| field("structure", a == b))
}

/// The first item that differs by a field `fields` names, else the first one dropped or added, as the change's text.
fn sequence<T, const N: usize>(
    kind: &str,
    old: &[T],
    new: &[T],
    id: impl Fn(&T) -> &String,
    fields: impl Fn(&T, &T) -> [(&'static str, bool); N],
) -> Option<String> {
    for (x, y) in old.iter().zip(new) {
        if let Some((name, _)) = fields(x, y).into_iter().find(|(_, same)| !same) {
            return Some(format!("changes the {name} of {kind} `{}`", id(x)));
        }
    }
    match old.len().cmp(&new.len()) {
        std::cmp::Ordering::Greater => Some(format!("drops {kind} `{}`", id(&old[new.len()]))),
        std::cmp::Ordering::Less => Some(format!("adds {kind} `{}`", id(&new[old.len()]))),
        std::cmp::Ordering::Equal => None,
    }
}

/// `spec` with the wording and layout cleared, so what is left must be equal.
fn masked(mut s: Spec) -> Spec {
    s.title.clear();
    s.lede.clear();
    for n in &mut s.nodes {
        n.label.clear();
        n.note = None;
        n.at = None;
        n.span = (1, 1);
    }
    for g in &mut s.groups {
        g.label.clear();
    }
    for e in &mut s.edges {
        e.label.clear();
        e.exit = None;
        e.enter = None;
    }
    for t in &mut s.steps {
        t.say.clear();
        // A return's text is wording; whether the step has a return is not.
        t.back = t.back.as_ref().map(|_| String::new());
        t.note = None;
    }
    s.lanes.iter_mut().for_each(|l| l.1.clear());
    s.legend.iter_mut().flatten().for_each(|l| l.1.clear());
    s
}

#[cfg(test)]
mod tests {
    use super::editorial_change;

    const OLD: &str = "type: architecture\nlevel: container\ntitle: One\nlede: Two.\nnodes:\n  - {id: a, noun: A, at: [0, 0]}\n  \
                       - {id: b, noun: B, at: [1, 0], note: first}\ngroups:\n  - {id: g, label: Old, members: [a, b]}\n\
                       edges:\n  - {from: a, to: b, label: x}\nlegend:\n  - {role: component, label: Part}\n";

    #[test]
    fn text_and_layout_pass_and_the_rest_is_named() {
        let words = OLD
            .replace("title: One", "title: Uno")
            .replace("lede: Two.", "lede: Deux.")
            .replace("at: [1, 0], note: first", "at: [3, 2], span: [2, 1], note: second")
            .replace("label: Old", "label: New")
            .replace("label: x", "label: y, exit: right, enter: left")
            .replace("label: Part", "label: Piece");
        assert_eq!(editorial_change(OLD, &words), None);
        assert_eq!(
            editorial_change(OLD, &OLD.replace("members: [a, b]", "members: [b, a]")).as_deref(),
            Some("changes the members of group `g`")
        );
        assert_eq!(
            editorial_change(OLD, &OLD.replace("role: component", "role: library")).as_deref(),
            Some("changes the spec's legend entries")
        );
        assert!(editorial_change(OLD, "type: nope").unwrap().starts_with("does not parse"));
    }

    const SEQ: &str = "type: sequence\ntitle: One\nlede: A call.\nparties:\n  - {id: s, noun: Store}\n  - {id: i, noun: Index}\n\
                       steps:\n  - {id: read, from: s, to: i, say: read, back: rows, note: first}\n";

    #[test]
    fn a_step_note_and_return_text_pass_and_a_dropped_return_is_named() {
        assert_eq!(
            editorial_change(SEQ, &SEQ.replace("back: rows, note: first", "back: all rows, note: second")),
            None
        );
        assert_eq!(
            editorial_change(SEQ, &SEQ.replace(", back: rows", "")).as_deref(),
            Some("changes the return of step `read`")
        );
    }
}
