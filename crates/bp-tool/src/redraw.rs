//! What a stamp does with its carried deltas' diagrams (28, design change 7; 15, An acceptance): the refusals before
//! it writes, the specs it writes, renders and removes, the container pages whose drill count changed, and each
//! spec's hash for the manifest.

use std::cell::RefCell;
use std::collections::BTreeMap;

use bp_check::nodes::{Names, specs};
use bp_diagram::Link;
use bp_vault::{Id, Index, Kind, Refusal, SLUG_MAX, Vault};

use crate::delta::{Diagrams, Writes, carrier, clean_specs, glossary, glossary_id, in_vault, terms_hold};
use crate::diagram::{level_of, page};

/// What a stamp of `subject`, a milestone or an editorial delta, refuses before it writes, before the blocks run and
/// again under the lock: `path_dirty` on any spec of the project or a carried spec, and, when it carries a glossary
/// delta, a term it drops that a spec still uses.
pub(crate) fn ready(
    vault: &Vault,
    subject: &Id,
    designs: &[Id],
    carried: &[(&Id, &str, &Diagrams)],
) -> Result<(), Refusal> {
    let files: Vec<String> =
        carried.iter().flat_map(|(_, _, d)| d.values().flatten().map(|s| s.file.display().to_string())).collect();
    clean_specs(vault, designs, &files)?;
    let project = subject.project().expect("a stamp's subject sits under a project");
    let Some((glossary, body, _)) = carried.iter().find(|(d, ..)| **d == glossary_id(project)) else { return Ok(()) };
    let body_of = |d: &Id| match carried.iter().find(|(c, ..)| *c == d) {
        Some((_, b, _)) => Ok((*b).to_string()),
        None => vault.read(d).map(|f| f.body.text),
    };
    let milestone = Kind::Milestone.fits(subject).then_some(subject.as_str());
    let record = format!("bp record delta {glossary} {}", carrier(milestone));
    terms_hold(designs, body, &|d| after(vault, d, carried), &body_of, &record).map_err(|r| r.0)
}

/// The specs of `design` as the stamp leaves them, by name.
fn after(vault: &Vault, design: &Id, carried: &[(&Id, &str, &Diagrams)]) -> BTreeMap<String, String> {
    let mut specs: BTreeMap<String, String> = specs(vault, design).0.into_iter().collect();
    for (_, _, diagrams) in carried.iter().filter(|(d, ..)| *d == design) {
        for (name, spec) in *diagrams {
            match spec {
                Some(s) => specs.insert(name.clone(), s.text.clone()),
                None => specs.remove(name),
            };
        }
    }
    specs
}

/// The container designs whose drill count the stamp changes: each carried design whose set of component-level specs
/// changes (13, Diagram).
fn drilled(vault: &Vault, carried: &[(&Id, &str, &Diagrams)]) -> Vec<Id> {
    let component = |t: &str| level_of(t).as_deref() == Some("component");
    let changes = |(d, _, diagrams): &&(&Id, &str, &Diagrams)| {
        let before: BTreeMap<String, String> = specs(vault, d).0.into_iter().collect();
        diagrams.iter().any(|(name, spec)| {
            before.get(name).is_some_and(|t| component(t)) || spec.as_ref().is_some_and(|s| component(&s.text))
        })
    };
    carried.iter().filter(changes).map(|(d, ..)| (*d).clone()).collect()
}

/// Whether a container-level `spec` of `project` has a node that drills to one of `designs`.
fn drills_to(spec: &str, project: &str, designs: &[Id]) -> bool {
    let links = RefCell::new(Vec::new());
    let _ = bp_diagram::outline(spec, &|l: Link<'_>| {
        let (Link::Term(t) | Link::Noun(t)) = l;
        links.borrow_mut().push(format!("projects/{project}/designs/containers/{}", bp_vault::slug(t, SLUG_MAX)));
        true
    });
    links.into_inner().iter().any(|l| designs.iter().any(|d| d.as_str() == l))
}

/// What a stamp writes for its carried deltas' diagrams (28, design change 7), and the hash of each spec of the
/// project as the stamp leaves it, by path, for the manifest. Each spec a delta puts is written and rendered against
/// its proposed body; each it removes goes with its page. Each container-level page with a node that drills to a
/// design whose component views change is rendered again, so its badge counts them (13, Diagram). A page that does
/// not render refuses; the stamp never keeps a stale one. `subject` is what the stamp names: a milestone, or an
/// editorial delta. A carried glossary delta is the glossary the stamp leaves.
pub(crate) fn stamped(
    vault: &Vault,
    index: &Index,
    subject: &Id,
    designs: &[Id],
    carried: &[(&Id, &str, &Diagrams)],
) -> Result<(Writes, BTreeMap<String, String>), Refusal> {
    let project = subject.project().expect("a stamp's subject sits under a project");
    let glossary = match carried.iter().find(|(d, ..)| **d == glossary_id(project)) {
        Some((_, body, _)) => Some((*body).to_string()),
        None => glossary(vault, index, project, None)?,
    };
    let drilled = drilled(vault, carried);
    let specs_of = |c: &Id| after(vault, c, carried).into_values().collect();
    let (mut writes, mut hashes) = (Vec::new(), BTreeMap::new());
    let mut put = |path: String, text: Option<String>| {
        if std::fs::read_to_string(vault.root.join(&path)).ok() != text {
            writes.push((path, text));
        }
    };
    for design in designs {
        let ours = carried.iter().find(|(d, ..)| *d == design);
        let changed = |name: &str| ours.is_some_and(|(_, _, diagrams)| diagrams.contains_key(name));
        for (name, _) in ours.iter().flat_map(|(_, _, diagrams)| diagrams.iter()).filter(|(_, s)| s.is_none()) {
            put(format!("{design}/diagrams/{name}.yaml"), None);
            put(format!("{design}/diagrams/{name}.html"), None);
        }
        let specs = after(vault, design, carried);
        let redraw: Vec<(&String, &String)> = specs
            .iter()
            .filter(|(n, t)| {
                changed(n) || (level_of(t).as_deref() == Some("container") && drills_to(t, project, &drilled))
            })
            .collect();
        if !redraw.is_empty() {
            let body = match ours {
                Some((_, body, _)) => (*body).to_string(),
                None => vault.read(design)?.body.text,
            };
            let names = Names::with(glossary.as_deref(), design, &body);
            for (name, text) in redraw {
                let (spec, html) = (format!("{design}/diagrams/{name}.yaml"), format!("{design}/diagrams/{name}.html"));
                let render = page(text, project, &names, &specs_of).map_err(|findings| {
                    // A carried spec rendered against the same body when its delta was recorded, and the glossary
                    // check passed, so only a rule the drill badge's width breaks is left.
                    let (path, fix) = match ours.and_then(|(_, _, d)| d.get(name.as_str())?.as_ref()) {
                        Some(s) => (
                            s.file.display().to_string(),
                            format!("bp record delta {design} again with a spec that renders"),
                        ),
                        None => (
                            spec.clone(),
                            format!("replace it through a delta: bp record delta {design} --diagram {name}=<f>"),
                        ),
                    };
                    in_vault(&path, &findings, &format!("{fix}; then rerun bp stamp {subject}")).0
                })?;
                put(spec, Some(text.clone()));
                put(html, Some(render.html));
            }
        }
        for (name, text) in &specs {
            hashes.insert(format!("{design}/diagrams/{name}.yaml"), blake3::hash(text.as_bytes()).to_hex().to_string());
        }
    }
    Ok((writes, hashes))
}
