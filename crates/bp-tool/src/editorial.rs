//! The editorial delta (28, design change 9; 13, Delta; 21, a human act). `record delta <design> --editorial` needs
//! no milestone. It refuses `not_editorial` when the proposed body or a spec it carries changes what another file, a
//! check or a view relies on: a check block, a heading, a link, or a diagram's node ids, edges or terms. Wording,
//! labels and cells may change. The human accepts it alone with `bp stamp <delta>`: no finish line runs and no
//! version is stamped, because a version is one milestone's acceptance (13, `BlueprintVersion`); the next milestone's
//! manifest pins what it wrote.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bp_vault::{Body, Code, DeltaState, Id, Kind, Moves, Refusal, Vault};
use serde_json::{Value, json};

use crate::Wrote;
use crate::delta::Diagrams;

/// `--editorial` against the kind and the milestone: an editorial delta has no milestone, and any other delta has one.
pub(crate) fn usage(kind: Kind, editorial: bool, milestone: bool) -> Result<(), Refusal> {
    match (kind == Kind::Delta, editorial, milestone) {
        (false, true, _) => {
            Err(Refusal::usage("--editorial", "only a delta is editorial: bp record delta <design> --editorial"))
        }
        (true, true, true) => Err(Refusal::usage(
            "--editorial",
            "an editorial delta has no milestone: drop --field milestone, or drop --editorial",
        )),
        (true, false, false) => Err(Refusal::new(
            Code::SchemaInvalid,
            "delta milestone",
            "a record holds its kind's fields (13, the record tables)",
            "`milestone` is required: --field milestone=<milestone>; or --editorial when only wording, labels and cells \
             change",
        )),
        _ => Ok(()),
    }
}

/// What an editorial delta keeps of a diagram: each node's link by id, and each edge as `from→to`, as the rendered
/// SVG states them (28, design change 12: the SVG is the view's contract).
#[derive(Default)]
struct Shape {
    nodes: BTreeMap<String, String>,
    edges: Vec<String>,
}

/// The shape of a rendered page: each `<g>` of its SVG with `data-role` is a node, and each with `data-from` an edge.
fn shape(html: &str) -> Shape {
    let svg = html.find("<svg").map_or("", |at| &html[at..html[at..].find("</svg>").map_or(html.len(), |e| at + e)]);
    let mut s = Shape::default();
    for tag in svg.split("<g ").skip(1) {
        let tag = format!(" {}", &tag[..tag.find('>').unwrap_or(tag.len())]);
        let attr = |name: &str| {
            let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
            Some(tag[at..].split('"').next().unwrap_or_default().to_string())
        };
        if let (Some(from), Some(to)) = (attr("data-from"), attr("data-to")) {
            s.edges.push(format!("{from}→{to}"));
        } else if let (Some(id), Some(_)) = (attr("data-id"), attr("data-role")) {
            let link = attr("data-term")
                .map(|t| format!("term {t}"))
                .or_else(|| attr("data-noun").map(|n| format!("noun {n}")));
            s.nodes.insert(id, link.unwrap_or_default());
        }
    }
    s.edges.sort();
    s
}

/// The first item one side holds more often than the other, and whether `new` drops or adds it.
fn first<T: PartialEq + Clone>(old: &[T], new: &[T]) -> Option<(T, &'static str)> {
    let count = |xs: &[T], x: &T| xs.iter().filter(|y| *y == x).count();
    let dropped = old.iter().find(|x| count(old, x) > count(new, x)).map(|x| (x.clone(), "drops"));
    dropped.or_else(|| new.iter().find(|x| count(new, x) > count(old, x)).map(|x| (x.clone(), "adds")))
}

/// Refuses `not_editorial` when `body`, the proposed body of `design`, or a spec of `diagrams` changes a check block,
/// a heading, a link, or a diagram's node ids, edges or terms. A diagram added or removed changes its node ids. A
/// link may move within the prose; a heading or a check block may not.
pub(crate) fn refuse(vault: &Vault, design: &Id, body: &str, diagrams: &Diagrams) -> Result<(), Refusal> {
    let (old, new) = (vault.read(design)?.body, Body::parse(body));
    let check = (old.checks != new.checks).then(|| {
        let i = (0..old.checks.len().max(new.checks.len()))
            .find(|i| old.checks.get(*i) != new.checks.get(*i))
            .expect("the blocks differ");
        let c = old.checks.get(i).or(new.checks.get(i)).expect("one side holds the block");
        format!("changes check block `{}`", c.attrs.get("name").unwrap_or(&c.runner))
    });
    let heading = || {
        let moved = (old.headings != new.headings).then(|| "moves a heading".to_string());
        first(&old.headings, &new.headings).map(|(h, how)| format!("{how} heading `{h}`")).or(moved)
    };
    let links = |b: &Body| -> Vec<String> {
        let all =
            b.links.iter().map(Id::to_string).chain(b.unresolved.iter().cloned()).chain(b.markdown.iter().cloned());
        all.collect::<BTreeSet<_>>().into_iter().collect()
    };
    let link = || first(&links(&old), &links(&new)).map(|(l, how)| format!("{how} link `{l}`"));
    if let Some(what) = check.or_else(heading).or_else(link) {
        return Err(not_editorial(design.as_str(), &format!("the proposed body {what}"), design));
    }
    for (name, spec) in diagrams {
        let path = format!("{design}/diagrams/{name}.yaml");
        let draw = |text: &str, file: &str| {
            bp_diagram::render(text, None, None).map(|r| shape(&r.html)).map_err(|f| {
                let fix =
                    format!("replace it through a milestone delta: bp record delta {design} --field milestone=<m>");
                crate::delta::in_vault(file, &f, &fix).0
            })
        };
        let before = match std::fs::read_to_string(vault.root.join(&path)) {
            Ok(text) => draw(&text, &path)?,
            Err(_) => Shape::default(),
        };
        let after = match spec {
            Some(s) => draw(&s.text, &s.file.display().to_string())?,
            None => Shape::default(),
        };
        let (bn, an): (Vec<&String>, Vec<&String>) = (before.nodes.keys().collect(), after.nodes.keys().collect());
        let mut found = first(&bn, &an).map(|(id, how)| format!("{how} node `{id}`"));
        let term = before.nodes.iter().find(|(id, link)| after.nodes.get(*id).is_some_and(|l| l != *link));
        found = found.or_else(|| term.map(|(id, link)| format!("changes the link of node `{id}` from `{link}`")));
        found = found.or_else(|| first(&before.edges, &after.edges).map(|(e, how)| format!("{how} edge `{e}`")));
        if let Some(what) = found {
            return Err(not_editorial(&path, &format!("the delta's spec {what}"), design));
        }
    }
    Ok(())
}

fn not_editorial(subject: &str, what: &str, design: &Id) -> Refusal {
    Refusal::new(
        Code::NotEditorial,
        subject,
        "an editorial delta changes wording, labels and cells only: no check block, heading, link, or diagram node \
         id, edge or term (28, design change 9)",
        format!(
            "{what}; keep it as it is, or propose the change under a milestone: bp record delta {design} --field \
             milestone=<milestone> (bp list milestone --status approved)"
        ),
    )
}

/// `bp stamp <delta>`: the human accepts an editorial delta alone (28, design change 9; 21, a human act). Under the
/// lock it refuses `path_dirty` and `base_moved` as a milestone's stamp does, then writes the body and the diagrams
/// into the design and appends `## Accepted`, in one commit with `Bp-Delta`. A milestone's delta waits for its
/// milestone's stamp.
pub(crate) fn stamp(cwd: &Path, delta: &Id, s: &crate::stamp::Stamp) -> Result<Wrote, Refusal> {
    let role = crate::set::human(format!("stamp {delta}"))?;
    if !s.waive.is_empty() {
        return Err(Refusal::usage(
            "--waive",
            format!("an editorial delta has no finish line to waive: bp stamp {delta}"),
        ));
    }
    let vault = &Vault::discover(cwd)?;
    let now = crate::clock::now()?;
    let mut writer = vault.writer()?;
    let file = vault.read(delta)?;
    let header = file.header_json().expect("a delta has a header");
    let design = Kind::Delta.parent(delta).expect("a delta sits under its design");
    if header.get("editorial") != Some(&Value::Bool(true)) {
        let m = header.get("milestone").and_then(Value::as_str).unwrap_or("<milestone>");
        return Err(Refusal::new(
            Code::IllegalTransition,
            format!("{delta} accepted"),
            "a milestone's delta is accepted when its milestone is stamped; only an editorial delta is stamped alone \
             (28, design change 9)",
            format!("bp stamp {m}"),
        ));
    }
    match DeltaState::of(&file.body) {
        DeltaState::Accepted => return Ok(Wrote::Already(json!({ "id": delta, "state": "accepted" }))),
        DeltaState::Rejected => {
            return Err(Refusal::new(
                Code::IllegalTransition,
                format!("{delta} accepted"),
                "a rejected delta stays rejected (13, Delta)",
                format!("propose the change again: bp record delta {design} --editorial --body-file <f>"),
            ));
        }
        DeltaState::Proposed => {}
    }
    writer.clean([&design, delta])?;
    crate::delta::moved(vault, &design, delta, &header, None)?;
    let diagrams = crate::delta::carried(vault, delta, &header)?;
    let designs = crate::delta::all(writer.index(), design.project().expect("a design sits in a project"))?;
    let carried = [(&design, file.body.text.as_str(), &diagrams)];
    crate::redraw::ready(vault, delta, &designs, &carried)?;
    let (drawn, _) = crate::redraw::stamped(vault, writer.index(), delta, &designs, &carried)?;
    let mut changes = BTreeMap::new();
    if let Some(text) = crate::set::rebody(&vault.text(&design)?, &file.body.text) {
        changes.insert(design.clone(), text);
    }
    let close = format!("## Accepted\n\naccepted_at: {}\n", crate::clock::instant(now));
    changes.insert(delta.clone(), crate::delta::append(&vault.text(delta)?, &close));
    writer.trailer(format!("Bp-Delta: {delta}"));
    let moves = Moves { writes: drawn, ..Moves::default() };
    let written = writer.commit_moving("stamp", role.as_str(), &format!("stamp {delta}"), &changes, &moves)?;
    Ok(Wrote::Changed(json!({
        "id": delta,
        "design": design,
        "state": DeltaState::Accepted.as_str(),
        "was": DeltaState::Proposed.as_str(),
        "commit": written.commit,
        "diagrams": moves.writes.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "findings": written.findings,
    })))
}
