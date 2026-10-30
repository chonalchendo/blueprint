//! What a pull request carries (27, design change 8; 13, Delta): each proposed delta the issue caused, each diagram
//! such a delta adds, replaces or removes against the link before it (28, design change 7; 34, design change 4), and
//! each named
//! block of the issue, its milestone or such a delta whose `check_hash` changed since the claim. The body names a
//! changed block and the `bp show` commands for its two texts, never the text, so a weakened block reaches the
//! reviewer as a row to read, not as prose to trust (E3). The check set binds those blocks to the approval: the head
//! names the set by a `Bp-Checks` trailer, and a changed set needs a push, which dismisses each approval (15,
//! Controls 1).

use std::collections::BTreeSet;
use std::path::Path;

use bp_read::checkset::{Blocks, CheckSet};
use bp_vault::{Id, Index, Kind, Refusal, Vault};
use serde_json::{Value, json};

use crate::claim::git;

/// The deltas and the block changes, in the order the body lists them, and the check set now and at the claim.
pub(crate) struct Carried {
    issue: Id,
    pub(crate) deltas: Vec<Id>,
    /// Each diagram a delta changes: the delta, the name, `added`, `replaced` or `removed`, and a replaced one's diff
    /// page (28, design change 13).
    diagrams: Vec<(Id, String, &'static str, Option<String>)>,
    changes: Vec<Change>,
    set: CheckSet,
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

/// Reads what `issue` carries: its check set ([`bp_read::checkset`]), each diagram a carried delta changes, and each
/// named block whose hash changed since the claim.
pub(crate) fn read(vault: &Vault, index: &Index, issue: &Id) -> Result<Carried, Refusal> {
    let set = bp_read::checkset::check_set(vault, index, issue)?;
    let (mut deltas, mut diagrams) = (Vec::new(), Vec::new());
    for (design, d) in &set.deltas {
        let delta = Id::parse(&d.id).expect("the index holds ids");
        // A chained delta's diagrams count against the specs the links before it leave (34, design change 4).
        let held = crate::chain::before(vault, index, design, &delta, &d.header)?;
        for (name, spec) in crate::delta::carried(vault, &delta, &d.header)? {
            let page = format!("{delta}/{name}.diff.html");
            let diff = vault.root.join(&page).exists().then_some(page);
            let change = match spec {
                None => "removed",
                Some(_) if held.contains_key(&name) => "replaced",
                Some(_) => "added",
            };
            diagrams.push((delta.clone(), name, change, diff));
        }
        deltas.push(delta);
    }
    let mut changes = Vec::new();
    for o in &set.owners {
        let names: BTreeSet<&String> = o.then.keys().chain(o.now.keys()).collect();
        for name in names {
            let hash = |blocks: &Blocks| blocks.get(name).map(|(h, _)| h.clone());
            let (from, to) = (hash(&o.then), hash(&o.now));
            if from != to {
                // A chained delta's blocks compare with the link before it, as it is now.
                let then = if Kind::Delta.fits(&o.source) {
                    format!("bp show {}", o.source)
                } else {
                    format!("bp show {} --at {}", o.source, set.at)
                };
                let now = format!("bp show {}", o.id);
                changes.push(Change { owner: o.id.clone(), name: name.clone(), from, to, then, now });
            }
        }
    }
    Ok(Carried { issue: issue.clone(), deltas, diagrams, changes, set })
}

impl Carried {
    /// Whether `commit` in `repo` names the check set now ([`CheckSet::named_by`]).
    pub(crate) fn named_by(&self, repo: &Path, commit: &str) -> Result<bool, Refusal> {
        self.set.named_by(repo, commit)
    }

    /// Puts an empty commit that names the set now on `head`, the worktree's branch, and returns it. Its push
    /// dismisses each approval, so the reviewer approves again after reading the new rows. The index and the files
    /// stay as they are.
    pub(crate) fn name_set(&self, worktree: &Path, head: &str) -> Result<String, Refusal> {
        let message = format!("bp: the check set changed\n\nBp-Checks: {}", self.set.set);
        let commit = git(worktree, &["commit-tree", &format!("{head}^{{tree}}"), "-p", head, "-m", &message])?;
        git(worktree, &["update-ref", "HEAD", &commit, head])?;
        Ok(commit)
    }

    /// The result's `diagrams`.
    pub(crate) fn diagrams(&self) -> Value {
        let row = |(d, name, change, diff): &(Id, String, &str, Option<String>)| {
            let mut row = json!({ "delta": d, "name": name, "change": change });
            if let Some(diff) = diff {
                row["diff"] = diff.as_str().into();
            }
            row
        };
        self.diagrams.iter().map(row).collect()
    }

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

    /// Whether the forge's body is this body. A forge may store CRLF line ends or trim trailing whitespace, so both
    /// sides compare with LF ends and no trailing whitespace; else every `pr` rewrites the body and none exits 3.
    pub(crate) fn describes(&self, forge: &str) -> bool {
        let plain = |b: &str| b.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim_end().to_string();
        plain(forge) == plain(&self.body())
    }

    /// The pull request's body.
    pub(crate) fn body(&self) -> String {
        let mut deltas = Vec::new();
        for d in &self.deltas {
            deltas.push(format!("- {d}\n"));
            let drawn = self.diagrams.iter().filter(|(of, ..)| of == d);
            deltas.extend(drawn.map(|(_, name, change, diff)| match diff {
                Some(diff) => format!("  - diagram `{name}`: {change}; diff page `{diff}`\n"),
                None => format!("  - diagram `{name}`: {change}\n"),
            }));
        }
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
