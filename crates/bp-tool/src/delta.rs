//! What a delta carries beside its body (28, design changes 7, 8 and 13; 13, Delta): its diagrams and its base.
//! `record delta --diagram <name>=<f>` puts the spec in the delta's own directory, `<delta>/<name>.yaml`, lists the
//! name in the header's `diagrams`, and, when it replaces a spec of the design, writes the diff page
//! `<delta>/<name>.diff.html` against it. `--remove-diagram <name>` lists the name in `remove_diagrams`. The `nodes`
//! check runs on the proposed body and the proposed specs together before the record is written. `base` hashes the
//! design file and its specs when the delta is proposed, so `stamp` refuses `base_moved` rather than write over a
//! change the delta never saw (`m5-design-evidence.md` §3.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_check::nodes::{Names, is_name, specs};
use bp_vault::{Body, Code, DeltaState, Id, Index, Kind, Moves, Refusal, Vault};
use serde_json::{Map, Value};

use crate::diagram::{Refused, invalid, level_of, page};

/// One spec a delta puts: the file it was read from, which a refusal names, and its text.
#[derive(Clone, PartialEq)]
pub(crate) struct Spec {
    pub(crate) file: PathBuf,
    pub(crate) text: String,
}

/// Each diagram a delta changes, by name: the spec that adds or replaces it, or none to remove it.
pub(crate) type Diagrams = BTreeMap<String, Option<Spec>>;

/// Files that are not markdown, by path, with the text to put or none to remove (`Moves::writes`).
pub(crate) type Writes = Vec<(String, Option<String>)>;

/// The `--diagram <name>=<f>` and `--remove-diagram <name>` flags: each name once, each spec read, on a delta only.
pub(crate) fn given(cwd: &Path, kind: Kind, puts: &[String], removes: &[String]) -> Result<Diagrams, Refusal> {
    if kind != Kind::Delta && !(puts.is_empty() && removes.is_empty()) {
        return Err(Refusal::usage("--diagram", "only a delta carries diagrams: bp record delta <design>"));
    }
    let mut diagrams = Diagrams::new();
    let mut add = |name: &str, spec: Option<Spec>| {
        if !is_name(name) {
            return Err(Refusal::usage(
                name,
                "name a diagram in lowercase letters, digits and `-`, such as containers",
            ));
        }
        if diagrams.insert(name.to_string(), spec).is_some() {
            return Err(Refusal::usage(
                name,
                "give each diagram once: --diagram to add or replace it, or --remove-diagram",
            ));
        }
        Ok(())
    };
    for flag in puts {
        let Some((name, file)) = flag.split_once('=') else {
            return Err(Refusal::usage(format!("--diagram {flag}"), "write each as --diagram <name>=<spec file>"));
        };
        let text = crate::diagram::read(cwd, Path::new(file))?;
        add(name, Some(Spec { file: file.into(), text }))?;
    }
    for name in removes {
        add(name, None)?;
    }
    Ok(diagrams)
}

/// The names a header field of a delta lists.
fn names<'h>(header: &'h Map<String, Value>, field: &str) -> impl Iterator<Item = &'h str> {
    header.get(field).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str)
}

/// The diagrams a recorded delta carries: each spec its `diagrams` lists, read from its directory, and each name of
/// its `remove_diagrams`.
pub(crate) fn carried(vault: &Vault, delta: &Id, header: &Map<String, Value>) -> Result<Diagrams, Refusal> {
    let mut diagrams: Diagrams = names(header, "remove_diagrams").map(|n| (n.to_string(), None)).collect();
    for name in names(header, "diagrams") {
        let file = PathBuf::from(format!("{delta}/{name}.yaml"));
        let text = std::fs::read_to_string(vault.root.join(&file)).map_err(|e| {
            Refusal::failed(
                file.display().to_string(),
                format!("{e}; the delta lists `{name}` in `diagrams`, so restore its spec from git"),
            )
        })?;
        diagrams.insert(name.to_string(), Some(Spec { file, text }));
    }
    Ok(diagrams)
}

/// The content hash `base` holds (28, design change 8): blake3 of the design file, then, for each spec under its
/// `diagrams/` by name, a NUL, the name, a NUL and the spec, so a diagram-only change moves it too.
pub(crate) fn base(vault: &Vault, design: &Id) -> Result<String, Refusal> {
    let mut hasher = blake3::Hasher::new();
    hasher.update(vault.text(design)?.as_bytes());
    for (name, text) in specs(vault, design).0 {
        hasher.update(b"\0").update(name.as_bytes()).update(b"\0").update(text.as_bytes());
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// What the record of `delta` writes beside its file: each spec in its directory, the diff page of each spec that
/// replaces one of its design's (28, design change 13), and the `## Rejected` append of the `stale` delta it replaces
/// (13, Delta: the record's one closing append).
pub(crate) fn close(
    vault: &Vault,
    delta: &Id,
    stale: Option<&Id>,
    diagrams: &Diagrams,
    now: i64,
    changes: &mut BTreeMap<Id, String>,
    result: &mut Value,
) -> Result<Moves, Refusal> {
    if let Some(old) = stale {
        let held = vault.text(old)?;
        let gap = if held.ends_with('\n') { "\n" } else { "\n\n" };
        let instant = crate::clock::instant(now);
        changes.insert(old.clone(), format!("{held}{gap}## Rejected\n\nrejected_at: {instant}\n"));
        result["rejected"] = old.as_str().into();
    }
    let design = Kind::Delta.parent(delta).expect("a delta sits under its design");
    let mut writes = Vec::new();
    for (name, spec) in diagrams.iter().filter_map(|(n, s)| Some((n, s.as_ref()?))) {
        writes.push((format!("{delta}/{name}.yaml"), Some(spec.text.clone())));
        // The `nodes` check passed, so the diff page needs no resolver; a design with no such spec has no diff.
        let Ok(held) = std::fs::read_to_string(vault.root.join(format!("{design}/diagrams/{name}.yaml"))) else {
            continue;
        };
        let then = format!("rerun bp record delta {design}");
        let diff = bp_diagram::render(&spec.text, Some(&held), None).map_err(|f| invalid(&spec.file, &f, &then).0)?;
        writes.push((format!("{delta}/{name}.diff.html"), Some(diff.html)));
    }
    Ok(Moves { writes, ..Moves::default() })
}

/// The glossary text a delta's diagrams resolve against: the body of the glossary's proposed delta under the same
/// milestone, which the stamp writes with them, else the glossary as committed.
pub(crate) fn glossary(
    vault: &Vault,
    index: &Index,
    project: &str,
    milestone: &str,
) -> Result<Option<String>, Refusal> {
    let id = format!("projects/{project}/designs/glossary");
    let proposed = index.records(&id, Kind::Delta)?.into_iter().find(|d| {
        d.header.get("milestone").and_then(Value::as_str) == Some(milestone)
            && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed
    });
    Ok(match proposed {
        Some(d) => Some(d.body),
        None => std::fs::read_to_string(vault.root.join(format!("{id}.md"))).ok(),
    })
}

/// What `record delta` adds to the record (28, design changes 7 and 8): its `base`, its `remove_diagrams`, and the
/// `nodes` check of its body and specs together. Returns the pending delta whose base moved, which the new one
/// closes as rejected.
pub(crate) fn propose(
    vault: &Vault,
    index: &Index,
    design: &Id,
    milestone: &str,
    body: &str,
    diagrams: &Diagrams,
    fields: &mut BTreeMap<String, Value>,
) -> Result<Option<Id>, Refused> {
    let stale = pending(vault, index, design, milestone)?;
    let project = design.project().expect("a design sits in a project");
    let glossary = if design.as_str() == format!("projects/{project}/designs/glossary") {
        Some(body.to_string())
    } else {
        glossary(vault, index, project, milestone)?
    };
    let names = Names::with(glossary.as_deref(), design, body);
    check(vault, design, &names, diagrams, &format!("rerun bp record delta {design}"))?;
    fields.insert("base".into(), base(vault, design)?.into());
    for (field, puts) in [("diagrams", true), ("remove_diagrams", false)] {
        let listed: Vec<Value> =
            diagrams.iter().filter(|(_, s)| s.is_some() == puts).map(|(n, _)| n.as_str().into()).collect();
        if !listed.is_empty() {
            fields.insert(field.into(), listed.into());
        }
    }
    Ok(stale)
}

/// The `nodes` check on the proposed body and the proposed specs together (28, design change 7): each spec the delta
/// puts renders and breaks no hard rule, and each spec the design keeps still resolves, so a body change cannot
/// orphan a diagram node. `then` reruns the record.
fn check(vault: &Vault, design: &Id, names: &Names, diagrams: &Diagrams, then: &str) -> Result<(), Refused> {
    let project = design.project().expect("a design sits in a project");
    let specs_of = |c: &Id| specs(vault, c).0.into_iter().map(|(_, t)| t).collect();
    for spec in diagrams.values().flatten() {
        page(&spec.text, project, names, &specs_of).map_err(|f| invalid(&spec.file, &f, then))?;
    }
    for (name, text) in specs(vault, design).0.iter().filter(|(n, _)| !diagrams.contains_key(n)) {
        let mut findings = bp_diagram::outline(text, &|l| names.resolves(l)).map_or_else(|f| f, |(_, f)| f);
        if !findings.is_empty() {
            names.explain(&mut findings);
            let then =
                format!("keep the name in the body, or pass --diagram {name}=<f> with a spec that drops it; {then}");
            return Err(invalid(Path::new(&format!("{design}/diagrams/{name}.yaml")), &findings, &then));
        }
    }
    Ok(())
}

/// The proposed delta of `design` under `milestone`, if any: one whose base moved can never be accepted, so the new
/// delta closes it as rejected; any other refuses `delta_pending` (13, Delta).
fn pending(vault: &Vault, index: &Index, design: &Id, milestone: &str) -> Result<Option<Id>, Refusal> {
    let Some(d) = index.records(design.as_str(), Kind::Delta)?.into_iter().find(|d| {
        d.header.get("milestone").and_then(Value::as_str) == Some(milestone)
            && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed
    }) else {
        return Ok(None);
    };
    let delta = Id::parse(&d.id).expect("the index holds ids");
    match moved(vault, design, &delta, d.header.get("base").and_then(Value::as_str), milestone) {
        Err(r) if r.code == Code::BaseMoved => return Ok(Some(delta)),
        Err(r) => return Err(r),
        Ok(()) => {}
    }
    Err(Refusal::new(
        Code::DeltaPending,
        format!("{design} {milestone}"),
        "a design carries at most one proposed delta per milestone (13, Delta)",
        format!("the pending delta carries the change until the milestone is stamped: bp list delta --under {design}"),
    ))
}

/// Refuses `base_moved` when `design` changed since `delta` was proposed on its `base` (28, design change 8).
pub(crate) fn moved(
    vault: &Vault,
    design: &Id,
    delta: &Id,
    base: Option<&str>,
    milestone: &str,
) -> Result<(), Refusal> {
    // A delta recorded before deltas held a base has nothing to compare.
    let Some(base) = base else { return Ok(()) };
    if self::base(vault, design)? == base {
        return Ok(());
    }
    Err(Refusal::new(
        Code::BaseMoved,
        delta.as_str(),
        "a delta is accepted only onto the design it was proposed on, so a stamp never reverts a change the delta did \
         not see (28, design change 8)",
        format!(
            "{design} or a spec under its diagrams/ changed since {delta} was proposed; read it (bp show {design}), then propose the change on it: \
             bp record delta {design} --field milestone={milestone} --body-file <f>, which closes {delta} as \
             rejected; then rerun the stamp"
        ),
    ))
}

/// What a stamp writes for its carried deltas' diagrams (28, design change 7), and the hash of each spec of the
/// project as the stamp leaves it, by path, for the manifest. Each spec a delta puts is written and rendered against
/// its proposed body; each it removes goes with its page. Every other container-level page is rendered again, so its
/// drill badges count the component diagrams the stamp leaves (13, Diagram); one that no longer renders keeps its
/// page, and the `nodes` check names why.
pub(crate) fn stamped(
    vault: &Vault,
    index: &Index,
    milestone: &Id,
    designs: &[Id],
    carried: &[(&Id, &str, &Diagrams)],
) -> Result<(Writes, BTreeMap<String, String>), Refusal> {
    let project = milestone.project().expect("a milestone sits under a project");
    let glossary = glossary(vault, index, project, milestone.as_str())?;
    let after = |design: &Id| {
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
    };
    let specs_of = |c: &Id| after(c).into_values().collect();
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
        let specs = after(design);
        let redraw: Vec<(&String, &String)> =
            specs.iter().filter(|(n, t)| changed(n) || level_of(t).as_deref() == Some("container")).collect();
        if !redraw.is_empty() {
            let body = match ours {
                Some((_, body, _)) => (*body).to_string(),
                None => vault.read(design)?.body.text,
            };
            let names = Names::with(glossary.as_deref(), design, &body);
            for (name, text) in redraw {
                let (spec, html) = (format!("{design}/diagrams/{name}.yaml"), format!("{design}/diagrams/{name}.html"));
                match page(text, project, &names, &specs_of) {
                    Ok(render) => {
                        put(spec, Some(text.clone()));
                        put(html, Some(render.html));
                    }
                    Err(findings) if changed(name) => {
                        let file = ours.and_then(|(_, _, d)| d[name.as_str()].as_ref()).map(|s| s.file.clone());
                        let file = file.expect("a changed spec the stamp leaves is one a delta puts");
                        // It rendered against the same proposed body when the delta was recorded, so the
                        // glossary changed since.
                        let then = format!("restore what it names in the glossary; then bp stamp {milestone}");
                        let Refused(refusal, _) = invalid(&file, &findings, &then);
                        return Err(refusal);
                    }
                    Err(_) => {}
                }
            }
        }
        for (name, text) in &specs {
            hashes.insert(format!("{design}/diagrams/{name}.yaml"), blake3::hash(text.as_bytes()).to_hex().to_string());
        }
    }
    Ok((writes, hashes))
}
