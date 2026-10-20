//! `bp rename <id> <slug>`: a new slug for one node, one commit (15, rename; 12, A rename). The file and its
//! directory move, every link that names the node is rewritten, and the old slug joins `aliases`, so the old id still
//! resolves and no other node takes it (13, rule 1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bp_vault::{Code, File, Id, Index, Kind, Moves, Refusal, SLUG_MAX, Vault, is_slug, retarget};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, create, node_id, set, subject};

const GRAMMAR: &str = "a slug is lowercase ASCII letters, digits, `-` and `.`, starting with a letter or digit, at most \
                       80 characters (13, rule 1)";

/// `bp rename <id> <slug> [--dry-run]`.
#[derive(Args)]
pub struct Rename {
    /// The node, for example `shared/threads/data-platform.0002-what-production-data-ml-platform-should-burgh-and-mentat-run`.
    pub id: String,
    /// The new last segment of the id: lowercase ASCII letters, digits, `-` and `.`, at most 80 characters.
    pub slug: String,
    /// Validate and print what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) fn rename(cwd: &Path, rename: &Rename) -> Result<Wrote, Refusal> {
    node_id(&rename.id)?;
    let grammar = |slug: &str| {
        let fix = format!("pass a slug in that grammar, for example {}", bp_vault::slug(slug, SLUG_MAX));
        Refusal::new(Code::SchemaInvalid, format!("rename {} {slug}", rename.id), GRAMMAR, fix)
    };
    if !is_slug(&rename.slug) {
        return Err(grammar(&rename.slug));
    }
    let vault = Vault::discover(cwd)?;
    let role = set::role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let writer = if rename.dry_run { None } else { Some(vault.writer()?) };
    let opened;
    let index = if let Some(w) = &writer {
        w.index()
    } else {
        opened = Index::open(&vault)?;
        &opened
    };
    // The index, not the file system, names the node: a case-blind file system would take a wrong-case id as a file.
    let old = subject(index, &rename.id)?;
    let (dir, was) = old.as_str().rsplit_once('/').expect("an id has three segments");
    let slug = inbox_slug(dir, was, &rename.slug)?;
    if !is_slug(&slug) {
        return Err(grammar(&slug));
    }
    let new = Id::parse(&format!("{dir}/{slug}")).expect("a slug keeps an id an id");
    if new == old {
        return Ok(Wrote::Already(serde_json::json!({ "id": old })));
    }
    let mut moves = directory(&vault.root, &old, &new)?;
    moves.ids.insert(new.clone(), old.clone());
    let renamed: BTreeMap<&Id, &Id> = moves.ids.iter().map(|(to, from)| (from, to)).collect();
    let inbound = index.inbound(&old)?;
    // A human's uncommitted edit to any file the rename touches refuses first, dry run or not, before its text is
    // read (12).
    let mut paths: Vec<String> = renamed.iter().flat_map(|(a, b)| [format!("{a}.md"), format!("{b}.md")]).collect();
    paths.extend(moves.files.iter().flat_map(|(a, b)| [a.clone(), b.clone()]));
    paths.extend(inbound.iter().map(|id| format!("{id}.md")));
    match &writer {
        Some(w) => w.clean_paths(paths)?,
        None => vault.clean_paths(&paths)?,
    }
    let text = vault.text(&old)?;
    admit(&vault, index, &File::from_bytes(old.clone(), text.as_bytes())?, &new)?;
    let mut names = vec![old.clone()];
    names.extend(index.aliases_of(&old)?.into_iter().filter(|a| *a != old));
    let rewrite = |text: &str| names.iter().fold(text.to_string(), |t, n| retarget(&t, n.as_str(), new.as_str()));
    let mut changes = BTreeMap::new();
    let files: BTreeSet<&Id> = renamed.keys().copied().chain(&inbound).collect();
    for id in files {
        let before = if *id == old { text.clone() } else { vault.text(id)? };
        let mut after = rewrite(&before);
        if *id == old {
            // The aliases line holds past slugs, not links, so it is written whole, the old slug last. A slug taken
            // back leaves it (13, rule 1).
            let mut aliases = bp_vault::written(&before, "aliases");
            aliases.retain(|a| *a != slug && *a != new.as_str());
            if !aliases.iter().any(|a| a == was) {
                aliases.push(was.to_string());
            }
            after = bp_vault::set_field(&after, "aliases", &Value::from(aliases));
        }
        let to = renamed.get(id).copied().unwrap_or(id);
        if to != id || after != before {
            changes.insert(to.clone(), after);
        }
    }
    let rewritten: Vec<String> =
        changes.keys().filter(|id| !moves.ids.contains_key(*id)).map(|id| format!("{id}.md")).collect();
    let mut result = serde_json::json!({
        "id": new, "was": old, "moved": moves.ids.len() + moves.files.len(), "rewritten": rewritten.len(),
    });
    let Some(writer) = writer else {
        let (_, findings) = index.propose_moving(&vault, &changes, &moves.ids)?;
        let pairs: Vec<Value> = moves
            .ids
            .iter()
            .map(|(to, from)| (format!("{from}.md"), format!("{to}.md")))
            .chain(moves.files.iter().cloned())
            .map(|(from, to)| serde_json::json!({ "from": from, "to": to }))
            .collect();
        result["files"] = serde_json::json!({ "moved": pairs, "rewritten": rewritten });
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit_moving("rename", role.as_str(), &format!("rename {old} {new}"), &changes, &moves)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The slug a rename gives: an inbox slug keeps its creation date (13, rule 1), so a new slug without one takes the
/// old date and a new slug with another date refuses.
fn inbox_slug(dir: &str, was: &str, asked: &str) -> Result<String, Refusal> {
    let date = match was.get(..11) {
        Some(date) if dir.ends_with(Kind::Inbox.dir()) && create::dated(was) => date,
        _ => return Ok(asked.to_string()),
    };
    if !create::dated(asked) {
        return Ok(format!("{date}{asked}"));
    }
    if !asked.starts_with(date) {
        return Err(Refusal::new(
            Code::SchemaInvalid,
            format!("rename {dir}/{was} {asked}"),
            "an inbox slug starts with its UTC creation date, and a rename keeps it (13, rule 1)",
            format!("pass {date}{} or drop the date", &asked[11..]),
        ));
    }
    Ok(asked.to_string())
}

/// The files under node `old`'s directory, each with where it goes under `new`'s: the markdown ones as ids, the
/// rest as paths under the root.
fn directory(root: &Path, old: &Id, new: &Id) -> Result<Moves, Refusal> {
    let mut moves = Moves::default();
    for path in under(&root.join(old.as_str()))? {
        let rel = path.strip_prefix(root).expect("a walk stays under the root").to_string_lossy().into_owned();
        let to = format!("{new}{}", &rel[old.as_str().len()..]);
        // The reader skips a dot name, so a dot file or one under a dot directory moves as bytes, as an asset does.
        let dot = rel.split('/').any(|s| s.starts_with('.'));
        match (Id::of_path(root, &path), Id::parse(&to)) {
            (Some(id), Ok(to)) if !dot && path.extension().is_some_and(|e| e == "md") => {
                moves.ids.insert(to, id);
            }
            _ => moves.files.push((rel, to)),
        }
    }
    Ok(moves)
}

/// Refuses a rename the node's kind does not allow: an asset, a record, or a new id outside the kind's layout or
/// already held.
fn admit(vault: &Vault, index: &Index, file: &File, new: &Id) -> Result<(), Refusal> {
    let old = &file.id;
    let schema = |subject: &str, rule: &'static str, fix: String| Refusal::new(Code::SchemaInvalid, subject, rule, fix);
    let Some(header) = &file.header else {
        return Err(Refusal::new(
            Code::NoNode,
            old.as_str(),
            "an id names a node or record: a file with a header",
            format!("the file has no header: it is an asset; git -C {} mv it", vault.root.display()),
        ));
    };
    if header.kind.is_record() {
        return Err(schema(
            old.as_str(),
            "a record keeps its stamp: its id is its parent's id and the stamp (13, rule 1)",
            format!("rename its parent node instead: bp rename {} <slug>", header.under.as_ref().unwrap_or(old)),
        ));
    }
    if !header.kind.fits(new) {
        return Err(schema(
            &format!("rename {old} {new}"),
            "a node's id is its kind's directory and a slug; a design's level, a project's name and an inbox date \
             come from its path (12, Layout; 13, rule 1)",
            format!("{new} does not fit kind {}; pass another slug", header.kind),
        ));
    }
    // The node itself may take a past slug back (13, rule 1).
    if let Some((holder, alias)) = index.holder(new)?
        && !(alias && holder == *old)
    {
        return Err(Refusal::new(
            Code::IdTaken,
            new.as_str(),
            "an id names one node or record for good, up to case: a file or another node's alias holds it (13, rule 1)",
            format!("bp show {holder} --no-body; pass another slug"),
        ));
    }
    // A directory left in place would merge with the moved one, and the move would overwrite its files (12, A rename).
    if vault.root.join(new.as_str()).exists() || in_head(&vault.root, new.as_str())? {
        return Err(Refusal::new(
            Code::IdTaken,
            new.as_str(),
            "a renamed node's directory is new: no file or directory holds its path, on disk or in HEAD (12, A rename)",
            format!("the directory {new}/ is taken; move its files out, or pass another slug"),
        ));
    }
    Ok(())
}

/// Whether HEAD holds `path`, a file or a directory. git branches on the exit code alone: 0 names it, 128 does not.
fn in_head(root: &Path, path: &str) -> Result<bool, Refusal> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &format!("HEAD:./{path}")])
        .output()
        .map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
    Ok(out.status.success())
}

/// Every file under `dir`, sorted, dot files too; none when it is not there. A tracked one moves with the rest, so no
/// file keeps the old directory in git; an untracked one refuses the write as a dirty path does.
fn under(dir: &Path) -> Result<Vec<std::path::PathBuf>, Refusal> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| Refusal::failed(dir.display().to_string(), e.to_string()))?.path();
        if path.is_dir() {
            out.extend(under(&path)?);
        } else {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
