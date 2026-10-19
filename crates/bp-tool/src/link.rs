//! `bp link <from> <type> <to> [--remove]`: one typed edge, one changed header line, one commit (15, link).

use std::collections::BTreeMap;
use std::path::Path;

use bp_vault::{Code, Id, Index, Kind, LINKS, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, node_id, schema, set};

/// `bp link <from> <type> <to> [--remove] [--dry-run]`.
#[derive(Args)]
pub struct Link {
    /// The node that holds the link, for example `projects/bp/issues/0412.define-the-per-project-minimum-design`.
    pub from: String,
    /// The link type: cites, answers, targets, authorizes, supersedes, depends.
    #[arg(value_name = "TYPE")]
    pub field: String,
    /// The node or record it names. Under `--remove`, also a link text that is not an id.
    pub to: String,
    /// Remove the edge instead of adding it.
    #[arg(long)]
    pub remove: bool,
    /// Validate and print what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) fn link(cwd: &Path, link: &Link) -> Result<Wrote, Refusal> {
    let from = node_id(&link.from)?;
    let ty = link.field.as_str();
    // A removal names the edge as written, so it also clears a link text that names nothing.
    let to = if link.remove { Id::parse(&link.to).ok() } else { Some(node_id(&link.to)?) };
    let vault = Vault::discover(cwd)?;
    let role = set::role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let mut writer = if link.dry_run { None } else { Some(vault.writer()?) };
    // A human's uncommitted edit refuses first, before its text is read or compared (12, Write path).
    if let Some(w) = &mut writer {
        w.clean([&from])?;
    }
    let index = Index::open(&vault)?;
    let text = vault.text(&from)?;
    let file = bp_vault::File::from_bytes(from.clone(), text.as_bytes())?;
    let Some(header) = &file.header else {
        return Err(Refusal::new(
            Code::NoNode,
            from.as_str(),
            "an id names a node or record: a file with a header",
            format!("the file has no header: it is an asset; edit {}", from.path(&vault.root).display()),
        ));
    };
    let types: Vec<&str> = LINKS.into_iter().filter(|f| schema::link(header.kind, f).is_some()).collect();
    let schema = schema::link(header.kind, ty);
    // A removal needs only a link field: the converted vault holds edges outside today's schema, and they may go.
    if schema.is_none() && !(link.remove && LINKS.contains(&ty)) {
        let fix = if types.is_empty() {
            format!("kind {} holds no link; link from a node that does, or cite it in the body", header.kind)
        } else {
            format!("the link types of kind {} are: {}", header.kind, types.join(", "))
        };
        return Err(Refusal::new(
            Code::SchemaInvalid,
            format!("{from} {ty}"),
            "a node holds only its kind's link types (13, each kind's table)",
            fix,
        ));
    }
    // The field as written, in order, with each id in its plain form: an edit keeps the order and appends.
    let mut values: Vec<String> =
        bp_vault::written(&text, ty).into_iter().map(|t| Id::parse(&t).map_or(t, |id| id.to_string())).collect();
    let target = if link.remove {
        // An alias names the node it points to, as it does when the edge is added.
        let real = match &to {
            Some(id) => index.aliased(id)?,
            None => None,
        };
        let names =
            |v: &String| *v == link.to || [to.as_ref(), real.as_ref()].into_iter().flatten().any(|id| v == id.as_str());
        let Some(at) = values.iter().position(names) else {
            return Ok(Wrote::Already(serde_json::json!({ "id": from })));
        };
        values.remove(at)
    } else {
        let schema = schema.expect("an added edge passed the schema");
        let target = admit(&index, &from, ty, header.kind, &schema, &to.expect("an added edge names an id"))?;
        if values.contains(&target.to_string()) {
            return Ok(Wrote::Already(serde_json::json!({ "id": from })));
        }
        if let (false, Some(held)) = (schema.many, values.first()) {
            return Err(Refusal::new(
                Code::SchemaInvalid,
                format!("{from} {ty} {target}"),
                "a link type that holds one id holds no second (13, each kind's table)",
                format!("bp link {from} {ty} {held} --remove, then link again"),
            ));
        }
        values.push(target.to_string());
        target.to_string()
    };
    let value = Value::from(values);
    // The last edge out takes its line with it: no converted header holds an empty list.
    let stored = if value.as_array().is_some_and(Vec::is_empty) { &Value::Null } else { &value };
    let changes = BTreeMap::from([(from.clone(), bp_vault::set_field(&text, ty, stored))]);
    let mut result =
        serde_json::json!({ "id": from, "type": ty, "to": target, "removed": link.remove, "value": value });
    let Some(writer) = writer else {
        let (_, findings) = index.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let remove = if link.remove { " --remove" } else { "" };
    let written = writer.commit("link", role.as_str(), &format!("link {from} {ty} {target}{remove}"), &changes)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The node an added edge names, once it passes 13: it resolves, through a live alias too; its kind is one the link
/// type allows; and a dependency is another issue of the same project.
fn admit(index: &Index, from: &Id, ty: &str, holder: Kind, schema: &schema::Link, to: &Id) -> Result<Id, Refusal> {
    let Some((target, kind)) = resolve(index, to)? else {
        return Err(Refusal::new(
            Code::UnresolvedLink,
            format!("{from} {ty} {to}"),
            "a write adds no link that resolves to nothing in the proposed tree (12, Write path)",
            format!("point it at an id that exists: bp search {}", search_terms(to)),
        ));
    };
    if !schema.targets.contains(&kind) {
        let kinds: Vec<&str> = schema.targets.iter().map(|k| k.as_str()).collect();
        return Err(Refusal::new(
            Code::SchemaInvalid,
            format!("{from} {ty} {target}"),
            "a link names a node of a kind its type allows (13, each kind's table)",
            format!("`{ty}` of kind {holder} names a {}; {target} is kind {kind}", kinds.join(" or ")),
        ));
    }
    if ty == "depends" && (target == *from || target.project() != from.project()) {
        let fix = if target == *from {
            "an issue cannot wait on itself; name another issue".to_string()
        } else {
            format!("name an issue of project {}", from.project().expect("an issue sits in a project"))
        };
        return Err(Refusal::new(
            Code::SchemaInvalid,
            format!("{from} {ty} {target}"),
            "an issue depends on another issue of its own project, never on itself (13, Issue)",
            fix,
        ));
    }
    Ok(target)
}

/// The node or record `id` names and its kind: the file itself, else the node a live alias of `id` names. `None`
/// when neither exists.
pub(crate) fn resolve(index: &Index, id: &Id) -> Result<Option<(Id, Kind)>, Refusal> {
    let target = if index.header(id.as_str())?.is_some() { Some(id.clone()) } else { index.aliased(id)? };
    let Some(target) = target else { return Ok(None) };
    let Some(header) = index.header(target.as_str())? else { return Ok(None) };
    let kind = header["kind"].as_str().and_then(|k| k.parse().ok()).expect("the index stores closed kinds");
    Ok(Some((target, kind)))
}

/// The words of an id's last segment, for a `bp search` fix.
pub(crate) fn search_terms(id: &Id) -> String {
    id.as_str().rsplit('/').next().expect("rsplit yields at least one piece").replace(['-', '.', '_'], " ")
}
