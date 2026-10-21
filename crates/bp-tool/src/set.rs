//! `bp set <id> <field> <value>`: one header label, one changed line, one commit (15, set). `bp set <id> --body-file
//! <f>`: the body replaced, the header byte for byte (25, design change 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_design::DesignStatus;
use bp_operation::Role;
use bp_vault::{Code, Header, Id, Kind, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::create::{body_file, stored};
use crate::{Wrote, node_id, schema, subject};

/// `bp set <id> <field> <value> [--dry-run]` or `bp set <id> --body-file <f> [--dry-run]`.
#[derive(Args)]
pub struct Set {
    /// The node, for example `projects/bp/issues/0412.define-the-per-project-minimum-design`.
    pub id: String,
    /// The label, for example `severity`, `tags`, `title`.
    #[arg(required_unless_present = "body_file", conflicts_with = "body_file")]
    pub field: Option<String>,
    /// The value; a list label takes comma-separated values.
    #[arg(required_unless_present = "body_file", conflicts_with = "body_file")]
    pub value: Option<String>,
    /// The new body, read from this file, instead of a label. The header stays byte for byte; a design past `draft`
    /// changes through `bp record delta`.
    #[arg(long)]
    pub body_file: Option<PathBuf>,
    /// Validate and print what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

pub(crate) fn set(cwd: &Path, set: &Set) -> Result<Wrote, Refusal> {
    let body = set.body_file.as_deref().map(body_file).transpose()?;
    node_id(&set.id)?;
    let vault = Vault::discover(cwd)?;
    let role = role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let writer = if set.dry_run { None } else { Some(vault.writer()?) };
    let mut opened = None;
    let index = crate::index(&vault, writer.as_ref(), &mut opened)?;
    let id = subject(index, &set.id)?;
    // A human's uncommitted edit refuses first, dry run or not, before its text is read (12, Write path).
    match &writer {
        Some(w) => w.clean([&id])?,
        None => vault.clean([&id])?,
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
    let (new, mut result, field) = if let Some(body) = &body {
        let Some(new) = with_body(&id, header, &text, body)? else {
            return Ok(Wrote::Already(serde_json::json!({ "id": id })));
        };
        (new, serde_json::json!({ "id": id, "field": "body" }), "body")
    } else {
        let (Some(field), Some(value)) = (&set.field, &set.value) else {
            unreachable!("clap requires a field and a value without --body-file")
        };
        let value = schema::label(header.kind, &id, field, value)?;
        let was = match field.as_str() {
            "title" => header.title.clone().map(Value::from),
            f => header.fields.get(f).cloned(),
        };
        if was.as_ref() == Some(&value) {
            return Ok(Wrote::Already(serde_json::json!({ "id": id })));
        }
        let new = bp_vault::set_field(&text, field, &value);
        (new, serde_json::json!({ "id": id, "field": field, "value": value, "was": was }), field.as_str())
    };
    let changes = BTreeMap::from([(id.clone(), new)]);
    let Some(writer) = writer else {
        let (_, findings) = index.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        // The change a dry run shows: a label's value and was, or the body as it would be stored.
        if let Some(body) = &body {
            result["body"] = stored(body).into();
        }
        return Ok(Wrote::Changed(result));
    };
    let written = writer.commit("set", role.as_str(), &format!("set {id} {field}"), &changes)?;
    result["commit"] = written.commit.into();
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}

/// The file with its body replaced: the header's bytes as they were, then the body as `create` stores it. `None`
/// when the stored body is already that one. A record is append-only, and a design past `draft` changes only through
/// an accepted delta (13, Design history).
fn with_body(id: &Id, header: &Header, text: &str, body: &str) -> Result<Option<String>, Refusal> {
    if let Some(fix) = schema::owner(header.kind, id, "body") {
        return Err(Refusal::new(
            Code::NotALabel,
            format!("{id} body"),
            "set writes a node's label or body; a record is append-only (13, rule 6)",
            fix,
        ));
    }
    if header.kind == Kind::Design && header.status.as_deref() != Some(DesignStatus::Draft.as_str()) {
        return Err(Refusal::new(
            Code::DesignNotDelta,
            id.as_str(),
            "a design past draft changes only through a delta record; its file is rewritten when the delta is \
             accepted (13, Design history)",
            format!(
                "bp record delta {id} --field milestone=<milestone> --body-file <f>; or --editorial when only wording and layout change"
            ),
        ));
    }
    Ok(rebody(text, body))
}

/// `text`, a file with a header, with its body replaced: the header's bytes as they were, then the body as `create`
/// stores it. `None` when the stored body is already that one. `stamp` writes an accepted delta's body this way.
pub(crate) fn rebody(text: &str, body: &str) -> Option<String> {
    let (head, old) = text.split_at(bp_vault::header_end(text).expect("the file parsed with a header"));
    let body = stored(body);
    if stored(old) == body {
        return None;
    }
    // A header closed at end of file gains the line end the body needs; every header byte stays.
    let eol = if head.ends_with('\n') { "" } else { "\n" };
    Some(if body.is_empty() { format!("{head}{eol}") } else { format!("{head}{eol}\n{body}\n") })
}

/// The role of a human act (`stamp`, `land`, a `transition` to `approved` or `active`), tested before any other: an agent
/// role refuses `role_forbidden` (27, design change 3). An agent can forge the label, so this stops a slip, not an
/// attack; the forge stays the gate. So a milestone's approval is its latest such commit with `Bp-Role: human` (13).
pub(crate) fn human(act: String) -> Result<Role, Refusal> {
    let role = role()?;
    if role == Role::Human {
        return Ok(role);
    }
    let fix = format!("{act} is the human's act; ask the human to run `bp {act}` from a shell with BP_ROLE unset");
    Err(Refusal::new(
        Code::RoleForbidden,
        act,
        "BP_ROLE names an agent role, and the verb is the human's: stamp, land, or transition to approved or active",
        fix,
    ))
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
