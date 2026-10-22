//! The spec's item lists: nodes, edges and steps, each read field by field into its typed form.

use yaml_rust2::Yaml;

use super::{Edge, Node, Reader, Step, Type};

const NODE_KEYS: &[&str] =
    &["id", "term", "noun", "label", "role", "at", "span", "note", "sub", "fields", "time", "owner", "lane"];
const EDGE_KEYS: &[&str] = &["id", "from", "to", "label", "style", "both", "exit", "enter", "card"];
const STEP_KEYS: &[&str] = &["id", "from", "to", "say", "back", "note", "kind"];

pub(super) fn nodes(r: &mut Reader, root: &Yaml, ty: Type) -> Vec<Node> {
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
        let said = r.text(n, "label", &id);
        // A dot or ring draws no text, so a label would be stored and never seen.
        if said.is_some() && matches!(role.as_str(), "start" | "end") {
            r.bad(&id, format!("a `{role}` node draws no `label`; say it in the design"));
        }
        let label = said.or_else(|| noun.clone()).or_else(|| term.clone()).unwrap_or_default();
        let (note, sub) = (r.text(n, "note", &id), r.text(n, "sub", &id));
        if note.is_some() && sub.is_some() {
            r.bad(&id, "give `note` (a box's line) or `sub` (a party's line), not both".into());
        }
        if (note.is_some() || sub.is_some()) && (ty == Type::Er || crate::nodes::pseudo(&role)) {
            // Name the key the author wrote. A dot or ring draws no text at all, so only the design can hold the words.
            let key = if note.is_some() { "note" } else { "sub" };
            let what = if ty == Type::Er { "an `er` node".to_string() } else { format!("a `{role}` node") };
            let fix = if matches!(role.as_str(), "start" | "end") { "say it in the design" } else { "use its `label`" };
            r.bad(&id, format!("{what} draws no `{key}`; {fix}"));
        }
        let note = match (note, sub) {
            (Some(_), Some(_)) => None,
            (note, sub) => note.or(sub),
        };
        out.push(Node {
            at: r.pair(n, "at", &id, 0),
            span: r.pair(n, "span", &id, 1).unwrap_or((1, 1)),
            fields: r.texts(n, "fields", &id),
            time: r.text(n, "time", &id),
            owner: r.text(n, "owner", &id),
            lane: r.text(n, "lane", &id),
            children: 0,
            changed: false,
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

pub(super) fn edges(r: &mut Reader, root: &Yaml) -> Vec<Edge> {
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
        if !matches!(card.len(), 0 | 2) {
            r.bad(&id, "`card` is two items: the cardinality at `from`, then at `to`".into());
        }
        let both = match &e["both"] {
            Yaml::BadValue => false,
            Yaml::Boolean(b) => *b,
            _ => {
                r.bad(&id, "`both` is true or false".into());
                false
            }
        };
        out.push(Edge {
            label: r.text(e, "label", &id).unwrap_or_default(),
            both,
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

pub(super) fn steps(r: &mut Reader, root: &Yaml) -> Vec<Step> {
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
