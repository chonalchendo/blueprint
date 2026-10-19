//! `bp set <id> <field> <value>`: one header label, one changed line, one commit (15, set).

use std::collections::BTreeMap;
use std::path::Path;

use bp_operation::Role;
use bp_vault::{Code, Index, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, node_id, schema};

/// `bp set <id> <field> <value> [--dry-run]`.
#[derive(Args)]
pub struct Set {
    /// The node, for example `projects/bp/issues/0412.define-the-per-project-minimum-design`.
    pub id: String,
    /// The label, for example `severity`, `tags`, `title`.
    pub field: String,
    /// The value; a list label takes comma-separated values.
    pub value: String,
    /// Validate and print what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) fn set(cwd: &Path, set: &Set) -> Result<Wrote, Refusal> {
    let id = node_id(&set.id)?;
    let vault = Vault::discover(cwd)?;
    let role = role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let mut writer = if set.dry_run { None } else { Some(vault.writer()?) };
    // A human's uncommitted edit refuses first, before its text is read (12, Write path).
    if let Some(w) = &mut writer {
        w.clean([&id])?;
    }
    let text = vault.text(&id)?;
    let file = bp_vault::File::from_bytes(id.clone(), text.as_bytes())?;
    let Some(header) = &file.header else {
        return Err(Refusal::new(
            Code::NoNode,
            id.as_str(),
            "an id names a node or record: a file with a header",
            format!("the file has no header: it is an asset; edit {}", id.path(&vault.root).display()),
        ));
    };
    let value = schema::label(header.kind, &id, &set.field, &set.value)?;
    let was = match set.field.as_str() {
        "title" => header.title.clone().map(Value::from),
        f => header.fields.get(f).cloned(),
    };
    if was.as_ref() == Some(&value) {
        return Ok(Wrote::Already(serde_json::json!({ "id": id })));
    }
    let changes = BTreeMap::from([(id.clone(), bp_vault::set_field(&text, &set.field, &value))]);
    let mut result =
        serde_json::json!({ "id": id, "field": set.field, "value": value, "was": was.unwrap_or(Value::Null) });
    let Some(writer) = writer else {
        let (_, findings) = Index::open(&vault)?.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit("set", role.as_str(), &format!("set {id} {}", set.field), &changes)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The `Bp-Role` of this process's commits: `BP_ROLE`, `human` when unset or empty (25, design change 7). A value
/// outside the set is an environment value that does not parse: `usage`, as for `BP_NOW`.
pub(crate) fn role() -> Result<Role, Refusal> {
    let text = std::env::var_os("BP_ROLE").unwrap_or_default();
    let text = text.to_string_lossy();
    if text.is_empty() {
        return Ok(Role::Human);
    }
    text.parse().map_err(|_| {
        let roles: Vec<&str> = Role::ALL.iter().map(|r| r.as_str()).collect();
        Refusal::usage(format!("BP_ROLE={text}"), format!("unset BP_ROLE, or set it to one of: {}", roles.join(", ")))
    })
}
