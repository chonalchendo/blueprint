//! What a pull request carries (27, design change 8; 13, Delta): each proposed delta the issue caused, and each named
//! block of the issue, its milestone or such a delta whose `check_hash` changed since the claim. The body names a
//! changed block and the `bp show` commands for its two texts, never the text, so a weakened block reaches the
//! reviewer as a row to read, not as prose to trust (E3).

use std::collections::{BTreeMap, BTreeSet};

use bp_vault::{Body, DeltaState, Filter, Id, Index, Kind, Refusal, Vault};
use serde_json::{Value, json};

use crate::claim::git;

/// The deltas and the block changes, in the order the body lists them.
pub(crate) struct Carried {
    issue: Id,
    pub(crate) deltas: Vec<Id>,
    changes: Vec<Change>,
}

/// One named block whose hash differs from its hash at the claim's commit.
struct Change {
    owner: Id,
    name: String,
    from: Option<String>,
    to: Option<String>,
    /// The commands that print the text at the claim and now.
    then: String,
    now: String,
}

/// Reads what `issue` carries under `claim`. A delta's blocks are compared with its design's at the claim, as its
/// body is the proposed design body.
pub(crate) fn read(vault: &Vault, index: &Index, issue: &Id, claim: &str) -> Result<Carried, Refusal> {
    let at = git(&vault.root, &["log", "--diff-filter=A", "--format=%H", "-1", "--", &format!("./{claim}.md")])?;
    let file = vault.read(issue)?;
    let project = issue.project().expect("an issue sits under a project");
    let all = Filter { kind: Kind::Design, status: None, project: Some(project), under: None, limit: usize::MAX >> 1 };
    let mut deltas = Vec::new();
    for row in index.list(&all)?.0 {
        let design = Id::parse(&row.id).expect("the index holds ids");
        for d in index.records(design.as_str(), Kind::Delta)? {
            let caused = d.header.get("cause").and_then(Value::as_str) == Some(issue.as_str());
            let body = Body::parse(&d.body);
            if caused && DeltaState::of(&body) == DeltaState::Proposed {
                deltas.push((Id::parse(&d.id).expect("the index holds ids"), design.clone(), body));
            }
        }
    }
    // Each owner: its id, the file its claim-time text comes from, and its blocks now.
    let mut owners = vec![(issue.clone(), issue.clone(), hashes(&file.body))];
    if let Some(m) = file.header.and_then(|h| h.under) {
        let blocks = hashes(&vault.read(&m)?.body);
        owners.push((m.clone(), m, blocks));
    }
    owners.extend(deltas.iter().map(|(d, design, body)| (d.clone(), design.clone(), hashes(body))));
    let mut changes = Vec::new();
    for (owner, source, now) in owners {
        // A file the claim's commit lacks had no blocks then. One today's schema cannot parse counts as none too:
        // its blocks read as added, which the reviewer sees, where a refusal would stop every later `pr`.
        let then = vault.read_at(&source, &at).map(|f| hashes(&f.body)).unwrap_or_default();
        let names: BTreeSet<&String> = then.keys().chain(now.keys()).collect();
        for name in names {
            let (from, to) = (then.get(name).cloned(), now.get(name).cloned());
            if from != to {
                let then = format!("bp show {source} --at {at}");
                let now = format!("bp show {owner}");
                changes.push(Change { owner: owner.clone(), name: name.clone(), from, to, then, now });
            }
        }
    }
    Ok(Carried { issue: issue.clone(), deltas: deltas.into_iter().map(|(d, ..)| d).collect(), changes })
}

/// Each named block's hash, by name.
fn hashes(body: &Body) -> BTreeMap<String, String> {
    bp_check::named(body).into_iter().map(|c| (c.attrs["name"].clone(), bp_check::hash(c))).collect()
}

impl Carried {
    /// The result's `check_changes`.
    pub(crate) fn changes(&self) -> Value {
        self.changes
            .iter()
            .map(|c| {
                json!({
                    "owner": c.owner, "name": c.name, "change": c.kind(), "from": c.from, "to": c.to,
                    "show": [c.then, c.now],
                })
            })
            .collect()
    }

    /// The pull request's body.
    pub(crate) fn body(&self) -> String {
        let deltas: Vec<String> = self.deltas.iter().map(|d| format!("- {d}\n")).collect();
        let deltas = if deltas.is_empty() { "None.\n".into() } else { deltas.concat() };
        let hash = |h: &Option<String>| h.as_deref().map_or("-".into(), |h| format!("`{h}`"));
        let rows: Vec<String> = self
            .changes
            .iter()
            .map(|c| {
                let (from, to, kind) = (hash(&c.from), hash(&c.to), c.kind());
                format!("| {} | {} | {kind} | {from} | {to} | `{}`; `{}` |\n", c.owner, c.name, c.then, c.now)
            })
            .collect();
        let changes = if rows.is_empty() {
            "None.\n".into()
        } else {
            format!(
                "| Owner | Block | Change | From | To | Read the texts |\n|---|---|---|---|---|---|\n{}",
                rows.concat()
            )
        };
        format!("Issue: {}\n\n## Deltas\n\n{deltas}\n## Check changes since the claim\n\n{changes}", self.issue)
    }
}

impl Change {
    fn kind(&self) -> &'static str {
        match (&self.from, &self.to) {
            (None, _) => "added",
            (_, None) => "removed",
            _ => "changed",
        }
    }
}
