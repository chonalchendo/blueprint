//! `bp record <kind> <parent>`: one append-only record under its parent, one new file, one commit (15, record; 25,
//! design change 6). No verb rewrites a record: a correction is a new record that cites the old one (13, rule 6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_change::DispositionAs;
use bp_design::ConcernState;
use bp_operation::Role;
use bp_reasoning::Outcome;
use bp_vault::{Code, Header, Id, Index, Kind, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::diagram::Refused;
use crate::{Wrote, subject};

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
    /// The record's body, from a file: a delta's proposed design body, a review's findings, a handoff's text.
    #[arg(long)]
    pub body_file: Option<PathBuf>,
    /// A delta's diagram as `<name>=<spec file>`: added, or replaced when its design has it. Repeat for each.
    #[arg(long = "diagram", value_name = "NAME=FILE")]
    pub diagrams: Vec<String>,
    /// A diagram the delta removes from its design, with its page. Repeat for each.
    #[arg(long = "remove-diagram", value_name = "NAME")]
    pub remove_diagrams: Vec<String>,
    /// A delta that changes only wording and layout: no milestone; the human accepts it with `bp stamp <delta>`.
    #[arg(long)]
    pub editorial: bool,
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
    /// A whole number from 1.
    Int,
    /// A full commit sha: 40 or 64 lower-case hex digits, so one commit has one key (13, Review).
    Sha,
    /// Lower-case letters, digits and `-`: it may name a file.
    Slug,
    Date,
    Instant,
    /// The id of a node or record of one of these kinds.
    Link(&'static [Kind]),
}

/// What names a record within its kind's directory (13). A stamp key and a second reading of one head take `-N`; an
/// identity key refuses `id_taken` when taken, and an identical retry exits 3 (25, Goal).
enum Key {
    /// The record's instant, UTC to the second with `-` for `:`.
    Stamp,
    /// `<concern>/<stamp>`: one file per state of one concern.
    Concern,
    /// A review's head; a second reading of it takes `-N`.
    Head,
    /// A disposition's finding: an identity.
    Finding,
    /// `<date>-<short session id>`, the date from `started_at` or the record's instant: an identity.
    Handoff,
}

/// One record kind (13): the key, the field that takes the record's instant, and the fields `--field` sets, each
/// with its type and whether it is required. The parent kinds and the directory are the kind's own (`Kind`).
struct Spec {
    key: Key,
    now: Option<&'static str>,
    fields: Vec<(&'static str, Type, bool)>,
}

fn closed<T: Copy>(all: &[T], text: fn(T) -> &'static str) -> Type {
    Type::Closed(all.iter().map(|v| text(*v)).collect())
}

fn spec(kind: Kind) -> Option<Spec> {
    let (key, now, fields) = match kind {
        Kind::Delta => (
            Key::Stamp,
            Some("stamp"),
            vec![
                // Required unless the delta is editorial (`editorial::usage`).
                ("milestone", Type::Link(&[Kind::Milestone]), false),
                ("change", Type::Text, false),
                ("cause", Type::Link(&[Kind::Thread, Kind::Issue, Kind::Run]), false),
                ("pull_request", Type::Int, false),
                ("authorized_by", Type::Link(&[Kind::Decision]), false),
                // The tool writes it (`delta::propose`); a caller who names it names the chain's tip.
                ("chains", Type::Link(&[Kind::Delta]), false),
            ],
        ),
        Kind::Review => (
            Key::Head,
            Some("at"),
            vec![
                ("head", Type::Sha, true),
                ("patch_id", Type::Sha, false),
                ("pull_request", Type::Int, false),
                ("role", closed(Role::ALL, Role::as_str), false),
            ],
        ),
        Kind::Disposition => (
            Key::Finding,
            None,
            vec![
                ("finding", Type::Int, true),
                ("as", closed(DispositionAs::ALL, DispositionAs::as_str), true),
                ("reason", Type::Text, false),
                ("head", Type::Sha, false),
            ],
        ),
        Kind::ConcernAnswer => (
            Key::Concern,
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
            Key::Stamp,
            Some("at"),
            vec![
                ("outcome", closed(Outcome::ALL, Outcome::as_str), true),
                ("issue", Type::Link(&[Kind::Issue]), false),
                ("note", Type::Text, false),
            ],
        ),
        Kind::Handoff => (
            Key::Handoff,
            None,
            vec![
                ("session", Type::Slug, true),
                ("started_at", Type::Instant, false),
                ("ended_at", Type::Instant, false),
                ("resumes", Type::Link(&[Kind::Handoff]), false),
                ("seed", Type::Link(&bp_operation::NODES), false),
            ],
        ),
        _ => return None,
    };
    Some(Spec { key, now, fields })
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

pub(crate) fn record(cwd: &Path, r: &Record) -> Result<Wrote, Refused> {
    let kind = r.kind.parse::<Kind>().map_err(|_| not_recorded(&r.kind))?;
    let spec = spec(kind).ok_or_else(|| not_recorded(&r.kind))?;
    let given = given(kind, &spec, &r.fields)?;
    crate::editorial::usage(kind, r.editorial, given.contains_key("milestone"))?;
    if r.external.as_deref().is_some_and(|e| e.trim().is_empty()) {
        return Err(Refusal::usage("--external", "give the forge URI the record names, or leave the flag out").into());
    }
    let diagrams = crate::delta::given(cwd, kind, &r.diagrams, &r.remove_diagrams)?;
    let body = body(kind, r.body_file.as_deref())?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let now = crate::clock::now()?;
    // The lock comes before the reads, so the key this write takes is still free when it commits.
    let writer = if r.dry_run { None } else { Some(vault.writer()?) };
    let mut opened = None;
    let index = crate::index(&vault, writer.as_ref(), &mut opened)?;
    let parent = if kind == Kind::Handoff && r.parent == "shared" { None } else { Some(subject(index, &r.parent)?) };
    parent.as_ref().map_or(Ok(()), |p| parent_fits(&vault, index, kind, p))?;
    let mut fields = BTreeMap::new();
    for (name, ty, _) in &spec.fields {
        let Some(text) = given.get(name) else { continue };
        fields.insert((*name).to_string(), value(index, kind, name, ty, text)?);
    }
    let milestone = fields.get("milestone").and_then(Value::as_str).map(str::to_string);
    let onto = match &parent {
        Some(design) if kind == Kind::Delta => {
            Some(crate::delta::propose(&vault, index, design, milestone.as_deref(), &body, &diagrams, &mut fields)?)
        }
        _ => None,
    };
    // A handoff's seed is the node of the session's latest vault commit, which the next session start walks (13).
    if kind == Kind::Handoff
        && !fields.contains_key("seed")
        && let Some(seed) = seed(&vault, index, given["session"])?
    {
        fields.insert("seed".into(), seed.as_str().into());
    }
    if let Some(field) = spec.now {
        fields.insert(field.into(), crate::clock::instant(now).into());
    }
    if let Some(uri) = &r.external {
        fields.insert("external".into(), uri.as_str().into());
    }
    let key = key(&spec.key, &given, &fields, now);
    // A record of the project's own sits in the project's kind directory (13, rule 4).
    let base = match &parent {
        Some(p) if kind.parents() == [Kind::Project] => format!("projects/{}", p.project().expect("a project node")),
        Some(p) => p.to_string(),
        None => "shared".into(),
    };
    let stem = format!("{base}/{}/{key}", kind.dir());
    let identity = matches!(spec.key, Key::Finding | Key::Handoff);
    let id = if identity {
        Id::parse(&stem).expect("a parent id and a slug key make an id")
    } else {
        free(&vault.root, &stem)
    };
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
    // The parent and each link target are committed, so the record cites HEAD, not a human's work in progress
    // (25, design change 1); the record's own path is clean too. A dry run refuses what the write would.
    let links = spec.fields.iter().filter(|(_, ty, _)| matches!(ty, Type::Link(_)));
    let targets: Vec<Id> = links
        .filter_map(|(name, _, _)| Id::parse(header.fields.get(*name)?.as_str()?).ok())
        .chain(parent.clone())
        .chain([id.clone()])
        .chain(onto.as_ref().and_then(|o| o.stale.clone()))
        .collect();
    match &writer {
        Some(w) => w.clean(&targets)?,
        None => vault.clean(&targets)?,
    }
    crate::review_rows::check(index, kind, parent.as_ref(), &given, &body)?;
    if identity && let Some(already) = taken(&vault.root, &id, &text)? {
        return Ok(already);
    }
    let mut changes = BTreeMap::from([(id.clone(), text)]);
    let mut result = serde_json::json!({ "id": id, "kind": kind, "under": parent });
    if kind == Kind::Review {
        result["rows"] = bp_read::inputs::rows(&body).into();
    }
    let moves = crate::delta::close(&vault, &id, onto.as_ref(), &diagrams, now, &mut changes, &mut result)?;
    let Some(writer) = writer else {
        let (_, findings) = index.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit_moving("record", role.as_str(), &format!("record {kind} {id}"), &changes, &moves)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The `--body-file` text; a delta's is the proposed design body, so it is required.
fn body(kind: Kind, file: Option<&Path>) -> Result<String, Refusal> {
    let body = file.map_or_else(|| Ok(String::new()), crate::create::body_file)?;
    if kind == Kind::Delta && body.trim().is_empty() {
        let fix = "a delta's body is the proposed design body: pass --body-file <f>";
        return Err(schema(format!("{kind} body"), fix.into()));
    }
    Ok(body)
}

/// The record's key within its kind's directory.
fn key(key: &Key, given: &BTreeMap<&str, &str>, fields: &BTreeMap<String, Value>, now: i64) -> String {
    let stamp = crate::clock::instant(now).replace(':', "-");
    match key {
        Key::Stamp => stamp,
        Key::Concern => format!("{}/{stamp}", given["concern"]),
        Key::Head => given["head"].to_string(),
        // The number as it parsed, so `01` and `1` name one finding.
        Key::Finding => fields["finding"].to_string(),
        Key::Handoff => {
            let date = fields
                .get("started_at")
                .and_then(Value::as_str)
                .map_or_else(|| crate::clock::instant(now), str::to_string);
            let session = fields["session"].as_str().expect("a session is a slug");
            format!("{}-{}", &date[..10], session.chars().take(8).collect::<String>())
        }
    }
}

/// The node of `session`'s latest vault commit (`Bp-Session`, 12): the first id its subject names, a record followed
/// up to the node it sits under. `None` when the session committed nothing, or the node is gone.
fn seed(vault: &Vault, index: &Index, session: &str) -> Result<Option<Id>, Refusal> {
    // A session is a slug, so it holds no character the pattern reads.
    let pattern = format!("--grep=^Bp-Session: {session}$");
    let subject = crate::claim::git(&vault.root, &["log", "-1", "--format=%s", &pattern])?;
    let Some(mut id) = subject.split_whitespace().find_map(|w| Id::parse(w).ok()) else { return Ok(None) };
    loop {
        let Some((held, _)) = index.holder(&id)? else { return Ok(None) };
        let Some(header) = index.header(held.as_str())? else { return Ok(None) };
        if !header.get("kind").and_then(Value::as_str).and_then(|k| k.parse().ok()).is_some_and(Kind::is_record) {
            return Ok(Some(held));
        }
        let Some(under) = header.get("under").and_then(Value::as_str).and_then(|u| Id::parse(u).ok()) else {
            return Ok(None);
        };
        id = under;
    }
}

/// An identity key that a record already holds: exit 3 when it holds these bytes, else `id_taken` (13).
fn taken(root: &Path, id: &Id, text: &str) -> Result<Option<Wrote>, Refusal> {
    let Ok(held) = std::fs::read_to_string(id.path(root)) else { return Ok(None) };
    if held == text {
        return Ok(Some(Wrote::Already(serde_json::json!({ "id": id }))));
    }
    Err(Refusal::new(
        Code::IdTaken,
        id.as_str(),
        "a disposition is one per finding and a handoff one per session; a record is never rewritten (13)",
        format!("bp show {id}; a correction is a new record that cites it"),
    ))
}

pub(crate) fn schema(subject: String, fix: String) -> Refusal {
    Refusal::new(Code::SchemaInvalid, subject, "a record holds its kind's fields (13, the record tables)", fix)
}

/// The `--field` flags by name: each once, each a field of the kind, every required one present.
fn given<'a>(kind: Kind, spec: &Spec, flags: &'a [String]) -> Result<BTreeMap<&'a str, &'a str>, Refusal> {
    let mut given = BTreeMap::new();
    for f in flags {
        let Some((name, value)) = f.split_once('=') else {
            return Err(Refusal::usage(format!("--field {f}"), "write each field as --field name=value"));
        };
        if name.is_empty() {
            return Err(Refusal::usage(format!("--field {f}"), "write each field as --field name=value"));
        }
        if given.insert(name, value).is_some() {
            return Err(Refusal::usage(format!("--field {name}"), "give each field once"));
        }
    }
    let names: Vec<&str> = spec.fields.iter().map(|(n, _, _)| *n).collect();
    let takes = || {
        let tool = spec.now.map_or_else(String::new, |f| format!("; the tool writes {f}"));
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
fn parent_fits(vault: &Vault, index: &Index, kind: Kind, parent: &Id) -> Result<(), Refusal> {
    match index.header(parent.as_str())?.and_then(|h| h["kind"].as_str()?.parse::<Kind>().ok()) {
        None => Err(vault.unparsed(parent).unwrap_or_else(|| {
            Refusal::new(
                Code::NoNode,
                parent.as_str(),
                "a record sits under a node or record that exists (13, rule 4)",
                format!(
                    "bp search {}",
                    parent.as_str().rsplit('/').next().unwrap_or_default().replace(['-', '.'], " ")
                ),
            )
        })),
        Some(k) if !kind.parents().contains(&k) => {
            let want: Vec<&str> = kind.parents().iter().map(|k| k.as_str()).collect();
            Err(schema(
                parent.to_string(),
                format!("kind {kind} sits under kind {}; {parent} is kind {k}", want.join(" or ")),
            ))
        }
        Some(_) => Ok(()),
    }
}

/// The id `stem` names, or the first free `-2`, `-3`: a second record with one key under one parent (25, design
/// change 6).
pub(crate) fn free(root: &Path, stem: &str) -> Id {
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
        Type::Int => text.parse::<u64>().ok().filter(|n| *n >= 1).ok_or_else(|| bad("a whole number from 1"))?.into(),
        Type::Sha
            if matches!(text.len(), 40 | 64)
                && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            text.into()
        }
        Type::Sha => return Err(bad("a full commit sha: 40 or 64 lower-case hex digits")),
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
