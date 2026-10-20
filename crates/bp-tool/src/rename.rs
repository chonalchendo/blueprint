//! `bp rename <id> <slug>`: a new slug for one node, one commit (15, rename; 12, A rename). The file and its
//! directory move, every link that names the node is rewritten, and the old slug joins `aliases`, so the old id still
//! resolves and no other node takes it (13, rule 1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bp_vault::{Code, File, Id, Index, Kind, Moves, Refusal, SLUG_MAX, Vault, is_slug, retarget};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, create, node_id, set};

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
    let asked = node_id(&rename.id)?;
    let grammar = |slug: &str| {
        let fix = format!("pass a slug in that grammar, for example {}", bp_vault::slug(slug, SLUG_MAX));
        Refusal::new(Code::SchemaInvalid, format!("rename {asked} {slug}"), GRAMMAR, fix)
    };
    if !is_slug(&rename.slug) {
        return Err(grammar(&rename.slug));
    }
    let vault = Vault::discover(cwd)?;
    let role = set::role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let mut writer = if rename.dry_run { None } else { Some(vault.writer()?) };
    let index = Index::open(&vault)?;
    // A past id names the node it moved to, as a link through an alias does.
    let old = match index.aliased(&asked)? {
        Some(real) if !asked.path(&vault.root).is_file() => real,
        _ => asked.clone(),
    };
    let (dir, was) = old.as_str().rsplit_once('/').expect("an id has three segments");
    // An inbox slug starts with its date (13, rule 1); a new slug without one keeps the old date.
    let slug = match was.get(..11) {
        Some(date) if dir.ends_with(Kind::Inbox.dir()) && create::dated(was) && !create::dated(&rename.slug) => {
            format!("{date}{}", rename.slug)
        }
        _ => rename.slug.clone(),
    };
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
    // A human's uncommitted edit to any file the rename touches refuses first, before its text is read (12).
    if let Some(w) = &mut writer {
        let mut paths: Vec<String> = renamed.iter().flat_map(|(a, b)| [format!("{a}.md"), format!("{b}.md")]).collect();
        paths.extend(moves.files.iter().flat_map(|(a, b)| [a.clone(), b.clone()]));
        paths.extend(inbound.iter().map(|id| format!("{id}.md")));
        w.clean_paths(paths)?;
    }
    let text = vault.text(&old)?;
    admit(&vault, &index, &File::from_bytes(old.clone(), text.as_bytes())?, &new)?;
    let mut names = vec![old.clone()];
    names.extend(index.aliases_of(&old)?.into_iter().filter(|a| *a != old));
    let rewrite = |text: &str| names.iter().fold(text.to_string(), |t, n| retarget(&t, n.as_str(), new.as_str()));
    let mut changes = BTreeMap::new();
    let files: BTreeSet<&Id> = renamed.keys().copied().chain(&inbound).collect();
    for id in files {
        let before = if *id == old { text.clone() } else { vault.text(id)? };
        let mut after = rewrite(&before);
        if *id == old {
            // The aliases line holds past ids, not links, so it is written whole, the old slug last.
            let mut aliases = bp_vault::written(&before, "aliases");
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
    let rewritten = changes.keys().filter(|id| !moves.ids.contains_key(*id)).count();
    let mut result = serde_json::json!({
        "id": new, "was": old, "moved": moves.ids.len() + moves.files.len(), "rewritten": rewritten,
    });
    let Some(writer) = writer else {
        let (_, findings) = index.propose_moving(&vault, &changes, &moves.ids)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit_moving("rename", role.as_str(), &format!("rename {old} {new}"), &changes, &moves)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The files under node `old`'s directory, each with where it goes under `new`'s: the markdown ones as ids, the
/// rest as paths under the root.
fn directory(root: &Path, old: &Id, new: &Id) -> Result<Moves, Refusal> {
    let mut moves = Moves::default();
    for path in under(&root.join(old.as_str()))? {
        let rel = path.strip_prefix(root).expect("a walk stays under the root").to_string_lossy().into_owned();
        let to = format!("{new}{}", &rel[old.as_str().len()..]);
        match (Id::of_path(root, &path), Id::parse(&to)) {
            (Some(id), Ok(to)) if path.extension().is_some_and(|e| e == "md") => {
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
    if let Some((holder, _)) = index.holder(new)? {
        return Err(Refusal::new(
            Code::IdTaken,
            new.as_str(),
            "an id names one node or record for good, up to case: a file or a live alias holds it (13, rule 1)",
            format!("bp show {holder} --no-body; pass another slug"),
        ));
    }
    Ok(())
}

/// Every file under `dir`, sorted; none when it is not there. Dot names are skipped, as the reader skips them, so a
/// temporary or system file stays behind.
fn under(dir: &Path) -> Result<Vec<std::path::PathBuf>, Refusal> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| Refusal::failed(dir.display().to_string(), e.to_string()))?.path();
        if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if path.is_dir() {
            out.extend(under(&path)?);
        } else {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
