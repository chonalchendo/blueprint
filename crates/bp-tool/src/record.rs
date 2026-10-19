//! `bp record <kind> <parent>`: one append-only record under its parent, one new file, one commit (15, record; 25,
//! design change 6). No verb rewrites a record: a correction is a new record that cites the old one (13, rule 6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_change::DispositionAs;
use bp_design::{ConcernState, DeltaState};
use bp_operation::Role;
use bp_reasoning::Outcome;
use bp_vault::{Code, Header, Id, Index, Kind, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, node_id};

/// `bp record <kind> <parent> [--field <name=value>]... [--body-file <f>] [--external <uri>] [--dry-run]`.
#[derive(Args)]
pub struct Record {
    /// One of: delta, review, disposition, concern-answer, evidence, handoff.
    pub kind: String,
    /// The node the record sits under: a design, issue, review, project or learning; `shared` for a handoff from a
    /// session outside any project.
    pub parent: String,
    /// One field as `name=value`; repeat the flag for each field.
    #[arg(long = "field", value_name = "NAME=VALUE")]
    pub fields: Vec<String>,
    /// The record's body, from a file: a delta's change, a review's findings, a handoff's text.
    #[arg(long)]
    pub body_file: Option<PathBuf>,
    /// A forge URI the record names, such as its pull request.
    #[arg(long)]
    pub external: Option<String>,
    /// Validate and print the record's id; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// What a field holds.
enum Type {
    Text,
    Closed(Vec<&'static str>),
    Int,
    /// A commit sha, hex.
    Sha,
    /// Lower-case letters, digits and `-`: it may name a file.
    Slug,
    Date,
    Instant,
    /// The id of a node or record of one of these kinds.
    Link(&'static [Kind]),
}

/// What names a record within its parent's directory.
enum Key {
    /// The record's instant, UTC to the second with `-` for `:`.
    Stamp,
    /// One of its fields.
    Field(&'static str),
    /// `<date>-<short session id>`, the date from `started_at` or the record's instant.
    Handoff,
}

/// One record kind (13): the parent kinds, the directory, the key, the field that takes the record's instant, and
/// the fields `--field` sets, each with its type and whether it is required.
struct Spec {
    parents: &'static [Kind],
    dir: &'static str,
    key: Key,
    now: Option<&'static str>,
    fields: Vec<(&'static str, Type, bool)>,
}

fn closed<T: Copy>(all: &[T], text: fn(T) -> &'static str) -> Type {
    Type::Closed(all.iter().map(|v| text(*v)).collect())
}

fn spec(kind: Kind) -> Option<Spec> {
    let (parents, dir, key, now, fields): (&[Kind], _, _, _, _) = match kind {
        Kind::Delta => (
            &[Kind::Design],
            "deltas",
            Key::Stamp,
            Some("stamp"),
            vec![
                ("milestone", Type::Link(&[Kind::Milestone]), true),
                ("cause", Type::Link(&[Kind::Thread, Kind::Issue, Kind::Run]), false),
                ("pull_request", Type::Int, false),
                ("authorized_by", Type::Link(&[Kind::Decision]), false),
            ],
        ),
        Kind::Review => (
            &[Kind::Issue],
            "reviews",
            Key::Field("head"),
            Some("at"),
            vec![
                ("head", Type::Sha, true),
                ("patch_id", Type::Sha, false),
                ("pull_request", Type::Int, false),
                ("role", closed(Role::ALL, Role::as_str), false),
            ],
        ),
        Kind::Disposition => (
            &[Kind::Review],
            "dispositions",
            Key::Field("finding"),
            None,
            vec![
                ("finding", Type::Int, true),
                ("as", closed(DispositionAs::ALL, DispositionAs::as_str), true),
                ("reason", Type::Text, false),
                ("head", Type::Sha, false),
            ],
        ),
        Kind::ConcernAnswer => (
            &[Kind::Project],
            "concerns",
            Key::Field("concern"),
            Some("decided_at"),
            vec![
                ("concern", Type::Slug, true),
                ("state", closed(ConcernState::ALL, ConcernState::as_str), true),
                ("design", Type::Link(&[Kind::Design]), false),
                ("reason", Type::Text, false),
                ("valid_until", Type::Date, false),
            ],
        ),
        Kind::Evidence => (
            &[Kind::Learning],
            "evidence",
            Key::Stamp,
            Some("at"),
            vec![
                ("outcome", closed(Outcome::ALL, Outcome::as_str), true),
                ("issue", Type::Link(&[Kind::Issue]), false),
                ("note", Type::Text, false),
            ],
        ),
        Kind::Handoff => (
            &[Kind::Project],
            "handoffs",
            Key::Handoff,
            None,
            vec![
                ("session", Type::Slug, true),
                ("started_at", Type::Instant, false),
                ("ended_at", Type::Instant, false),
                ("resumes", Type::Link(&[Kind::Handoff]), false),
            ],
        ),
        _ => return None,
    };
    Some(Spec { parents, dir, key, now, fields })
}

/// The usage error for a kind `record` does not write, with the verb that does.
fn not_recorded(text: &str) -> Refusal {
    let fix = match text.parse::<Kind>() {
        Ok(Kind::Run) => "only bp check <owner> writes a run (15, the run record)".into(),
        Ok(Kind::Claim) => "bp cut <issue> writes the claim".into(),
        Ok(Kind::Waiver) => "bp stamp <milestone> --waive <predicate> --reason <why> writes a waiver".into(),
        Ok(Kind::BlueprintVersion) => "bp stamp <milestone> writes the version".into(),
        Ok(k) if !k.is_record() => format!("kind {k} is a node: bp create {k}"),
        _ => "record writes one of: delta, review, disposition, concern-answer, evidence, handoff".into(),
    };
    Refusal::usage(format!("record {text}"), fix)
}

pub(crate) fn record(cwd: &Path, r: &Record) -> Result<Wrote, Refusal> {
    let kind = r.kind.parse::<Kind>().map_err(|_| not_recorded(&r.kind))?;
    let spec = spec(kind).ok_or_else(|| not_recorded(&r.kind))?;
    let given = given(kind, &spec, &r.fields)?;
    let body = match &r.body_file {
        Some(p) => std::fs::read_to_string(p).map_err(|e| Refusal::failed(p.display().to_string(), e.to_string()))?,
        None => String::new(),
    };
    if kind == Kind::Delta && body.trim().is_empty() {
        return Err(schema(format!("{kind} body"), "a delta's body says what changes: pass --body-file <f>".into()));
    }
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let now = crate::clock::now()?;
    // The lock comes before the reads, so the key this write takes is still free when it commits.
    let writer = if r.dry_run { None } else { Some(vault.writer()?) };
    let index = Index::open(&vault)?;
    let parent = if kind == Kind::Handoff && r.parent == "shared" { None } else { Some(node_id(&r.parent)?) };
    if let Some(p) = &parent {
        parent_fits(&index, kind, &spec, p)?;
    }
    let mut fields = BTreeMap::new();
    for (name, ty, _) in &spec.fields {
        let Some(text) = given.get(name) else { continue };
        fields.insert((*name).to_string(), value(&index, kind, name, ty, text)?);
    }
    if let (Some(design), Some(milestone)) = (&parent, fields.get("milestone").filter(|_| kind == Kind::Delta)) {
        none_pending(&index, design, milestone)?;
    }
    if let Some(field) = spec.now {
        fields.insert(field.into(), crate::clock::instant(now).into());
    }
    if kind == Kind::Delta {
        fields.insert("state".into(), DeltaState::Proposed.as_str().into());
    }
    if let Some(uri) = &r.external {
        fields.insert("external".into(), uri.as_str().into());
    }
    let key = match spec.key {
        Key::Stamp => crate::clock::instant(now).replace(':', "-"),
        Key::Field(f) => match &fields[f] {
            Value::String(s) => s.clone(),
            v => v.to_string(),
        },
        Key::Handoff => {
            let date = fields
                .get("started_at")
                .and_then(Value::as_str)
                .map_or_else(|| crate::clock::instant(now), str::to_string);
            let session = fields["session"].as_str().expect("a session is a slug");
            format!("{}-{}", &date[..10], session.chars().take(8).collect::<String>())
        }
    };
    // A record of the project's own sits in the project's kind directory (13, rule 4).
    let base = match &parent {
        Some(p) if spec.parents == [Kind::Project] => p.as_str().trim_end_matches("/project").to_string(),
        Some(p) => p.to_string(),
        None => "shared".into(),
    };
    let id = free(&vault.root, &format!("{base}/{}/{key}", spec.dir));
    let header = Header {
        kind,
        title: None,
        status: None,
        under: parent.clone(),
        aliases: Vec::new(),
        links: BTreeMap::new(),
        unresolved: Vec::new(),
        fields,
    };
    let mut text = format!("---\n{}---\n", header.emit());
    if !body.is_empty() {
        text = format!("{text}\n{body}");
    }
    let changes = BTreeMap::from([(id.clone(), text)]);
    let mut result = serde_json::json!({ "id": id, "kind": kind, "under": parent });
    let Some(writer) = writer else {
        let (_, findings) = index.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit("record", role.as_str(), &format!("record {kind} {id}"), &changes)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

fn schema(subject: String, fix: String) -> Refusal {
    Refusal::new(Code::SchemaInvalid, subject, "a record holds its kind's fields (13, the record tables)", fix)
}

/// The `--field` flags by name: each once, each a field of the kind, every required one present.
fn given<'a>(kind: Kind, spec: &Spec, flags: &'a [String]) -> Result<BTreeMap<&'a str, &'a str>, Refusal> {
    let mut given = BTreeMap::new();
    for f in flags {
        let Some((name, value)) = f.split_once('=') else {
            return Err(Refusal::usage(format!("--field {f}"), "write each field as --field name=value"));
        };
        if given.insert(name, value).is_some() {
            return Err(Refusal::usage(format!("--field {name}"), "give each field once"));
        }
    }
    let names: Vec<&str> = spec.fields.iter().map(|(n, _, _)| *n).collect();
    let takes = || {
        let tool: Vec<&str> = spec.now.into_iter().chain((kind == Kind::Delta).then_some("state")).collect();
        let tool = if tool.is_empty() { String::new() } else { format!("; the tool writes {}", tool.join(", ")) };
        format!("record {kind} takes --field {}{tool}", names.join(", --field "))
    };
    if let Some(name) = given.keys().find(|n| !names.contains(n)) {
        return Err(schema(format!("{kind} {name}"), takes()));
    }
    if let Some((name, _, _)) = spec.fields.iter().find(|(n, _, req)| *req && !given.contains_key(n)) {
        return Err(schema(format!("{kind} {name}"), format!("`{name}` is required; {}", takes())));
    }
    Ok(given)
}

/// The parent exists and is a kind this record sits under.
fn parent_fits(index: &Index, kind: Kind, spec: &Spec, parent: &Id) -> Result<(), Refusal> {
    match index.header(parent.as_str())?.and_then(|h| h["kind"].as_str()?.parse::<Kind>().ok()) {
        None => Err(Refusal::new(
            Code::NoNode,
            parent.as_str(),
            "a record sits under a node or record that exists (13, rule 4)",
            format!("bp search {}", parent.as_str().rsplit('/').next().unwrap_or_default().replace(['-', '.'], " ")),
        )),
        Some(k) if !spec.parents.contains(&k) => {
            let want: Vec<&str> = spec.parents.iter().map(|k| k.as_str()).collect();
            Err(schema(
                parent.to_string(),
                format!("kind {kind} sits under kind {}; {parent} is kind {k}", want.join(" or ")),
            ))
        }
        Some(_) => Ok(()),
    }
}

/// Refuses `delta_pending` when the design already carries a proposed delta under the milestone.
fn none_pending(index: &Index, design: &Id, milestone: &Value) -> Result<(), Refusal> {
    let pending = index.records(design.as_str(), Kind::Delta)?.into_iter().any(|d| {
        d.header.get("state").and_then(Value::as_str) == Some(DeltaState::Proposed.as_str())
            && d.header.get("milestone") == Some(milestone)
    });
    if !pending {
        return Ok(());
    }
    Err(Refusal::new(
        Code::DeltaPending,
        format!("{design} {}", milestone.as_str().unwrap_or_default()),
        "a design carries at most one proposed delta per milestone (13, Delta)",
        format!("the pending delta carries the change until the milestone is stamped: bp list delta --under {design}"),
    ))
}

/// The id `stem` names, or the first free `-2`, `-3`: a second record with one key under one parent (25, design
/// change 6).
fn free(root: &Path, stem: &str) -> Id {
    let mut n = 1;
    loop {
        let text = if n == 1 { stem.to_string() } else { format!("{stem}-{n}") };
        let id = Id::parse(&text).expect("a parent id and a slug key make an id");
        if !id.path(root).exists() {
            return id;
        }
        n += 1;
    }
}

/// The header value of one `--field`, refused when it is outside its type.
fn value(index: &Index, kind: Kind, name: &str, ty: &Type, text: &str) -> Result<Value, Refusal> {
    let bad = |want: &str| {
        Refusal::new(
            Code::SchemaInvalid,
            format!("{kind} {name}={text}"),
            "a record holds its kind's fields (13, the record tables)",
            format!("`{name}` takes {want}"),
        )
    };
    let stamp = |t: &str| bp_change::intents::timestamp(t).is_some();
    Ok(match ty {
        Type::Text => text.into(),
        Type::Closed(values) if values.contains(&text) => text.into(),
        Type::Closed(values) => {
            return Err(Refusal::new(
                Code::UnknownValue,
                format!("{name} {text}"),
                "a field with a closed set takes one of its values (13)",
                format!("use one of: {}", values.join(", ")),
            ));
        }
        Type::Int => text.parse::<u64>().map_err(|_| bad("a whole number"))?.into(),
        Type::Sha if (7..=64).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_hexdigit()) => text.into(),
        Type::Sha => return Err(bad("a commit sha in hex")),
        Type::Slug
            if !text.is_empty() && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') =>
        {
            text.into()
        }
        Type::Slug => return Err(bad("lower-case letters, digits and -")),
        Type::Date if stamp(&format!("{text}T00:00:00Z")) => text.into(),
        Type::Date => return Err(bad("a date, YYYY-MM-DD")),
        Type::Instant if stamp(text) => text.into(),
        Type::Instant => return Err(bad("an instant, YYYY-MM-DDTHH:MM:SSZ")),
        Type::Link(kinds) => {
            let id = Id::parse(text).map_err(|_| bad("a path id"))?;
            let target = index.header(id.as_str())?.and_then(|h| h["kind"].as_str()?.parse::<Kind>().ok());
            match target {
                Some(k) if kinds.contains(&k) => id.as_str().into(),
                Some(k) => {
                    let want: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
                    return Err(bad(&format!("the id of kind {}; {id} is kind {k}", want.join(" or "))));
                }
                None => {
                    return Err(Refusal::new(
                        Code::UnresolvedLink,
                        format!("{kind} {name}={text}"),
                        "a write adds no link that resolves to nothing in the proposed tree (12, Write path)",
                        format!(
                            "point `{name}` at an id that exists: bp search {}",
                            text.rsplit('/').next().unwrap_or(text)
                        ),
                    ));
                }
            }
        }
    })
}
