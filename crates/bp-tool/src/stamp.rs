//! `bp stamp <milestone>`: the acceptance (15, An acceptance; 26, design change 5). It runs the milestone's finish
//! line with no lock held, but the blocks the human waives. A failed `error` block commits the runs and refuses
//! `check_failed`. Otherwise one commit holds the runs, a waiver per waived block, each carried delta's body in its
//! design with the delta's `## Accepted`, the version record and the milestone at `done`, with `Bp-Version` and one
//! `Bp-Delta` per delta. A local tag `blueprint/<project>/<n>` names the commit; the human pushes it (07, Deployment).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, ExitCode};

use bp_change::{IssueStatus, MilestoneStatus};
use bp_vault::{Body, Code, DeltaState, File, Filter, Header, Id, Index, Kind, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::{Wrote, finish, subject};

/// `bp stamp <milestone> [--waive <name> --reason <why>]...`.
#[derive(Args)]
pub struct Stamp {
    /// The milestone: `approved`, and every issue under it resolved or abandoned.
    pub milestone: String,
    /// A finish-line block not to run, recorded as a waiver. Give each its own `--reason`, in the same order.
    #[arg(long, value_name = "NAME")]
    pub waive: Vec<String>,
    /// Why the block of the matching `--waive` is waived, in one line.
    #[arg(long, value_name = "WHY")]
    pub reason: Vec<String>,
}

/// What a stamp did: wrote or found done, or committed the runs of a failed block.
enum Stamped {
    Wrote(Wrote),
    Failed { at: usize, commit: String, runs: Vec<bp_check::Run> },
}

pub(crate) fn run(cwd: &Path, s: &Stamp, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match stamp(cwd, s) {
        Ok(Stamped::Wrote(w)) => crate::wrote(Ok(w), json, out),
        Ok(Stamped::Failed { at, commit, runs }) => {
            let run = &runs[at];
            let m = &s.milestone;
            let then = format!("rerun bp stamp {m}, or waive it: bp stamp {m} --waive {} --reason <why>", run.name);
            crate::check::failed(run, &then, &commit, crate::check::rows(&runs), json, out)
        }
        Err(refusal) => finish::<Value>(Err(refusal), json, out),
    }
}

fn stamp(cwd: &Path, s: &Stamp) -> Result<Stamped, Refusal> {
    if s.waive.len() != s.reason.len() {
        return Err(Refusal::usage("--waive", "give each --waive <name> its own --reason <why>, in the same order"));
    }
    if s.reason.iter().any(|r| r.trim().is_empty() || r.contains(['\n', '\r'])) {
        return Err(Refusal::usage("--reason", "give each reason as one line of text"));
    }
    if let Some(w) = s.waive.iter().enumerate().find_map(|(i, w)| s.waive[..i].contains(w).then_some(w)) {
        return Err(Refusal::usage(format!("--waive {w}"), "waive each block once"));
    }
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let now = crate::clock::now()?;
    let index = Index::open(&vault)?;
    let id = subject(&index, &s.milestone)?;
    // A human's uncommitted edit and every refusal come before the blocks run, which may take minutes.
    vault.clean([&id])?;
    if let Some(done) = gate(&vault, &index, &id, &s.waive)? {
        return Ok(Stamped::Wrote(Wrote::Already(done)));
    }
    // A block may rebuild the index; this connection is not needed again.
    drop(index);
    let skip: Vec<&str> = s.waive.iter().map(String::as_str).collect();
    let clock = || crate::clock::instant(crate::clock::now().unwrap_or(now));
    let ran = bp_check::run(&vault, &id, cwd, None, &skip, &clock)?;
    let mut writer = match &ran {
        Some(r) => r.writer(&vault)?,
        None => vault.writer()?,
    };
    // Another write may have landed while the blocks ran: the gate holds under the lock too.
    if let Some(done) = gate(&vault, writer.index(), &id, &s.waive)? {
        return Ok(Stamped::Wrote(Wrote::Already(done)));
    }
    let (mut changes, runs) = ran.map(|r| r.records(&vault)).unwrap_or_default();
    if let Some(at) = runs.iter().position(bp_check::Run::failed) {
        let written = writer.commit("stamp", role.as_str(), &format!("stamp {id}"), &changes)?;
        return Ok(Stamped::Failed { at, commit: written.commit, runs });
    }
    let project = id.project().expect("a milestone sits under a project");
    let index = writer.index();
    let versions = index.records(&format!("projects/{project}/project"), Kind::BlueprintVersion)?;
    let n = versions.iter().filter_map(|r| r.header.get("n")?.as_u64()).max().map_or(1, |h| h + 1);
    let tag = format!("blueprint/{project}/{n}");
    tag_free(&vault.root, &tag)?;
    let version = Id::parse(&format!("projects/{project}/versions/{n}")).expect("a project and a number make an id");
    let accepted_at = crate::clock::instant(now);
    let all = Filter { kind: Kind::Design, status: None, project: Some(project), under: None, limit: usize::MAX >> 1 };
    let designs: Vec<Id> = index.list(&all)?.0.iter().filter_map(|r| Id::parse(&r.id).ok()).collect();
    let close = format!("## Accepted\n\naccepted_at: {accepted_at}\nversion: {version}\n");
    let deltas = accept(&vault, index, &id, &designs, &close, &mut changes)?;
    let mut waivers = Vec::new();
    for (name, reason) in s.waive.iter().zip(&s.reason) {
        // `valid_until` stays empty, so the next stamp of the project asks again (13, Waiver).
        let waiver = crate::record::free(&vault.root, &format!("{id}/waivers/{name}"));
        let fields = [("reason", reason.as_str().into()), ("decided_at", accepted_at.as_str().into())];
        changes.insert(waiver.clone(), record(Kind::Waiver, &id, &[], fields));
        waivers.push(waiver);
    }
    let mut manifest = serde_json::Map::new();
    for design in &designs {
        let hash = match changes.get(design) {
            Some(text) => File::from_bytes(design.clone(), text.as_bytes())?.hash,
            None => vault.read(design)?.hash,
        };
        manifest.insert(design.to_string(), hash.into());
    }
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
    let written = writer.commit("stamp", role.as_str(), &format!("stamp {id}"), &changes)?;
    git(&vault.root, &["tag", &tag, &written.commit]).map_err(|why| {
        Refusal::failed(
            tag.as_str(),
            format!(
                "commit {} holds version {n}, but git tag failed: {why}; git -C {} tag {tag} {}",
                written.commit,
                vault.root.display(),
                written.commit
            ),
        )
    })?;
    Ok(Stamped::Wrote(Wrote::Changed(json!({
        "id": id,
        "status": done,
        "was": MilestoneStatus::Approved.as_str(),
        "version": n,
        "tag": tag,
        "commit": written.commit,
        "runs": crate::check::rows(&runs),
        "waivers": waivers,
        "deltas": deltas,
        "findings": written.findings,
    }))))
}

/// Puts each proposed delta the milestone carries into `changes`, with `close` appended, and its body into its
/// design. The body is the proposed design body, so acceptance writes it in whole (13, Design). Returns the deltas.
fn accept(
    vault: &Vault,
    index: &Index,
    milestone: &Id,
    designs: &[Id],
    close: &str,
    changes: &mut BTreeMap<Id, String>,
) -> Result<Vec<Id>, Refusal> {
    let mut deltas = Vec::new();
    for design in designs {
        for d in index.records(design.as_str(), Kind::Delta)? {
            let carried = d.header.get("milestone").and_then(Value::as_str) == Some(milestone.as_str());
            if !carried || DeltaState::of(&Body::parse(&d.body)) != DeltaState::Proposed {
                continue;
            }
            if let Some(text) = crate::set::rebody(&vault.text(design)?, &d.body) {
                changes.insert(design.clone(), text);
            }
            let delta = Id::parse(&d.id).expect("the index holds ids");
            let held = vault.text(&delta)?;
            let gap = if held.ends_with('\n') { "\n" } else { "\n\n" };
            changes.insert(delta.clone(), format!("{held}{gap}{close}"));
            deltas.push(delta);
        }
    }
    Ok(deltas)
}

/// The refusals before a stamp runs anything, and again under the lock: the subject is a milestone, `approved`, each
/// waived name is a named block of its finish line, and every issue under it is resolved or abandoned. `Some` when
/// the milestone is already `done`: a repeat, exit 3.
fn gate(vault: &Vault, index: &Index, id: &Id, waive: &[String]) -> Result<Option<Value>, Refusal> {
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
        return Ok(Some(json!({ "id": id, "status": status })));
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
    let names: Vec<&str> = bp_check::named(&file.body).iter().map(|c| c.attrs["name"].as_str()).collect();
    if let Some(w) = waive.iter().find(|w| !names.contains(&w.as_str())) {
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("waive {w}"),
            "a waiver names a block of the milestone's finish line (13, Waiver)",
            format!("waive one of the blocks of {id}: {}", names.join(", ")),
        ));
    }
    let under =
        Filter { kind: Kind::Issue, status: None, project: None, under: Some(id.as_str()), limit: usize::MAX >> 1 };
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
    Ok(None)
}

/// A record's text: its header under `under`, with one id per `→` field, and no body.
fn record<const N: usize>(kind: Kind, under: &Id, links: &[(&str, &Id)], fields: [(&str, Value); N]) -> String {
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

/// Refuses `id_taken` when the tag is already a ref: a version's tag names one commit and never moves.
fn tag_free(root: &Path, tag: &str) -> Result<(), Refusal> {
    if git(root, &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]).is_err() {
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
