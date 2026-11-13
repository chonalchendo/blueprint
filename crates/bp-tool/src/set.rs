//! `bp set <id> <field> <value>`: one header label, one changed line, one commit (15, set). `bp set <id> --body-file
//! <f>`: the body replaced, the header byte for byte (25, design change 2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bp_change::MilestoneStatus;
use bp_change::risk::Level;
use bp_design::DesignStatus;
use bp_operation::Role;
use bp_vault::{Code, Header, Id, Index, Kind, Refusal, Vault};
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
    // A dry run takes no writer, so it runs the writer's commit precondition itself (34, design change 7).
    let writer = if set.dry_run { vault.committed().map(|()| None)? } else { Some(vault.writer()?) };
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
        lowers(header.kind, &id, field, was.as_ref(), &value, || lowerable(&vault, index, header.under.as_ref(), &id))?;
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

/// `not_a_label` when the write lowers an issue's `risk` or clears its `complex`, and [`lowerable`] says no: both are
/// set at derive and approved with the milestone, so before the approval a level is a label; after it, only the
/// human's signed record lowers a level (37, design changes 1 and 2). Setting `complex` is always allowed, as a raise
/// of `risk` is; clearing it moves the worker from Fable to Sonnet (36, section 3). An issue with no `risk` label
/// reads medium, so a first `low` is a lowering too, and so is a `low` at `create` (`create::create`). `lowerable`
/// runs only for a lowering: it reads git.
pub(crate) fn lowers(
    kind: Kind,
    id: &Id,
    field: &str,
    was: Option<&Value>,
    value: &Value,
    lowerable: impl FnOnce() -> Result<bool, Refusal>,
) -> Result<(), Refusal> {
    let rank = |v: Option<&Value>| v.and_then(Value::as_str).and_then(|l| l.parse::<Level>().ok());
    let refuse = |rule: &'static str, fix: String| Refusal::new(Code::NotALabel, format!("{id} {field}"), rule, fix);
    if kind != Kind::Issue {
        return Ok(());
    }
    if field == "complex" && was.and_then(Value::as_bool) == Some(true) && value.as_bool() == Some(false) {
        let fix = format!("the human clears complex in the header of {id} from a shell and commits it");
        if lowerable()? {
            return Ok(());
        }
        return Err(refuse(
            "a label clears complex only while the issue's milestone is planned and was never approved, and the issue \
             holds no claim; else only the human does (36, section 3)",
            fix,
        ));
    }
    match (rank(was).or(Some(Level::Medium)), rank(Some(value))) {
        (Some(was), Some(now)) if field == "risk" && now < was && !lowerable()? => {
            let fix = format!("the human runs bp lower {id} <level> --reason <why> from a shell");
            Err(refuse(
                "a label lowers a risk level only while the issue's milestone is planned and was never approved, and \
                 the issue holds no claim; else only a signed human record lowers it (37, design changes 1 and 2)",
                fix,
            ))
        }
        _ => Ok(()),
    }
}

/// Whether a label may still lower the risk of `issue` under the milestone `under`: the milestone is `planned`, no
/// commit of its file ever held `approved` or `done` as its status, and the issue holds no claim record (37, design
/// change 1). So `approved -> abandoned -> planned` does not reopen the label (review of #98, row 29). A history git
/// cannot read refuses.
pub(crate) fn lowerable(vault: &Vault, index: &Index, under: Option<&Id>, issue: &Id) -> Result<bool, Refusal> {
    let Some(m) = under else { return Ok(false) };
    if bp_read::inputs::status(index, m.as_str())?.as_deref() != Some(MilestoneStatus::Planned.as_str())
        || !index.records(issue.as_str(), Kind::Claim)?.is_empty()
    {
        return Ok(false);
    }
    let path = format!("./{m}.md");
    let pattern = "^status: *[\"']?(approved|done)";
    // `--text`: a `-diff` attribute makes git skip a binary file under `-G` and exit 0, which would read as "never
    // approved" (review of #98, row 35).
    let args = ["log", "--follow", "--text", "--format=%H", "-G", pattern, "--", &path];
    Ok(bp_read::git::git(&vault.root, &args)?.is_empty())
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

/// The role of a human act (`stamp`, `approve`, `lower`, a `transition` to `approved` or `active`), tested before any
/// other: an agent role refuses `role_forbidden` (27, design change 3). An agent can forge the label, so this stops a
/// slip, not an attack; the forge stays the gate. So a milestone's approval is its latest such commit with `Bp-Role:
/// human` (13).
pub(crate) fn human(act: String) -> Result<Role, Refusal> {
    let role = role()?;
    if role == Role::Human {
        return Ok(role);
    }
    let fix = format!("{act} is the human's act; ask the human to run `bp {act}` from a shell with BP_ROLE unset");
    Err(Refusal::new(
        Code::RoleForbidden,
        act,
        "BP_ROLE names an agent role, and the verb is the human's: stamp, approve, lower, or transition to approved or \
         active",
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
