//! The tool: the 18 verbs, the write path composition, the envelope. Every verb prints one JSON object on stdout with
//! `--json`; diagnostics go to stderr.

mod carry;
mod check;
mod claim;
mod clock;
mod create;
mod done;
mod install;
mod intent;
mod land;
mod link;
mod record;
mod rename;
mod schema;
mod set;
mod stack;
mod stamp;
mod transition;
mod waiver;
mod walk;

use std::path::Path;
use std::process::ExitCode;

use bp_vault::{Code, Filter, Id, Index, Kind, Refusal, Vault};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;

/// The contract version every `--json` result carries (15). A breaking change to a result bumps it.
pub const CONTRACT: u32 = 1;

/// No limit on a `list` read: every row.
pub(crate) const ALL: usize = usize::MAX >> 1;

/// Every successful result: the verb's own fields and the contract version.
#[derive(Serialize)]
pub(crate) struct Envelope<T: Serialize> {
    #[serde(flatten)]
    pub(crate) result: T,
    pub(crate) contract_version: u32,
}

/// Blueprint: one evolving blueprint per project. The agent's hands.
#[derive(Parser)]
#[command(name = "bp", version, about)]
pub struct Cli {
    /// Print the result as one JSON object. Every verb supports it; the agent always passes it.
    #[arg(long, global = true)]
    pub json: bool,
    /// The verb to run.
    #[command(subcommand)]
    pub verb: Verb,
}

/// The verbs. Read verbs first; the rest land with their milestones.
#[derive(Subcommand)]
pub enum Verb {
    /// Vault, project, worktree, contract version.
    Where,
    /// One node or record by id: its header, lead, section headings and body, or one section.
    Show(Show),
    /// Rows of one kind from the index, with a default limit and `truncated`.
    List(List),
    /// BM25 over ids, titles and bodies; each row has id, kind, rank and one snippet.
    Search(Search),
    /// The ranked slice an action needs, under its budget, with the scope's intents.
    Walk(walk::Walk),
    /// One header label of one node: one changed line, one commit.
    Set(set::Set),
    /// One node from flags, keyed by its slug; a same-content repeat is a no-op.
    Create(create::Create),
    /// One typed edge, added or removed; a repeat is a no-op.
    Link(link::Link),
    /// One status move under the kind's machine: one changed line, one commit.
    Transition(transition::Transition),
    /// One append-only record under its parent: one new file, one commit.
    Record(record::Record),
    /// A new slug for one node: the file and its directory move, every inbound link follows, one commit.
    Rename(rename::Rename),
    /// Claim an open issue: fetch, a worktree and branch at their fixed names, the claim and `in-progress` in one
    /// commit.
    Cut(claim::Cut),
    /// Close the issue's active claim with a reason; the issue returns to `open` and the worktree stays.
    Release(claim::Release),
    /// Push the claim's branch, open or update its pull request, and run and commit the issue's blocks at the head.
    Pr(land::Pr),
    /// Merge the approved, green pull request at its head; release the claim `landed` and resolve the issue.
    Land(land::Land),
    /// Run the owner's check blocks; one run record per block, one commit.
    Check(check::Check),
    /// Accept an approved milestone: its finish line, its deltas written into their designs, a blueprint version and
    /// the milestone at `done` in one commit, and a local tag.
    Stamp(stamp::Stamp),
    /// The bundle into a harness, at the clone's top, with its manifest; local, so it calls no forge.
    Install(install::Install),
}

/// `bp show <id> [--section <heading>] [--no-body] [--at <ref>]`.
#[derive(Args)]
pub struct Show {
    /// The path id, for example `projects/bp/issues/0411.pair-each-data-model-level-with-its`.
    pub id: String,
    /// One `## ` section by its heading, instead of the whole body.
    #[arg(long, conflicts_with = "no_body")]
    pub section: Option<String>,
    /// The header, the lead and the section headings, without the body.
    #[arg(long)]
    pub no_body: bool,
    /// The file as it was at a commit, branch or blueprint tag.
    #[arg(long)]
    pub at: Option<String>,
}

/// `bp list <kind> [filters]` or `bp list --count-only`.
#[derive(Args)]
pub struct List {
    /// The kind, for example `issue`, `milestone`, `claim`.
    #[arg(required_unless_present = "count_only")]
    pub kind: Option<String>,
    /// Exact status.
    #[arg(long)]
    pub status: Option<String>,
    /// The project segment of the id.
    #[arg(long)]
    pub project: Option<String>,
    /// The parent id.
    #[arg(long)]
    pub under: Option<String>,
    /// At most this many rows.
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
    /// Only issues whose intent is `ready`: open, unclaimed, every dependency resolved or holding an open pull
    /// request.
    #[arg(long, conflicts_with = "status")]
    pub ready: bool,
    /// Files, nodes per kind and records per kind, instead of rows.
    #[arg(long, conflicts_with_all = ["kind", "status", "project", "under", "ready"])]
    pub count_only: bool,
}

/// `bp search <terms...>`.
#[derive(Args)]
pub struct Search {
    /// The terms; every term must match, or any term when none matches all.
    #[arg(required = true)]
    pub terms: Vec<String>,
    /// Only this kind.
    #[arg(long)]
    pub kind: Option<String>,
    /// Only this project.
    #[arg(long)]
    pub project: Option<String>,
    /// At most this many rows.
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
}

/// The result of `where`.
#[derive(Serialize)]
pub struct Where {
    /// The vault root.
    pub vault: String,
    /// The project the working directory sits under, if any.
    pub project: Option<String>,
    /// The git worktree that holds the working directory, if any.
    pub worktree: Option<String>,
    /// The binary version.
    pub version: &'static str,
}

/// Runs one verb from `cwd` and returns the exit code: 0 ok, 1 error, 2 refused, 3 no-op, 4 usage.
pub fn run(cli: &Cli, cwd: &Path, out: &mut impl std::io::Write) -> ExitCode {
    match &cli.verb {
        Verb::Where => finish(where_(cwd), cli.json, out),
        Verb::Show(show) => finish(show_(cwd, show), cli.json, out),
        Verb::List(list) => finish(list_(cwd, list), cli.json, out),
        Verb::Search(search) => finish(search_(cwd, search), cli.json, out),
        Verb::Walk(w) => finish(walk::walk(cwd, w), cli.json, out),
        Verb::Set(s) => wrote(set::set(cwd, s), cli.json, out),
        Verb::Create(c) => wrote(create::create(cwd, c), cli.json, out),
        Verb::Link(l) => wrote(link::link(cwd, l), cli.json, out),
        Verb::Transition(t) => wrote(transition::transition(cwd, t), cli.json, out),
        Verb::Record(r) => wrote(record::record(cwd, r), cli.json, out),
        Verb::Rename(r) => wrote(rename::rename(cwd, r), cli.json, out),
        Verb::Cut(c) => wrote(claim::cut(cwd, c), cli.json, out),
        Verb::Release(r) => wrote(claim::release(cwd, r), cli.json, out),
        Verb::Pr(p) => land::run_pr(cwd, p, cli.json, out),
        Verb::Land(l) => wrote(land::land(cwd, l), cli.json, out),
        Verb::Check(c) => check::run(cwd, c, cli.json, out),
        Verb::Stamp(s) => stamp::run(cwd, s, cli.json, out),
        Verb::Install(i) => wrote(install::install(cwd, i), cli.json, out),
    }
}

/// A write verb's exit: 0 for a write, 3 for a repeat, with the id, else the refusal's own.
fn wrote(result: Result<Wrote, Refusal>, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match result {
        Ok(Wrote::Already(v)) => {
            emit(out, &Envelope { result: v, contract_version: CONTRACT }, json);
            ExitCode::from(3)
        }
        Ok(Wrote::Changed(v)) => finish(Ok(v), json, out),
        Err(refusal) => finish::<Value>(Err(refusal), json, out),
    }
}

/// A write verb's result (exit 0), or the id of what the vault already held (exit 3).
enum Wrote {
    Changed(Value),
    Already(Value),
}

pub(crate) fn finish<T: Serialize>(result: Result<T, Refusal>, json: bool, out: &mut impl std::io::Write) -> ExitCode {
    match result {
        Ok(result) => {
            emit(out, &Envelope { result, contract_version: CONTRACT }, json);
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            emit(out, &refusal, json);
            ExitCode::from(refusal.exit)
        }
    }
}

fn where_(cwd: &Path) -> Result<Where, Refusal> {
    let vault = Vault::discover(cwd)?;
    Ok(Where {
        project: vault.project_for(cwd)?,
        vault: vault.root.display().to_string(),
        worktree: bp_vault::git::Repo::find(cwd).map(|r| r.top.display().to_string()),
        version: env!("CARGO_PKG_VERSION"),
    })
}

fn show_(cwd: &Path, show: &Show) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    // A past commit is read as asked; today's index names today's ids only.
    let id = if show.at.is_some() { node_id(&show.id)? } else { subject(&Index::open(&vault)?, &show.id)? };
    let file = match &show.at {
        Some(at) => vault.read_at(&id, at)?,
        None => vault.read(&id)?,
    };
    // A refusal's fix reads the same commit the verb read.
    let at = show.at.as_ref().map_or_else(String::new, |a| format!(" --at {a}"));
    let Some(header) = &file.header else {
        return Err(Refusal::new(
            Code::NoNode,
            id.as_str(),
            "an id names a node or record: a file with a header",
            match &show.at {
                Some(a) => format!(
                    "the file has no header: it is an asset; git -C {} show {a}:./{id}.md",
                    vault.root.display()
                ),
                None => format!("the file has no header: it is an asset; cat {}", id.path(&vault.root).display()),
            },
        ));
    };
    let mut m = serde_json::Map::new();
    m.insert("id".into(), id.as_str().into());
    m.insert("record".into(), header.kind.is_record().into());
    m.insert("hash".into(), file.hash.clone().into());
    m.extend(file.header_json().expect("the file has a header"));
    m.insert("lead".into(), file.body.lead.clone().into());
    if !file.body.findings.is_empty() {
        m.insert("findings".into(), serde_json::to_value(&file.body.findings).expect("a plain struct serialises"));
    }
    let headings: Vec<&str> = file.body.sections.iter().map(|s| s.heading.as_str()).collect();
    if let Some(heading) = &show.section {
        let refuse = |fix: String| {
            Refusal::new(
                Code::NoSection,
                format!("{id}#{heading}"),
                "a section is one top-level `## ` heading of the body, matched as written",
                fix,
            )
        };
        let section = match file.body.sections_named(heading)[..] {
            [one] => one,
            [] => {
                let mut unique: Vec<&str> = Vec::new();
                for h in &headings {
                    if !unique.contains(h) {
                        unique.push(h);
                    }
                }
                return Err(refuse(format!("bp show {id}{at} --no-body; the headings are: {}", unique.join(", "))));
            }
            // Two sections share the heading: the whole body is the only read that holds both.
            _ => return Err(refuse(format!("bp show {id}{at} --json"))),
        };
        m.insert("section".into(), section.text.clone().into());
    } else {
        m.insert("sections".into(), headings.into());
        if !show.no_body {
            m.insert("body".into(), file.body.text.clone().into());
        }
    }
    Ok(Value::Object(m))
}

/// An id argument. Anything that is not a path id under the vault names no node.
pub(crate) fn node_id(text: &str) -> Result<Id, Refusal> {
    Id::parse(text).map_err(|why| {
        Refusal::new(
            Code::NoNode,
            text,
            "an id is a path under projects/ or shared/, without .md",
            format!("{why}; bp search {}", text.rsplit('/').next().unwrap_or(text).replace(['-', '.', '_'], " ")),
        )
    })
}

/// The index a write verb reads through: the writer's, opened under the lock, or for a dry run one opened without
/// it into `opened`.
pub(crate) fn index<'a>(
    vault: &Vault,
    writer: Option<&'a bp_vault::Writer<'_>>,
    opened: &'a mut Option<Index>,
) -> Result<&'a Index, Refusal> {
    match writer {
        Some(w) => Ok(w.index()),
        None => Ok(opened.insert(Index::open(vault)?)),
    }
}

/// A verb's subject: the id a node or record holds today, through the index. Case is ignored, and a renamed node's
/// past id names the node (13, rule 1). An id nothing holds stays as asked, and the verb refuses it `no_node`.
pub(crate) fn subject(index: &Index, text: &str) -> Result<Id, Refusal> {
    let id = node_id(text)?;
    Ok(index.holder(&id)?.map_or(id, |(held, _)| held))
}

fn list_(cwd: &Path, list: &List) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let mut index = Index::open(&vault)?;
    let Some(kind) = &list.kind else {
        index.reconcile(&vault)?;
        return census(&index);
    };
    let under = list.under.as_deref().map(node_id).transpose()?;
    let kind_value = kind_arg(kind)?;
    if let Some(status) = &list.status
        && !schema::status_in_set(kind, Some(status))
    {
        return Err(unknown("status", status, format!("bp list {kind}")));
    }
    if list.ready && kind_value != Kind::Issue {
        return Err(Refusal::usage(
            "--ready",
            format!("--ready lists issues only; bp list issue --ready, or bp list {kind}"),
        ));
    }
    let ready = list.ready;
    let filter = Filter {
        kind: kind_value,
        // `--ready` reads every open issue, then limits after the intent filter.
        status: if ready { Some("open") } else { list.status.as_deref() },
        project: list.project.as_deref(),
        under: under.as_ref().map(Id::as_str),
        limit: if ready { crate::ALL } else { list.limit },
    };
    let (rows, total) = index.list(&filter)?;
    if !matches!(kind_value, Kind::Issue | Kind::Milestone) {
        return Ok(serde_json::json!({ "rows": rows, "total": total, "truncated": total > rows.len() }));
    }
    let intents = intent::Intents::new(&vault, &index, kind_value == Kind::Issue)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let verdict = intents.row(kind_value, &row.id, row.status.as_deref())?;
        if ready && verdict.intent != Some(bp_change::intents::Intent::Ready) {
            continue;
        }
        let mut v = serde_json::to_value(row).expect("a plain struct serialises");
        if let Some(intent) = verdict.intent {
            v["intent"] = serde_json::to_value(intent).expect("an enum serialises");
            v["note"] = verdict.note.into();
        }
        out.push(v);
    }
    let total = if ready { out.len() } else { total };
    out.truncate(list.limit);
    let mut result = serde_json::json!({ "rows": out, "total": total, "truncated": total > out.len() });
    if let Some(w) = intents.world {
        result["forge"] = if w.forge.is_some() { "snapshot" } else { "not read" }.into();
    }
    Ok(result)
}

/// A kind argument, from the closed set.
fn kind_arg(text: &str) -> Result<Kind, Refusal> {
    text.parse().map_err(|_| {
        let kinds: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
        unknown("kind", text, format!("use one of: {}", kinds.join(", ")))
    })
}

pub(crate) fn unknown(what: &str, value: &str, fix: String) -> Refusal {
    Refusal::new(Code::UnknownValue, format!("{what} {value}"), "a kind and a status come from closed sets (13)", fix)
}

/// The counts, and the findings the reader keeps instead of ignoring: statuses outside their kind's set, link
/// targets that are not ids, the body findings, and files that do not parse.
fn census(index: &Index) -> Result<Value, Refusal> {
    let mut v = serde_json::to_value(index.counts()?).expect("a plain struct serialises");
    let outside: usize = index
        .statuses()?
        .iter()
        .filter(|(kind, status, _)| !schema::status_in_set(kind, status.as_deref()))
        .map(|(_, _, n)| n)
        .sum();
    let mut findings = serde_json::json!({ "status_outside_set": outside, "unresolved_link": index.unresolved()? });
    for (kind, n) in index.findings()? {
        findings[kind] = n.into();
    }
    v["findings"] = findings;
    Ok(v)
}

fn search_(cwd: &Path, search: &Search) -> Result<Value, Refusal> {
    let kind = search.kind.as_deref().map(kind_arg).transpose()?;
    let index = Index::open(&Vault::discover(cwd)?)?;
    let rows = index.search(&search.terms, kind, search.project.as_deref(), search.limit)?;
    Ok(serde_json::json!({ "rows": rows }))
}

pub(crate) fn emit(out: &mut impl std::io::Write, value: &impl Serialize, json: bool) {
    let text = if json {
        serde_json::to_string(value).expect("a plain struct serialises")
    } else {
        serde_json::to_string_pretty(value).expect("a plain struct serialises")
    };
    // A closed stdout (a pipe that went away) is not an error the tool can act on.
    let _ = writeln!(out, "{text}");
}
