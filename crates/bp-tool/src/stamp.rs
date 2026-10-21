//! `bp stamp <milestone>`: the acceptance (15, An acceptance; 26, design change 5). It runs the milestone's finish
//! line with no lock held, but the blocks the human waives. A failed `error` block commits the runs and refuses
//! `check_failed`. Otherwise one commit holds the runs, a waiver per waived block, each carried delta's body in its
//! design with the delta's `## Accepted`, its diagrams written and rendered (28, design change 7), the version record
//! and the milestone at `done`, with `Bp-Version` and one `Bp-Delta` per delta. A delta whose design changed since it
//! was proposed refuses `base_moved` (28, design change 8). A local tag `blueprint/<project>/<n>` names the commit; the human pushes it (07, Deployment).
//! After the finish line it runs the block of each open waiver of the project, which a pass closes (27, design change
//! 10).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, ExitCode};

use bp_change::{IssueStatus, MilestoneStatus};
use bp_vault::{Body, Check, Code, DeltaState, File, Filter, Header, Id, Index, Kind, Moves, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::waiver::{self, Reask};
use crate::{Wrote, finish, subject};

/// `bp stamp <milestone> [--waive <name> --reason <why>]...`.
#[derive(Args)]
pub struct Stamp {
    /// The milestone: `approved`, and every issue under it resolved or abandoned.
    pub milestone: String,
    /// A finish-line block not to run, recorded as a waiver; or `<milestone>/<name>`, an open waiver of the project,
    /// waived again. Give each its own `--reason`, in the same order.
    #[arg(long, value_name = "NAME")]
    pub waive: Vec<String>,
    /// Why the block of the matching `--waive` is waived, in one line.
    #[arg(long, value_name = "WHY")]
    pub reason: Vec<String>,
}

/// What a stamp did: wrote or found done, or committed the runs of a failed block, which `waive` names.
enum Stamped {
    Wrote(Wrote),
    Failed { at: usize, commit: String, runs: Vec<bp_check::Run>, waive: String },
}

pub(crate) fn run(cwd: &Path, s: &Stamp, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match stamp(cwd, s) {
        Ok(Stamped::Wrote(w)) => crate::wrote(Ok(w), json, out),
        Ok(Stamped::Failed { at, commit, runs, waive }) => {
            let m = &s.milestone;
            let then = format!("rerun bp stamp {m}, or waive it: bp stamp {m} --waive {waive} --reason <why>");
            crate::check::failed(&runs[at], &then, &commit, crate::check::rows(&runs), None, json, out)
        }
        Err(refusal) => finish::<Value>(Err(refusal), json, out),
    }
}

fn stamp(cwd: &Path, s: &Stamp) -> Result<Stamped, Refusal> {
    let role = crate::set::human(format!("stamp {}", s.milestone))?;
    waiver::usage(s)?;
    let vault = Vault::discover(cwd)?;
    let now = crate::clock::now()?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &s.milestone)?;
    let project = id.project().expect("a milestone sits under a project");
    // Every refusal comes before the blocks run, which may take minutes: a human's uncommitted edit to a path the
    // stamp writes, and a tag already taken. Each holds under the lock too.
    vault.clean([&id])?;
    let read = match gate(&vault, &index, &id, &s.waive, now)? {
        Gated::Open(g) => g,
        Gated::Done(done) => return retag(&vault, &index, &id, done),
    };
    // The milestone keeps the paths non-empty: a clean of no path reads the whole tree, ignored files too. A spec that
    // does not render refuses here too, and so does a dirty diagram path or a carried spec.
    vault.clean(std::iter::once(&id).chain(read.deltas.iter().flat_map(|c| [&c.design, &c.delta])))?;
    early(&vault, &index, &id, &read)?;
    tag_free(&vault.root, &next(&index, project)?.1)?;
    // A block may rebuild the index; this connection is not needed again.
    drop(index);
    let skip: Vec<&str> = s.waive.iter().map(String::as_str).collect();
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(now));
    let ran = bp_check::run(&vault, &id, cwd, None, &skip, &clock)?;
    // An open waiver waived again runs nothing.
    let mut asked = Vec::new();
    for r in read.reasked.iter().filter(|r| !s.waive.contains(&r.key())) {
        asked.extend(bp_check::run(&vault, &r.milestone, cwd, Some(&r.name), &[], &clock)?.map(|ran| (r, ran)));
    }
    let mut writer = match ran.as_ref().or(asked.first().map(|(_, ran)| ran)) {
        Some(r) => r.writer(&vault, &format!("bp stamp {id}"))?,
        None => vault.writer()?,
    };
    // Another write may have landed while the blocks ran: the gate holds under the lock too.
    let held = match gate(&vault, writer.index(), &id, &s.waive, now)? {
        Gated::Open(g) => g,
        Gated::Done(done) => return Ok(Stamped::Wrote(Wrote::Already(done))),
    };
    let (mut changes, mut runs) = ran.map(|r| r.records(&vault)).unwrap_or_default();
    // Each re-asked waiver and the place of its one run among `runs`.
    let mut reasked = Vec::new();
    for (r, ran) in &asked {
        let (records, rows) = ran.records(&vault);
        changes.extend(records);
        reasked.push((*r, runs.len()));
        runs.extend(rows);
    }
    if let Some(at) = runs.iter().position(bp_check::Run::failed) {
        let written = writer.commit("stamp", role.as_str(), &format!("stamp {id}"), &changes)?;
        let waive = reasked.iter().find(|(_, i)| *i == at).map_or_else(|| runs[at].name.clone(), |(r, _)| r.key());
        return Ok(Stamped::Failed { at, commit: written.commit, runs, waive });
    }
    // The stamp accepts what its blocks ran against: the finish line, the deltas it cleaned before them, and the
    // open waivers.
    if held.finish != read.finish || held.deltas != read.deltas || held.reasked != read.reasked {
        return Err(Refusal::new(
            Code::EvidenceStale,
            id.as_str(),
            "stamp accepts the finish line its blocks ran, the deltas it read before them and the open waivers it ran \
             (26, design change 5; 27, design change 10)",
            format!(
                "the finish line, a carried delta or an open waiver changed while the blocks ran, so nothing was \
                 committed; rerun bp stamp {id}"
            ),
        ));
    }
    writer.clean(std::iter::once(&id).chain(held.deltas.iter().flat_map(|c| [&c.design, &c.delta])))?;
    based(&vault, &id, &held)?;
    let index = writer.index();
    let (n, tag) = next(index, project)?;
    tag_free(&vault.root, &tag)?;
    let version = Id::parse(&format!("projects/{project}/versions/{n}")).expect("a project and a number make an id");
    let accepted_at = crate::clock::instant(now);
    // `version` is a wikilink, so the index holds the delta's edge to its version (13, Delta).
    let close = format!("## Accepted\n\naccepted_at: {accepted_at}\nversion: [[{version}]]\n");
    let deltas = accept(&vault, &held.deltas, &close, &mut changes)?;
    let waivers = waiver::write(&vault, &id, s, &accepted_at, &mut changes);
    let (manifest, drawn) = manifest(&vault, index, &id, &held, &changes)?;
    let fields = [
        ("n", n.into()),
        ("tag", tag.as_str().into()),
        ("manifest", manifest.into()),
        ("accepted_at", accepted_at.as_str().into()),
    ];
    let under = Kind::BlueprintVersion.parent(&version).expect("a version sits under its project");
    changes.insert(version.clone(), record(Kind::BlueprintVersion, &under, &[("milestone", &id)], fields));
    let done = MilestoneStatus::Done.as_str();
    changes.insert(id.clone(), bp_vault::set_field(&vault.text(&id)?, "status", &done.into()));
    writer.trailer(format!("Bp-Version: {n}"));
    for d in &deltas {
        writer.trailer(format!("Bp-Delta: {d}"));
    }
    let moves = Moves { writes: drawn, ..Moves::default() };
    let written = writer.commit_moving("stamp", role.as_str(), &format!("stamp {id}"), &changes, &moves)?;
    make_tag(&vault.root, &tag, &written.commit, n)?;
    Ok(Stamped::Wrote(Wrote::Changed(json!({
        "id": id,
        "status": done,
        "was": MilestoneStatus::Approved.as_str(),
        "version": n,
        "tag": tag,
        "commit": written.commit,
        "runs": crate::check::rows(&runs),
        "waivers": waivers,
        "reasked": reasked.iter().map(|(r, i)| json!({ "waiver": r.waiver, "verdict": runs[*i].verdict })).collect::<Vec<_>>(),
        "deltas": deltas,
        "diagrams": moves.writes.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        "findings": written.findings,
    }))))
}

/// The manifest, which pins each design and each diagram spec as the stamp leaves them (28, design change 7), and
/// the stamp's diagram writes.
fn manifest(
    vault: &Vault,
    index: &Index,
    id: &Id,
    held: &Gate,
    changes: &BTreeMap<Id, String>,
) -> Result<(serde_json::Map<String, Value>, crate::delta::Writes), Refusal> {
    let (drawn, specs) = diagrams(vault, index, id, held)?;
    let mut manifest = serde_json::Map::new();
    for design in &held.designs {
        let hash = match changes.get(design) {
            Some(text) => File::from_bytes(design.clone(), text.as_bytes())?.hash,
            None => vault.read(design)?.hash,
        };
        manifest.insert(design.to_string(), hash.into());
    }
    manifest.extend(specs.into_iter().map(|(path, hash)| (path, hash.into())));
    Ok((manifest, drawn))
}

/// Puts each carried delta into `changes`, with `close` appended, and its body into its design. The body is the
/// proposed design body, so acceptance writes it in whole (13, Design). Returns the deltas.
fn accept(
    vault: &Vault,
    carried: &[Carried],
    close: &str,
    changes: &mut BTreeMap<Id, String>,
) -> Result<Vec<Id>, Refusal> {
    let mut deltas = Vec::new();
    for c in carried {
        if let Some(text) = crate::set::rebody(&vault.text(&c.design)?, &c.body) {
            changes.insert(c.design.clone(), text);
        }
        let held = vault.text(&c.delta)?;
        let gap = if held.ends_with('\n') { "\n" } else { "\n\n" };
        changes.insert(c.delta.clone(), format!("{held}{gap}{close}"));
        deltas.push(c.delta.clone());
    }
    Ok(deltas)
}

/// The project's next version number and its tag: the highest plus one, else 1 (0 is the conversion's).
fn next(index: &Index, project: &str) -> Result<(u64, String), Refusal> {
    let versions = index.records(&format!("projects/{project}/project"), Kind::BlueprintVersion)?;
    let n = versions.iter().filter_map(|r| r.header.get("n")?.as_u64()).max().map_or(1, |h| h + 1);
    Ok((n, format!("blueprint/{project}/{n}")))
}

/// A repeat on a `done` milestone exits 3, unless its version's tag is missing: the stamp stopped between its commit
/// and its tag. Then it tags the commit that added the version record (15, An acceptance).
fn retag(vault: &Vault, index: &Index, id: &Id, done: Value) -> Result<Stamped, Refusal> {
    let project = id.project().expect("a milestone sits under a project");
    let versions = index.records(&format!("projects/{project}/project"), Kind::BlueprintVersion)?;
    let ours = versions.iter().find(|r| r.header.get("milestone").and_then(Value::as_str) == Some(id.as_str()));
    let Some((v, Some(tag), Some(n))) =
        ours.map(|v| (v, v.header.get("tag").and_then(Value::as_str), v.header.get("n").and_then(Value::as_u64)))
    else {
        return Ok(Stamped::Wrote(Wrote::Already(done)));
    };
    if tagged(&vault.root, tag) {
        return Ok(Stamped::Wrote(Wrote::Already(done)));
    }
    let path = format!("{}.md", v.id);
    let commit = git(&vault.root, &["log", "-1", "--format=%H", "--diff-filter=A", "--", &path])
        .map_err(|why| Refusal::failed(tag, format!("git log could not find the commit that added {path}: {why}")))?;
    let commit = commit.trim();
    make_tag(&vault.root, tag, commit, n)?;
    Ok(Stamped::Wrote(Wrote::Changed(json!({
        "id": id,
        "status": MilestoneStatus::Done.as_str(),
        "version": n,
        "tag": tag,
        "commit": commit,
    }))))
}

/// What the gate read: the finish line's named blocks, each design of the project, each proposed delta the
/// milestone carries, and each open waiver of the project. The blocks run against these reads, so the read under the
/// lock must equal them.
struct Gate {
    finish: Vec<Check>,
    designs: Vec<Id>,
    deltas: Vec<Carried>,
    reasked: Vec<Reask>,
}

/// A proposed delta the milestone carries: its design, its id, its body, the proposed design body, its base and its
/// diagrams.
#[derive(PartialEq)]
struct Carried {
    design: Id,
    delta: Id,
    body: String,
    base: Option<String>,
    diagrams: crate::delta::Diagrams,
}

/// Refuses before the blocks run what the diagram writes would: a carried spec that does not render, and a dirty
/// diagram path or carried spec.
fn early(vault: &Vault, index: &Index, id: &Id, gate: &Gate) -> Result<(), Refusal> {
    based(vault, id, gate)?;
    let (drawn, _) = diagrams(vault, index, id, gate)?;
    let specs = gate.deltas.iter().flat_map(|c| c.diagrams.values().flatten().map(|s| s.file.display().to_string()));
    let paths: Vec<String> = drawn.into_iter().map(|(p, _)| p).chain(specs).collect();
    if paths.is_empty() { Ok(()) } else { vault.clean_paths(&paths) }
}

/// Refuses `base_moved` for a carried delta whose design changed since it was proposed. It reads each design as
/// committed, so it runs once they are clean.
fn based(vault: &Vault, id: &Id, gate: &Gate) -> Result<(), Refusal> {
    let mut deltas = gate.deltas.iter();
    deltas.try_for_each(|c| crate::delta::moved(vault, &c.design, &c.delta, c.base.as_deref(), id.as_str()))
}

/// The diagram writes of the deltas `gate` read, and each spec's hash for the manifest (`delta::stamped`).
type Drawn = (crate::delta::Writes, BTreeMap<String, String>);
fn diagrams(vault: &Vault, index: &Index, id: &Id, gate: &Gate) -> Result<Drawn, Refusal> {
    let carried: Vec<_> = gate.deltas.iter().map(|c| (&c.design, c.body.as_str(), &c.diagrams)).collect();
    crate::delta::stamped(vault, index, id, &gate.designs, &carried)
}

/// The gate's answer: the stamp goes on, or the milestone is already `done`, a repeat.
enum Gated {
    Open(Gate),
    Done(Value),
}

/// The refusals before a stamp runs anything, and again under the lock: the subject is a milestone, `approved`, each
/// waived name is a named block of its finish line or an open waiver of the project (`waiver::refuse`), every file
/// of its project parses, and every issue under it is resolved or abandoned (21, needs acceptance).
fn gate(vault: &Vault, index: &Index, id: &Id, waive: &[String], now: i64) -> Result<Gated, Refusal> {
    let file = vault.read(id)?;
    let Some(header) = file.header.as_ref().filter(|h| h.kind == Kind::Milestone) else {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            id.as_str(),
            "stamp accepts a milestone (15, An acceptance)",
            format!("name a milestone: bp list milestone --status approved; {id} is not one"),
        ));
    };
    let status = header.status.as_deref().unwrap_or_default();
    if status == MilestoneStatus::Done.as_str() {
        return Ok(Gated::Done(json!({ "id": id, "status": status })));
    }
    if status != MilestoneStatus::Approved.as_str() {
        let fix = if status == MilestoneStatus::Planned.as_str() {
            format!("the human approves it first: bp transition {id} approved, then bp stamp {id}")
        } else {
            format!("no verb moves a milestone from {status} to done")
        };
        return Err(Refusal::new(
            Code::IllegalTransition,
            format!("{id} done"),
            "stamp moves an approved milestone to done (13, Milestone)",
            fix,
        ));
    }
    let finish: Vec<Check> = bp_check::named(&file.body).into_iter().cloned().collect();
    let project = id.project().expect("a milestone sits under a project");
    let reasked = waiver::open(vault, index, project, now)?;
    waiver::refuse(id, &finish, &reasked, waive)?;
    // A file that does not parse is in no list, so an open issue it holds would pass the next refusal unseen.
    if let Some((path, why)) = index.broken(project)?.into_iter().next() {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            path.as_str(),
            "a milestone is accepted when no file of its project fails to parse (21, needs acceptance)",
            format!("the file does not parse ({why}); edit {path} by hand, git commit it, then bp stamp {id}"),
        ));
    }
    let under = Filter { kind: Kind::Issue, status: None, project: None, under: Some(id.as_str()), limit: crate::ALL };
    let closed = [IssueStatus::Resolved.as_str(), IssueStatus::Abandoned.as_str()];
    let (issues, _) = index.list(&under)?;
    if let Some(open) = issues.iter().find(|i| !closed.contains(&i.status.as_deref().unwrap_or_default())) {
        return Err(Refusal::new(
            Code::IssuesOpen,
            open.id.as_str(),
            "a milestone is accepted when every issue under it is resolved or abandoned (21, needs acceptance)",
            format!(
                "{} is {}; land or abandon each open issue: bp list issue --under {id}",
                open.id,
                open.status.as_deref().unwrap_or("without a status")
            ),
        ));
    }
    let all = Filter { kind: Kind::Design, status: None, project: Some(project), under: None, limit: crate::ALL };
    let designs: Vec<Id> = index.list(&all)?.0.iter().filter_map(|r| Id::parse(&r.id).ok()).collect();
    let mut deltas = Vec::new();
    for design in &designs {
        for d in index.records(design.as_str(), Kind::Delta)? {
            let carried = d.header.get("milestone").and_then(Value::as_str) == Some(id.as_str());
            if carried && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed {
                let delta = Id::parse(&d.id).expect("the index holds ids");
                let diagrams = crate::delta::carried(vault, &delta, &d.header);
                let base = d.header.get("base").and_then(Value::as_str).map(str::to_string);
                deltas.push(Carried { design: design.clone(), delta, body: d.body, base, diagrams });
            }
        }
    }
    Ok(Gated::Open(Gate { finish, designs, deltas, reasked }))
}

/// A record's text: its header under `under`, with one id per `→` field, and no body.
pub(crate) fn record<const N: usize>(
    kind: Kind,
    under: &Id,
    links: &[(&str, &Id)],
    fields: [(&str, Value); N],
) -> String {
    let header = Header {
        kind,
        title: None,
        status: None,
        under: Some(under.clone()),
        aliases: Vec::new(),
        links: links.iter().map(|(k, id)| ((*k).to_string(), vec![(*id).clone()])).collect::<BTreeMap<_, _>>(),
        unresolved: Vec::new(),
        fields: fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    };
    format!("---\n{}---\n", header.emit())
}

/// The tag is already a ref.
fn tagged(root: &Path, tag: &str) -> bool {
    git(root, &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]).is_ok()
}

/// Tags `commit`, which holds version `n`. If git fails, the refusal names the commit and the command to run.
fn make_tag(root: &Path, tag: &str, commit: &str, n: u64) -> Result<(), Refusal> {
    git(root, &["tag", tag, commit]).map(drop).map_err(|why| {
        Refusal::failed(
            tag,
            format!(
                "commit {commit} holds version {n}, but git tag failed: {why}; git -C {} tag {tag} {commit}, \
                 or rerun bp stamp",
                root.display()
            ),
        )
    })
}

/// Refuses `id_taken` when the tag is already a ref: a version's tag names one commit and never moves.
fn tag_free(root: &Path, tag: &str) -> Result<(), Refusal> {
    if !tagged(root, tag) {
        return Ok(());
    }
    Err(Refusal::new(
        Code::IdTaken,
        tag,
        "a blueprint version's tag names one commit and is never moved (12, Blueprint version)",
        format!(
            "the tag has no version record under it; git -C {} show {tag} --no-patch, then delete the tag or add the record",
            root.display()
        ),
    ))
}

/// Runs git in the vault: its stdout, or its stderr as the reason it failed.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out =
        Command::new("git").arg("-C").arg(root).args(args).output().map_err(|e| format!("git did not start: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
