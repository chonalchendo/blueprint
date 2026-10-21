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
use bp_diagram::Finding;
use bp_vault::{Body, Code, DeltaState, Filter, Id, Index, Kind, Moves, Refusal, Vault};
use serde_json::{Map, Value};

use crate::diagram::{Refused, invalid, page, rows};

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

/// The content hash `base` holds (28, design change 8; 13, Delta): blake3 over the design's body, the part `stamp`
/// rewrites, then each spec under its `diagrams/` as its name and its text, by name. Each part goes with its length
/// first, so two sets of parts never hash alike. A header-only change moves nothing.
pub(crate) fn base(vault: &Vault, design: &Id) -> Result<String, Refusal> {
    let text = vault.text(design)?;
    let body = &text[bp_vault::header_end(&text).expect("a design has a header")..];
    let mut hasher = blake3::Hasher::new();
    let mut part = |bytes: &[u8]| {
        hasher.update(&(bytes.len() as u64).to_le_bytes()).update(bytes);
    };
    part(body.as_bytes());
    for (name, spec) in specs(vault, design).0 {
        part(name.as_bytes());
        part(spec.as_bytes());
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
        let instant = crate::clock::instant(now);
        changes.insert(old.clone(), append(&vault.text(old)?, &format!("## Rejected\n\nrejected_at: {instant}\n")));
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

/// A delta's text with its one closing append, `## Accepted` or `## Rejected`, after one blank line.
pub(crate) fn append(held: &str, close: &str) -> String {
    let gap = if held.ends_with('\n') { "\n" } else { "\n\n" };
    format!("{held}{gap}{close}")
}

/// The glossary text a delta's diagrams resolve against: the body of the glossary's proposed delta under the same
/// milestone, which the stamp writes with them, else the glossary as committed. An editorial delta (no milestone)
/// is stamped alone, so it reads the glossary as committed.
pub(crate) fn glossary(
    vault: &Vault,
    index: &Index,
    project: &str,
    milestone: Option<&str>,
) -> Result<Option<String>, Refusal> {
    let id = glossary_id(project);
    let proposed = index.records(id.as_str(), Kind::Delta)?.into_iter().find(|d| {
        milestone.is_some()
            && d.header.get("milestone").and_then(Value::as_str) == milestone
            && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed
    });
    Ok(match proposed {
        Some(d) => Some(d.body),
        None => std::fs::read_to_string(vault.root.join(format!("{id}.md"))).ok(),
    })
}

pub(crate) fn glossary_id(project: &str) -> Id {
    Id::parse(&format!("projects/{project}/designs/glossary")).expect("a project and a slug make an id")
}

/// The flag of `record delta` that names what carries a delta: its milestone, or none for an editorial delta (28,
/// design change 9).
pub(crate) fn carrier(milestone: Option<&str>) -> String {
    milestone.map_or_else(|| "--editorial".into(), |m| format!("--field milestone={m}"))
}

/// What `record delta` adds to the record (28, design changes 7 and 8): its `base`, its `diagrams` and
/// `remove_diagrams`, and the `nodes` check of its body and specs together. It reads each spec it checks as
/// committed. Returns the pending delta with the same cause whose base moved, which the new one closes as rejected.
/// An editorial delta has no `milestone`: at most one is pending per design, as one is per milestone.
pub(crate) fn propose(
    vault: &Vault,
    index: &Index,
    design: &Id,
    milestone: Option<&str>,
    body: &str,
    diagrams: &Diagrams,
    fields: &mut BTreeMap<String, Value>,
) -> Result<Option<Id>, Refused> {
    let project = design.project().expect("a design sits in a project");
    let is_glossary = *design == glossary_id(project);
    let designs = if is_glossary { all(index, project)? } else { vec![design.clone()] };
    // The design too: an editorial delta is judged against the design as committed.
    clean_specs(vault, &designs, &[format!("{design}.md")])?;
    let cause = fields.get("cause").and_then(Value::as_str);
    let stale = pending(vault, index, design, milestone, cause)?;
    for name in diagrams.iter().filter(|(_, s)| s.is_none()).map(|(n, _)| n) {
        if !vault.root.join(format!("{design}/diagrams/{name}.yaml")).exists() {
            return Err(Refusal::new(
                Code::UnknownValue,
                format!("--remove-diagram {name}"),
                "a delta removes a diagram its design has (28, design change 7)",
                format!("name a spec of {design}: ls {design}/diagrams/ in the vault, or bp show {design}"),
            )
            .into());
        }
    }
    let glossary = if is_glossary { Some(body.to_string()) } else { glossary(vault, index, project, milestone)? };
    let names = Names::with(glossary.as_deref(), design, body);
    check(vault, design, &names, diagrams, &format!("rerun bp record delta {design}"))?;
    if is_glossary {
        let specs_of = |d: &Id| specs(vault, d).0.into_iter().collect();
        let body_of = |d: &Id| if d == design { Ok(body.to_string()) } else { vault.read(d).map(|f| f.body.text) };
        terms_hold(&designs, body, &specs_of, &body_of, &format!("bp record delta {design}"))?;
    }
    if milestone.is_none() {
        crate::editorial::refuse(vault, design, body, diagrams)?;
        fields.insert("editorial".into(), true.into());
    }
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

/// Every design of `project`.
pub(crate) fn all(index: &Index, project: &str) -> Result<Vec<Id>, Refusal> {
    let all = Filter { kind: Kind::Design, status: None, project: Some(project), under: None, limit: crate::ALL };
    Ok(index.list(&all)?.0.iter().filter_map(|r| Id::parse(&r.id).ok()).collect())
}

/// Refuses `path_dirty` on each spec of `designs` and its page, and on `more`: a spec read from the working tree
/// would reach the record or the version without git holding it.
pub(crate) fn clean_specs(vault: &Vault, designs: &[Id], more: &[String]) -> Result<(), Refusal> {
    let mut paths: Vec<String> = more.to_vec();
    for d in designs {
        for (name, _) in specs(vault, d).0 {
            paths.extend([format!("{d}/diagrams/{name}.yaml"), format!("{d}/diagrams/{name}.html")]);
        }
    }
    if paths.is_empty() { Ok(()) } else { vault.clean_paths(&paths) }
}

/// A `diagram_invalid` refusal on specs in the vault: their subject is the first, and the fix names `bp` calls only,
/// as only `bp` writes a vault file.
pub(crate) fn in_vault(path: &str, findings: &[Finding], fix: &str) -> Refused {
    let first = &findings[0];
    let refusal = Refusal::new(
        Code::DiagramInvalid,
        format!("{path}#{}", first.id),
        "a diagram spec breaks no hard rule (28, design change 4)",
        format!("{}: {}; {fix}", first.rule, first.detail),
    );
    Refused(refusal, Some(rows(findings)))
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
            let fix = format!(
                "keep the name in the proposed body, or add --diagram {name}=<f> with a spec that drops it (bp diagram \
                 --spec-file <f> --out /tmp/{name}.html previews it), or --remove-diagram {name}; then {then}"
            );
            return Err(in_vault(&format!("{design}/diagrams/{name}.yaml"), &findings, &fix));
        }
    }
    Ok(())
}

/// A glossary delta's check (28, design change 7): every spec of `designs`, as `specs_of` gives them, still resolves
/// against the proposed glossary, with each design's body as `body_of` gives it. A term it drops refuses while a
/// spec uses it; the refusal names every such spec. `record` is the glossary delta's record call.
pub(crate) fn terms_hold(
    designs: &[Id],
    glossary: &str,
    specs_of: &dyn Fn(&Id) -> BTreeMap<String, String>,
    body_of: &dyn Fn(&Id) -> Result<String, Refusal>,
    record: &str,
) -> Result<(), Refused> {
    let mut found: Vec<(String, Finding)> = Vec::new();
    for d in designs {
        let specs = specs_of(d);
        if specs.is_empty() {
            continue;
        }
        let names = Names::with(Some(glossary), d, &body_of(d)?);
        for (name, text) in &specs {
            let findings = bp_diagram::outline(text, &|l| names.resolves(l)).map_or_else(|f| f, |(_, f)| f);
            found.extend(findings.into_iter().map(|f| (format!("{d}/diagrams/{name}.yaml"), f)));
        }
    }
    let Some((first, _)) = found.first() else { return Ok(()) };
    let paths: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
    let fix = format!(
        "keep each term these specs use: rerun {record} with a body that defines it; or first replace each spec \
         through a delta on its design (bp record delta <design> --diagram <name>=<f>): {}",
        paths.join(", ")
    );
    let findings: Vec<Finding> =
        found.iter().map(|(p, f)| Finding { detail: format!("{p}: {}", f.detail), ..f.clone() }).collect();
    Err(in_vault(first, &findings, &fix))
}

/// The proposed delta of `design` under `milestone`, or its editorial one, if any. One whose base moved can never be
/// accepted: when it has the new delta's `cause`, the new delta closes it as rejected; any other refuses
/// `delta_pending` (13, Delta).
fn pending(
    vault: &Vault,
    index: &Index,
    design: &Id,
    milestone: Option<&str>,
    cause: Option<&str>,
) -> Result<Option<Id>, Refusal> {
    let Some(d) = index.records(design.as_str(), Kind::Delta)?.into_iter().find(|d| {
        d.header.get("milestone").and_then(Value::as_str) == milestone
            && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed
    }) else {
        return Ok(None);
    };
    let delta = Id::parse(&d.id).expect("the index holds ids");
    let theirs = d.header.get("cause").and_then(Value::as_str);
    let fix = match moved(vault, design, &delta, &d.header, milestone) {
        Ok(()) if milestone.is_none() => {
            format!("the human accepts the pending editorial delta first: bp stamp {delta}; then propose this one")
        }
        Ok(()) => format!(
            "the pending delta carries the change until the milestone is stamped: bp list delta --under {design}"
        ),
        Err(r) if r.code == Code::BaseMoved && theirs == cause => return Ok(Some(delta)),
        Err(r) if r.code == Code::BaseMoved => format!(
            "{delta} can never be stamped, and its cause is {}, not this one; its owner proposes it again: {}",
            theirs.unwrap_or("design work"),
            r.fix
        ),
        Err(r) => return Err(r),
    };
    Err(Refusal::new(
        Code::DeltaPending,
        format!("{design} {}", milestone.unwrap_or("editorial")),
        "a design carries at most one proposed delta per milestone, and one editorial delta (13, Delta)",
        fix,
    ))
}

/// Refuses `base_moved` when `design` changed since `delta`, whose header is `header`, was proposed on its `base`
/// (28, design change 8). A header with no `base` reads as moved. The fix proposes the same change again: the same
/// carrier, cause, specs and removals.
pub(crate) fn moved(
    vault: &Vault,
    design: &Id,
    delta: &Id,
    header: &Map<String, Value>,
    milestone: Option<&str>,
) -> Result<(), Refusal> {
    if header.get("base").and_then(Value::as_str) == Some(base(vault, design)?.as_str()) {
        return Ok(());
    }
    let mut again = vec![format!("bp record delta {design} {}", carrier(milestone))];
    again.extend(header.get("cause").and_then(Value::as_str).map(|c| format!("--field cause={c}")));
    again.push("--body-file <f>".into());
    for (name, spec) in carried(vault, delta, header)? {
        again.push(match spec {
            Some(s) => format!("--diagram {name}={}", vault.root.join(s.file).display()),
            None => format!("--remove-diagram {name}"),
        });
    }
    Err(Refusal::new(
        Code::BaseMoved,
        delta.as_str(),
        "a delta is accepted only onto the design it was proposed on, so a stamp never reverts a change the delta did \
         not see (28, design change 8)",
        format!(
            "{design} or a spec under its diagrams/ changed since {delta} was proposed; read it (bp show {design}), \
             then propose the change on it: {}, which rejects {delta}; then bp stamp {}",
            again.join(" "),
            milestone.unwrap_or("<the new delta>")
        ),
    ))
}
