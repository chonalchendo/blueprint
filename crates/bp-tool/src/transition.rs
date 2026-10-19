//! `bp transition <id> <status>`: one human decision under the kind's machine, one changed line, one commit (15,
//! transition; 25, design change 3). The machines are data in the domain crates; this composes them by kind.

use std::collections::BTreeMap;
use std::path::Path;

use bp_change::{IssueStatus, MilestoneStatus};
use bp_design::{DesignStatus, ProjectStatus};
use bp_reasoning::{ConventionStatus, DecisionStatus, InboxStatus, LearningStatus, ThreadStatus};
use bp_vault::{Code, Index, Kind, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, node_id};

/// `bp transition <id> <status> [--reason <why>] [--dry-run]`.
#[derive(Args)]
pub struct Transition {
    /// The node, for example `projects/bp/milestones/agent-experience`.
    pub id: String,
    /// The status it moves to, from its kind's machine.
    pub status: String,
    /// Why, in one line; it goes in the commit body.
    #[arg(long)]
    pub reason: Option<String>,
    /// Validate and print what would change; write nothing.
    #[arg(long)]
    pub dry_run: bool,
}

/// One kind's machine: its statuses, the moves `transition` makes, and the verb that owns a move it does not.
pub(crate) struct Machine {
    pub statuses: Vec<&'static str>,
    pub moves: Vec<(&'static str, &'static str)>,
    pub owner: fn(&str, &str) -> Option<&'static str>,
}

macro_rules! machine {
    ($t:ty) => {
        machine!($t, |_, _| None)
    };
    ($t:ty, $owner:expr) => {
        Machine {
            statuses: <$t>::ALL.iter().map(|s| s.as_str()).collect(),
            moves: <$t>::MOVES.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect(),
            owner: $owner,
        }
    };
}

/// The machine of a node kind; `None` for a record, which has no status.
pub(crate) fn machine(kind: Kind) -> Option<Machine> {
    Some(match kind {
        Kind::Project => machine!(ProjectStatus),
        Kind::Design => machine!(DesignStatus),
        Kind::Milestone => machine!(MilestoneStatus, |f, t| MilestoneStatus::owner(f.parse().ok()?, t.parse().ok()?)),
        Kind::Issue => machine!(IssueStatus, |f, t| IssueStatus::owner(f.parse().ok()?, t.parse().ok()?)),
        Kind::Thread => machine!(ThreadStatus),
        Kind::Decision => machine!(DecisionStatus),
        Kind::Convention => machine!(ConventionStatus),
        Kind::Learning => machine!(LearningStatus),
        Kind::Inbox => machine!(InboxStatus),
        _ => return None,
    })
}

pub(crate) fn transition(cwd: &Path, t: &Transition) -> Result<Wrote, Refusal> {
    let id = node_id(&t.id)?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let writer = if t.dry_run { None } else { Some(vault.writer()?) };
    let text = vault.text(&id)?;
    let file = bp_vault::File::from_bytes(id.clone(), text.as_bytes())?;
    let illegal = |fix: String| {
        Refusal::new(
            Code::IllegalTransition,
            format!("{id} {}", t.status),
            "transition moves only the edges of the kind's machine; the loop verbs own the rest (25, design change 3)",
            fix,
        )
    };
    let Some((header, m)) = file.header.as_ref().and_then(|h| Some((h, machine(h.kind)?))) else {
        return Err(illegal(format!("{id} has no status: a record is never edited (13, rule 6); bp show {id}")));
    };
    if !m.statuses.contains(&t.status.as_str()) {
        return Err(Refusal::new(
            Code::UnknownValue,
            format!("status {}", t.status),
            "a kind and a status come from closed sets (13)",
            format!("kind {} takes one of: {}", header.kind, m.statuses.join(", ")),
        ));
    }
    let from = header.status.as_deref().unwrap_or_default();
    if from == t.status {
        return Ok(Wrote::Already(serde_json::json!({ "id": id, "status": from })));
    }
    if !m.moves.contains(&(from, t.status.as_str())) {
        return Err(illegal(match (m.owner)(from, &t.status) {
            Some("release") => format!("bp release {id} --reason <escalated|dropped|lost>"),
            Some(verb) => format!("bp {verb} {id}"),
            None => {
                let next: Vec<&str> = m.moves.iter().filter(|(f, _)| *f == from).map(|(_, to)| *to).collect();
                if next.is_empty() {
                    format!("no verb moves kind {} from {from} to {}", header.kind, t.status)
                } else {
                    format!("from {from}, transition moves kind {} to: {}", header.kind, next.join(", "))
                }
            }
        }));
    }
    let mut new = bp_vault::set_field(&text, "status", &t.status.as_str().into());
    if header.kind == Kind::Decision && t.status.parse().is_ok_and(DecisionStatus::decides) {
        new = bp_vault::set_field(&new, "decided_at", &crate::clock::instant(crate::clock::now()?).into());
    }
    let changes = BTreeMap::from([(id.clone(), new)]);
    let mut result = serde_json::json!({ "id": id, "status": t.status, "was": from });
    let Some(writer) = writer else {
        let (_, findings) = Index::open(&vault)?.propose(&vault, &changes)?;
        result["findings"] = serde_json::to_value(findings).expect("a plain struct serialises");
        result["dry_run"] = true.into();
        return Ok(Wrote::Changed(result));
    };
    // Git makes a second paragraph of the subject's blank line, so the reason is the commit body.
    let mut subject = format!("transition {id} {}", t.status);
    if let Some(reason) = &t.reason {
        subject = format!("{subject}\n\n{reason}");
    }
    let written = writer.commit("transition", role.as_str(), &subject, &changes)?;
    result["commit"] = Value::from(written.commit);
    result["findings"] = serde_json::to_value(written.findings).expect("a plain struct serialises");
    Ok(Wrote::Changed(result))
}
