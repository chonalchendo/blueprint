//! The editorial delta (28, design change 9; 13, Delta; 21, a human act). `record delta <design> --editorial` needs
//! no milestone. The rule is an allow-list: only wording and layout may change. It refuses `not_editorial` when the
//! proposed body changes a check block, a heading, a link, a glossary term or a noun, or a spec it carries changes
//! any part but its text and layout (`bp_diagram::editorial_change`). The human accepts it alone with `bp stamp <delta>`: no finish line runs and no
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
            "`milestone` is required: --field milestone=<milestone>; or --editorial when only wording and layout change",
        )),
        _ => Ok(()),
    }
}

/// The first item one side holds more often than the other, and whether `new` drops or adds it.
fn first<T: PartialEq + Clone>(old: &[T], new: &[T]) -> Option<(T, &'static str)> {
    let count = |xs: &[T], x: &T| xs.iter().filter(|y| *y == x).count();
    let dropped = old.iter().find(|x| count(old, x) > count(new, x)).map(|x| (x.clone(), "drops"));
    dropped.or_else(|| new.iter().find(|x| count(new, x) > count(old, x)).map(|x| (x.clone(), "adds")))
}

/// Refuses `not_editorial` when `body`, the proposed body of `design`, or a spec of `diagrams` changes more than
/// wording and layout. A diagram added or removed is a change. A link may move within the prose; a heading or a check
/// block may not.
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
    let project = design.project().expect("a design sits in a project");
    let terms = |b: &Body| -> Vec<String> {
        let all = (*design == crate::delta::glossary_id(project)).then(|| bp_check::nodes::terms(&b.text));
        all.into_iter().flatten().map(str::to_string).collect::<BTreeSet<_>>().into_iter().collect()
    };
    let nouns = |b: &Body| bp_check::nodes::nouns(&b.text).into_iter().collect::<Vec<_>>();
    let term = || first(&terms(&old), &terms(&new)).map(|(t, how)| format!("{how} glossary term `{t}`"));
    let noun = || first(&nouns(&old), &nouns(&new)).map(|(n, how)| format!("{how} noun `{n}`"));
    if let Some(what) = term().or_else(noun) {
        return Err(not_editorial(design.as_str(), &format!("the proposed body {what}"), design));
    }
    for (name, spec) in diagrams {
        let path = format!("{design}/diagrams/{name}.yaml");
        let before = std::fs::read_to_string(vault.root.join(&path)).ok();
        let found = match (before, spec) {
            (Some(old), Some(s)) => bp_diagram::editorial_change(&old, &s.text),
            (None, Some(_)) => Some("adds the diagram".to_string()),
            (Some(_), None) => Some("removes the diagram".to_string()),
            (None, None) => None,
        };
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
        "an editorial delta changes wording and layout only: no check block, heading, link, glossary term, noun, or \
         diagram part but its text and layout (28, design change 9)",
        format!(
            "{what}; keep it as it is, or propose the change under a milestone: bp record delta {design} --field \
             milestone=<milestone> (bp list milestone --status planned, then approved)"
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
    crate::delta::clean_specs(vault, std::slice::from_ref(&design), &[])?;
    crate::delta::moved(vault, &design, delta, &header, None, &crate::delta::base(vault, &design)?, None)?;
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
