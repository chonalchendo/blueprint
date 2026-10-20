//! `bp create <kind>`: one node from flags, keyed by its slug, one commit (15, create). A retried create that
//! finds its own node exits 3 instead of making a second (14).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_vault::{Code, File, Header, Id, Index, Kind, Refusal, SLUG_MAX, Vault, is_slug};
use clap::Args;

use crate::link::{resolve, search_terms};
use crate::{Wrote, clock, kind_arg, node_id, schema, set};

/// `bp create <kind> --title <t> [--slug <s>] [--under <id>] [--body-file <f>] [--project <p>] [--dry-run]`.
#[derive(Args)]
pub struct Create {
    /// The node kind: project, design, milestone, issue, thread, decision, convention, learning, inbox.
    pub kind: String,
    /// The title; the slug comes from it unless `--slug` names one.
    #[arg(long)]
    pub title: String,
    /// The slug: the last segment of the id, or `containers/<slug>` and `components/<slug>` for a design.
    /// Lowercase ASCII letters, digits, `-` and `.`, at most 80 characters; an inbox slug gains the date.
    #[arg(long)]
    pub slug: Option<String>,
    /// The parent: the milestone of an issue, the design of a milestone, the design or project of a design.
    #[arg(long)]
    pub under: Option<String>,
    /// The body, read from this file.
    #[arg(long)]
    pub body_file: Option<PathBuf>,
    /// The project; issues, milestones and designs need one, and threads, decisions and inbox entries may have one.
    #[arg(long)]
    pub project: Option<String>,
    /// Validate and print what would be written; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) fn create(cwd: &Path, create: &Create) -> Result<Wrote, Refusal> {
    let kind = kind_arg(&create.kind)?;
    let Some(status) = schema::first_status(kind) else {
        return Err(Refusal::usage(
            format!("create {kind}"),
            format!("kind {kind} is a record, written by its own verb: bp record {kind} <parent>, bp check or bp cut"),
        ));
    };
    schema::title(&format!("create {kind} title"), &create.title)?;
    let under = create.under.as_deref().map(node_id).transpose()?;
    let body = create.body_file.as_deref().map(body_file).transpose()?.unwrap_or_default();
    let vault = Vault::discover(cwd)?;
    let id = home(kind, create)?;
    let role = set::role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let mut writer = if create.dry_run { None } else { Some(vault.writer()?) };
    // A file at the id that is not committed refuses first, dry run or not, before anything is read or compared
    // (12, Write path).
    match &mut writer {
        Some(w) => w.clean([&id])?,
        None => vault.clean([&id])?,
    }
    let index = Index::open(&vault)?;
    if let Some(p) = id.project().filter(|_| kind != Kind::Project)
        && index.header(&format!("projects/{p}/project"))?.is_none()
    {
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("project {p}"),
            "a node sits in a project that exists, named exactly (12, Layout)",
            "bp list project --json",
        ));
    }
    let under = parent(&index, kind, &id, under.as_ref())?;
    let header = Header {
        kind,
        title: Some(create.title.clone()),
        status: Some(status.into()),
        under,
        aliases: Vec::new(),
        links: BTreeMap::new(),
        unresolved: Vec::new(),
        fields: BTreeMap::new(),
    };
    let body = stored(&body);
    let text = if body.is_empty() {
        format!("---\n{}---\n", header.emit())
    } else {
        format!("---\n{}---\n\n{body}\n", header.emit())
    };
    let new = File::from_bytes(id.clone(), text.as_bytes())?;
    if let Some(already) = taken(&vault, &index, &id, &new)? {
        return Ok(already);
    }
    let changes = BTreeMap::from([(id.clone(), text)]);
    let mut result = serde_json::json!({ "id": id, "kind": kind, "status": status });
    let Some(writer) = writer else {
        let (_, findings) = index.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit("create", role.as_str(), &format!("create {kind} {id}"), &changes)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The id a new node takes (12, Layout): its kind's directory in its project, or in `shared/`, and the slug from
/// `--slug` or the title. A slug is lowercase ASCII letters, digits, `-` and `.`, at most 80 characters; an inbox
/// slug starts with the UTC date, override or not (13, rule 1). A `--slug` outside the grammar refuses; it is never
/// cut or rewritten.
fn home(kind: Kind, create: &Create) -> Result<Id, Refusal> {
    let schema = |fix: String| {
        Refusal::new(
            Code::SchemaInvalid,
            format!("create {kind}"),
            "a node's id is its project or shared/, its kind's directory and a slug: lowercase ASCII letters, digits, \
             `-` and `.`, starting with a letter or digit, at most 80 characters (12, Layout; 13, rule 1)",
            fix,
        )
    };
    let date = if kind == Kind::Inbox { Some(clock::date(clock::now()?)) } else { None };
    // A letter outside ASCII would split its word: `Café` mints `caf`.
    let foreign = create.title.chars().any(|c| !c.is_ascii() && c.is_alphabetic());
    let minted = match &date {
        _ if foreign => String::new(),
        Some(d) => {
            let words = bp_vault::slug(&create.title, SLUG_MAX - d.len() - 1);
            if words.is_empty() { words } else { format!("{d}-{words}") }
        }
        None => bp_vault::slug(&create.title, SLUG_MAX),
    };
    // A refused `--slug` shows what the title mints instead.
    let example = match (create.slug.as_deref().and_then(|s| s.rsplit_once('/')), minted.is_empty()) {
        (_, true) => String::new(),
        (Some((dir, _)), false) => format!(", for example --slug {dir}/{minted}"),
        (None, false) => format!(", for example --slug {minted}"),
    };
    let slug = match &create.slug {
        Some(s) => {
            let s = match &date {
                Some(d) if !dated(s) => format!("{d}-{s}"),
                _ => s.clone(),
            };
            if !s.split('/').all(is_slug) {
                return Err(schema(format!("{s:?} is not a slug: pass a --slug in that grammar{example}")));
            }
            s
        }
        None if foreign => {
            return Err(schema(
                "the title holds a letter outside ASCII, which would split its word in a slug; pass --slug <slug>"
                    .into(),
            ));
        }
        None if minted.is_empty() => {
            return Err(schema("the title holds no letter or digit to mint a slug from; pass --slug <slug>".into()));
        }
        None => minted,
    };
    let text = match (kind, &create.project) {
        (Kind::Project, None) => format!("projects/{slug}/project"),
        (Kind::Project, Some(_)) => return Err(schema("the slug names the project; drop --project".into())),
        (Kind::Convention | Kind::Learning, Some(_)) => {
            return Err(schema(format!(
                "kind {kind} is shared: drop --project; a project learning cites the project's design"
            )));
        }
        (Kind::Issue | Kind::Milestone | Kind::Design, None) => {
            return Err(schema(format!(
                "kind {kind} sits in a project: pass --project <project>; bp list project --json"
            )));
        }
        (_, Some(p)) => format!("projects/{p}/{}/{slug}", kind.dir()),
        (_, None) => format!("shared/{}/{slug}", kind.dir()),
    };
    // `Id::parse` strips `.md` and link syntax; an id it would change is not the one asked for.
    let id = Id::parse(&text)
        .ok()
        .filter(|id| id.as_str() == text)
        .ok_or_else(|| schema(format!("{text:?} is not a path id: pass another --slug or --project{example}")))?;
    if !kind.fits(&id) {
        let fix = if kind == Kind::Design {
            "pass --slug product, system, data-model, glossary, containers/<slug> or components/<slug>"
        } else {
            "pass a --slug of one path segment"
        };
        return Err(schema(format!("{id} does not fit kind {kind}: {fix}")));
    }
    Ok(id)
}

/// Whether a slug starts with a date and a dash, `YYYY-MM-DD-`, as an inbox slug does.
fn dated(slug: &str) -> bool {
    let b = slug.as_bytes();
    b.len() > 11
        && b[..11]
            .iter()
            .enumerate()
            .all(|(i, c)| if matches!(i, 4 | 7 | 10) { *c == b'-' } else { c.is_ascii_digit() })
}

/// The text of a `--body-file`, which `create` and `set` read. A file that opens with a header is a node's file, not
/// a body: stored, it would nest a second header under the node's own, and each repeat one more.
pub(crate) fn body_file(path: &Path) -> Result<String, Refusal> {
    let text = std::fs::read_to_string(path)
        // The caller named a file the tool cannot read: a usage error, not a failure of the tool.
        .map_err(|e| Refusal::usage(path.display().to_string(), format!("{e}; pass a readable UTF-8 file")))?;
    if bp_vault::opens_with_header(&stored(&text)) {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            path.display().to_string(),
            "a body file holds the body only; the header is the node's, and its verbs write it (25, issue 7)",
            "pass the body only, without the header: bp show <id> --json | jq -r .body > <f>",
        ));
    }
    Ok(text)
}

/// The body as stored: no BOM, LF line endings, no leading blank lines and no trailing whitespace. The first line
/// keeps its indentation, so a body that opens with an indented code block stays one.
pub(crate) fn stored(body: &str) -> String {
    let body = bp_vault::normalise(body);
    let mut rest = body.as_str();
    while let Some((line, after)) = rest.split_once('\n')
        && line.trim().is_empty()
    {
        rest = after;
    }
    rest.trim_end().to_string()
}

/// The answer when a file or a live alias holds `id`, up to case (25, design change 2): exit 3 when the file is this
/// create's own node, so a retry makes no second one (14), else `id_taken`. The same node has the same kind, title,
/// parent and body; a status, a label or a link a later verb wrote does not make it another.
fn taken(vault: &Vault, index: &Index, id: &Id, new: &File) -> Result<Option<Wrote>, Refusal> {
    let Some((holder, alias)) = index.holder(id)? else { return Ok(None) };
    if !alias {
        let text = vault.text(&holder)?;
        let old = File::from_bytes(holder.clone(), text.as_bytes()).ok();
        let key = |f: &File| f.header.as_ref().map(|h| (h.kind, h.title.clone(), h.under.clone(), f.body.text.clone()));
        if old.as_ref().and_then(key).is_some_and(|k| Some(k) == key(new)) {
            return Ok(Some(Wrote::Already(serde_json::json!({ "id": holder }))));
        }
    }
    Err(Refusal::new(
        Code::IdTaken,
        id.as_str(),
        "an id names one node or record for good, up to case: a file or a live alias holds it (13, rule 1)",
        format!("bp show {holder} --no-body; pass another --slug"),
    ))
}

/// The parent rule (13): a kind with a spine has a parent of a kind it allows, in its own project, and a kind
/// without one has none. Returns the parent's real id, so an alias given as `--under` is written as the node it
/// names.
fn parent(index: &Index, kind: Kind, id: &Id, under: Option<&Id>) -> Result<Option<Id>, Refusal> {
    let allowed = schema::parents(kind);
    let names: Vec<&str> = allowed.iter().map(|k| k.as_str()).collect();
    let refuse = |fix: String| {
        Refusal::new(
            Code::SchemaInvalid,
            format!("{id} under"),
            "a node sits under a kind its own allows, in its own project (13)",
            fix,
        )
    };
    let Some(under) = under else {
        if allowed.is_empty() {
            return Ok(None);
        }
        return Err(refuse(format!("kind {kind} needs a parent: --under <{}>", names.join(" or "))));
    };
    if allowed.is_empty() {
        return Err(refuse(format!("kind {kind} has no parent; drop --under")));
    }
    let Some((target, parent)) = resolve(index, under)? else {
        return Err(Refusal::new(
            Code::UnresolvedLink,
            format!("{id} under {under}"),
            "a write adds no link that resolves to nothing in the proposed tree (12, Write path)",
            format!("point --under at an id that exists: bp search {}", search_terms(under)),
        ));
    };
    if !allowed.contains(&parent) {
        return Err(refuse(format!("kind {kind} sits under a {}; {target} is kind {parent}", names.join(" or "))));
    }
    let project = id.project().expect("a kind with a parent sits in a project (home)");
    if target.project() != Some(project) {
        return Err(refuse(format!("{target} is in another project; pass a parent in project {project}")));
    }
    Ok(Some(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_keeps_its_first_indent_and_loses_its_edges() {
        assert_eq!(stored("\u{feff}\n \n    code\r\nline\r\n\n  \n"), "    code\nline");
        assert_eq!(stored("\n\n  \n"), "");
        assert_eq!(stored("One."), "One.");
    }

    #[test]
    fn an_inbox_slug_is_dated_once() {
        assert!(dated("2026-10-19-a"));
        assert!(!dated("2026-10-19-") && !dated("2026-10-1x-a") && !dated("note"));
    }
}
