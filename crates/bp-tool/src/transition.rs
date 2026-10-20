//! `bp transition <id> <status>`: one human decision under the kind's machine, one changed line, one commit (15,
//! transition; 25, design change 3). The machines are data in the domain crates; this composes them by kind.

use std::collections::BTreeMap;
use std::path::Path;

use bp_change::{IssueStatus, MilestoneStatus};
use bp_design::{DesignStatus, ProjectStatus};
use bp_reasoning::{ConventionStatus, DecisionStatus, InboxStatus, LearningStatus, ThreadStatus};
use bp_vault::{Code, Kind, Refusal, Vault};
use clap::Args;
use serde_json::Value;

use crate::{Wrote, node_id, subject};

/// `bp transition <id> <status> [--reason <why>] [--dry-run]`.
#[derive(Args)]
pub struct Transition {
    /// The node, for example `projects/bp/milestones/agent-experience`.
    pub id: String,
    /// The status it moves to, from its kind's machine.
    pub status: String,
    /// Why, in one line; it goes in the commit body. A newline refuses, so no line of it reads as a trailer.
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
    let role = match t.status.as_str() {
        "approved" | "active" => crate::set::human(format!("transition {} {}", t.id, t.status))?,
        _ => crate::set::role()?,
    };
    node_id(&t.id)?;
    if t.reason.as_deref().is_some_and(|r| r.contains(['\n', '\r'])) {
        return Err(Refusal::usage(
            "--reason",
            "give the reason in one line; a line of its own could read as a trailer",
        ));
    }
    let vault = Vault::discover(cwd)?;
    // The lock comes before the read, so no other write lands between what this one reads and what it commits.
    let writer = if t.dry_run { None } else { Some(vault.writer()?) };
    let mut opened = None;
    let index = crate::index(&vault, writer.as_ref(), &mut opened)?;
    let id = subject(index, &t.id)?;
    // A human's uncommitted edit refuses first, dry run or not, before its text is read (12, Write path).
    match &writer {
        Some(w) => w.clean([&id])?,
        None => vault.clean([&id])?,
    }
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
    let Some(header) = &file.header else {
        return Err(Refusal::new(
            Code::NoNode,
            id.as_str(),
            "an id names a node or record: a file with a header",
            format!("the file has no header: an asset has no status; edit {}", id.path(&vault.root).display()),
        ));
    };
    let Some(m) = machine(header.kind) else {
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
        let fix = match route(&m, from, &t.status) {
            Some(steps) => {
                let steps: Vec<String> = steps
                    .iter()
                    .map(|(verb, to)| match *verb {
                        "transition" => format!("bp transition {id} {to}"),
                        "release" => format!("bp release {id} --reason <escalated|dropped|lost>"),
                        verb => format!("bp {verb} {id}"),
                    })
                    .collect();
                steps.join(", then ")
            }
            None => format!("no verb moves kind {} from {from} to {}", header.kind, t.status),
        };
        return Err(illegal(fix));
    }
    let mut new = bp_vault::set_field(&text, "status", &t.status.as_str().into());
    if header.kind == Kind::Decision && t.status.parse().is_ok_and(DecisionStatus::decides) {
        new = bp_vault::set_field(&new, "decided_at", &crate::clock::instant(crate::clock::now()?).into());
    }
    let changes = BTreeMap::from([(id.clone(), new)]);
    let mut result = serde_json::json!({ "id": id, "status": t.status, "was": from });
    let Some(writer) = writer else {
        let (_, findings) = index.propose(&vault, &changes)?;
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

/// The shortest run of moves from `from` to `to`, each by `transition` or its owning verb, so each step runs from the
/// status the step before it leaves: `abandoned → resolved` starts with `transition open`.
fn route<'a>(m: &Machine, from: &'a str, to: &str) -> Option<Vec<(&'static str, &'a str)>> {
    let step = |a: &str, b: &str| if m.moves.contains(&(a, b)) { Some("transition") } else { (m.owner)(a, b) };
    let mut seen = vec![from];
    let mut frontier = vec![(from, Vec::new())];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (at, path) in frontier {
            for &s in &m.statuses {
                let Some(verb) = step(at, s).filter(|_| !seen.contains(&s)) else { continue };
                let mut path: Vec<(&'static str, &'a str)> = path.clone();
                path.push((verb, s));
                if s == to {
                    return Some(path);
                }
                seen.push(s);
                next.push((s, path));
            }
        }
        frontier = next;
    }
    None
}
