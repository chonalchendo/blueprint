//! `bp create <kind>`: one node from flags, keyed by its slug, one commit (15, create). A retried create that
//! finds its own node exits 3 instead of making a second (14).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_vault::{Code, File, Header, Id, Index, Kind, Refusal, SLUG_MAX, Vault};
use clap::Args;

use crate::link::{resolve, search_terms};
use crate::{Wrote, intent, kind_arg, node_id, schema, set};

/// `bp create <kind> --title <t> [--slug <s>] [--under <id>] [--body-file <f>] [--project <p>] [--dry-run]`.
#[derive(Args)]
pub struct Create {
    /// The node kind: project, design, milestone, issue, thread, decision, convention, learning, inbox.
    pub kind: String,
    /// The title; the slug comes from it unless `--slug` names one.
    #[arg(long)]
    pub title: String,
    /// The slug, as written: the last segment of the id, or `containers/<slug>` and `components/<slug>` for a
    /// design.
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
    let under = create.under.as_deref().map(node_id).transpose()?;
    let body = match &create.body_file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| Refusal::failed(path.display().to_string(), format!("{e}; pass a readable UTF-8 file")))?,
        None => String::new(),
    };
    let vault = Vault::discover(cwd)?;
    let id = home(&vault, kind, create)?;
    let role = set::role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let writer = if create.dry_run { None } else { Some(vault.writer()?) };
    let index = Index::open(&vault)?;
    let header = Header {
        kind,
        title: Some(create.title.clone()),
        status: Some(status.into()),
        under: under.clone(),
        aliases: Vec::new(),
        links: BTreeMap::new(),
        unresolved: Vec::new(),
        fields: BTreeMap::new(),
    };
    let body = body.trim();
    let text = if body.is_empty() {
        format!("---\n{}---\n", header.emit())
    } else {
        format!("---\n{}---\n\n{body}\n", header.emit())
    };
    let new = File::from_bytes(id.clone(), text.as_bytes())?;
    if let Some(already) = taken(&vault, &index, &id, &new)? {
        return Ok(already);
    }
    parent(&index, kind, &id, under.as_ref())?;
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
/// `--slug` or the title. An inbox slug starts with the day's date (13).
fn home(vault: &Vault, kind: Kind, create: &Create) -> Result<Id, Refusal> {
    let schema = |fix: String| {
        Refusal::new(
            Code::SchemaInvalid,
            format!("create {kind}"),
            "a node's id follows its kind's layout: its project or shared/, its kind's directory, a slug of at most \
             80 characters (12, Layout; 12, Identity)",
            fix,
        )
    };
    let slug = match &create.slug {
        Some(s) => s.clone(),
        None if kind == Kind::Inbox => {
            let date = date(intent::now()?);
            let words = bp_vault::slug(&create.title, SLUG_MAX - date.len() - 1);
            if words.is_empty() { String::new() } else { format!("{date}-{words}") }
        }
        None => bp_vault::slug(&create.title, SLUG_MAX),
    };
    if slug.is_empty() {
        return Err(schema("the title holds no letter or digit to mint a slug from; pass --slug <slug>".into()));
    }
    let last = slug.rsplit('/').next().unwrap_or_default();
    if last.len() > SLUG_MAX {
        return Err(schema(format!("pass a --slug of at most {SLUG_MAX} characters")));
    }
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
    let id = Id::parse(&text).map_err(|why| schema(format!("{why}; pass another --slug")))?;
    if !kind.fits(&id) {
        let fix = if kind == Kind::Design {
            "pass --slug product, system, data-model, glossary, containers/<slug> or components/<slug>"
        } else {
            "pass a --slug of one path segment"
        };
        return Err(schema(format!("{id} does not fit kind {kind}: {fix}")));
    }
    if let Some(p) = id.project().filter(|_| kind != Kind::Project)
        && !vault.root.join("projects").join(p).join("project.md").is_file()
    {
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("project {p}"),
            "a node sits in a project that exists (12, Layout)",
            "bp list project --json",
        ));
    }
    Ok(id)
}

/// The answer when a node, record or live alias already holds `id` (25, design change 2): exit 3 when it is this
/// create's own node, so a retry makes no second one (14), else `id_taken`. The same node has the same kind, title,
/// parent and body; a status, a label or a link a later verb wrote does not make it another.
fn taken(vault: &Vault, index: &Index, id: &Id, new: &File) -> Result<Option<Wrote>, Refusal> {
    let holder = match vault.text(id) {
        Ok(text) => {
            let old = File::from_bytes(id.clone(), text.as_bytes()).ok();
            let key =
                |f: &File| f.header.as_ref().map(|h| (h.kind, h.title.clone(), h.under.clone(), f.body.text.clone()));
            if old.as_ref().and_then(key).is_some_and(|k| Some(k) == key(new)) {
                return Ok(Some(Wrote::Already(serde_json::json!({ "id": id }))));
            }
            id.clone()
        }
        Err(e) if e.code == Code::NoNode => match index.aliased(id)? {
            Some(holder) => holder,
            None => return Ok(None),
        },
        Err(e) => return Err(e),
    };
    Err(Refusal::new(
        Code::IdTaken,
        id.as_str(),
        "an id names one node or record for good: a node, record or live alias holds it (13, rule 1)",
        format!("bp show {holder} --no-body; pass another --slug"),
    ))
}

/// The parent rule (13): a kind with a spine has a parent of a kind it allows, and a kind without one has none.
fn parent(index: &Index, kind: Kind, id: &Id, under: Option<&Id>) -> Result<(), Refusal> {
    let allowed = schema::parents(kind);
    let names: Vec<&str> = allowed.iter().map(|k| k.as_str()).collect();
    let refuse = |fix: String| {
        Refusal::new(Code::SchemaInvalid, format!("{id} under"), "a node sits under a kind its own allows (13)", fix)
    };
    let Some(under) = under else {
        if allowed.is_empty() {
            return Ok(());
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
    Ok(())
}

/// The UTC date of Unix seconds, `YYYY-MM-DD`: the civil date from days (Howard Hinnant's algorithm), so no
/// calendar crate is needed for one date.
fn date(secs: i64) -> String {
    let days = secs.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_date_is_the_utc_civil_date() {
        assert_eq!(date(1_792_281_600), "2026-10-18");
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_792_281_599), "2026-10-17");
    }
}
