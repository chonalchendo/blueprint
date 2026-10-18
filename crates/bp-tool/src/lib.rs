//! The tool: the 18 verbs, the write path composition, the envelope. This milestone ships the read verbs over the
//! files; `list` and `search` grow the index with their issue. Every verb prints one JSON object on stdout with `--json`; diagnostics go to stderr.

// The permitted edges of the crate rule, re-exported so the dependency graph is real before the verbs use them.
pub use bp_change;
pub use bp_check;
pub use bp_design;
pub use bp_forge;
pub use bp_install;
pub use bp_operation;
pub use bp_reasoning;

use std::path::Path;
use std::process::ExitCode;

use bp_vault::{CONTRACT, Filter, Id, Index, Refusal, Vault};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;

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
}

/// `bp show <id> [--section <heading>]`.
#[derive(Args)]
pub struct Show {
    /// The path id, for example `projects/bp/issues/0411.pair-each-data-model-level-with-its`.
    pub id: String,
    /// One `## ` section by its heading, instead of the whole body.
    #[arg(long)]
    pub section: Option<String>,
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
    /// Files, nodes per kind and records per kind, instead of rows.
    #[arg(long, conflicts_with_all = ["kind", "status", "project", "under"])]
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
    /// The git worktree, once the repo verbs land.
    pub worktree: Option<String>,
    /// The contract version.
    pub contract_version: u32,
    /// The binary version.
    pub version: &'static str,
}

/// Runs one verb from `cwd` and returns the exit code: 0 ok, 1 error, 2 refused, 3 no-op, 4 usage.
pub fn run(cli: &Cli, cwd: &Path, out: &mut impl std::io::Write) -> ExitCode {
    let result = match &cli.verb {
        Verb::Where => where_(cwd),
        Verb::Show(show) => show_(cwd, show),
        Verb::List(list) => list_(cwd, list),
        Verb::Search(search) => search_(cwd, search),
    };
    match result {
        Ok(mut value) => {
            // The contract (15): `contract_version` appears in every --json result.
            if let Value::Object(m) = &mut value {
                m.insert("contract_version".into(), CONTRACT.into());
            }
            emit(out, &value, cli.json);
            ExitCode::SUCCESS
        }
        Err(refusal) => {
            emit(out, &refusal, cli.json);
            ExitCode::from(refusal.exit)
        }
    }
}

fn where_(cwd: &Path) -> Result<serde_json::Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let w = Where {
        project: vault.project_of(cwd),
        vault: vault.root.display().to_string(),
        worktree: None,
        contract_version: CONTRACT,
        version: env!("CARGO_PKG_VERSION"),
    };
    Ok(serde_json::to_value(w).expect("a plain struct serialises"))
}

fn show_(cwd: &Path, show: &Show) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let id = Id::new(&show.id);
    let file = vault.read(&id)?;
    let mut m = serde_json::Map::new();
    m.insert("id".into(), id.as_str().into());
    m.insert("record".into(), file.is_record().into());
    m.insert("hash".into(), file.hash.clone().into());
    if let Some(header) = &file.header {
        m.extend(header.to_json());
    }
    m.insert("lead".into(), file.body.lead.clone().into());
    let headings: Vec<&str> = file.body.sections.iter().map(|s| s.heading.as_str()).collect();
    if let Some(heading) = &show.section {
        let section = file.body.section(heading).ok_or_else(|| {
            Refusal::new(
                "no_section",
                format!("{id}#{heading}"),
                "a section is one `## ` heading of the body, matched exactly",
                format!("use one of: {}", headings.join(", ")),
            )
        })?;
        m.insert("section".into(), section.text.clone().into());
    } else {
        m.insert("sections".into(), headings.into());
        m.insert("body".into(), file.body.text.clone().into());
    }
    Ok(Value::Object(m))
}

fn list_(cwd: &Path, list: &List) -> Result<Value, Refusal> {
    let vault = Vault::discover(cwd)?;
    let mut index = Index::open(&vault)?;
    let Some(kind) = &list.kind else {
        index.reconcile(&vault)?;
        return Ok(serde_json::to_value(index.counts()?).expect("a plain struct serialises"));
    };
    let filter = Filter {
        kind,
        status: list.status.as_deref(),
        project: list.project.as_deref(),
        under: list.under.as_deref(),
        limit: list.limit,
    };
    let (rows, total) = index.list(&filter)?;
    Ok(serde_json::json!({ "rows": rows, "total": total, "truncated": total > rows.len() }))
}

fn search_(cwd: &Path, search: &Search) -> Result<Value, Refusal> {
    let index = Index::open(&Vault::discover(cwd)?)?;
    let rows = index.search(&search.terms, search.kind.as_deref(), search.project.as_deref(), search.limit)?;
    Ok(serde_json::json!({ "rows": rows }))
}

fn emit(out: &mut impl std::io::Write, value: &impl Serialize, json: bool) {
    let text = if json {
        serde_json::to_string(value).expect("a plain struct serialises")
    } else {
        serde_json::to_string_pretty(value).expect("a plain struct serialises")
    };
    // A closed stdout (a pipe that went away) is not an error the tool can act on.
    let _ = writeln!(out, "{text}");
}
