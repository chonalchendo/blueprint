//! What a pull request carries (27, design change 8; 13, Delta): each proposed delta the issue caused, each diagram
//! such a delta adds, replaces or removes (28, design change 7), and each named
//! block of the issue, its milestone or such a delta whose `check_hash` changed since the claim. The body names a
//! changed block and the `bp show` commands for its two texts, never the text, so a weakened block reaches the
//! reviewer as a row to read, not as prose to trust (E3). The check set binds those blocks to the approval: the head
//! names the set by a `Bp-Checks` trailer, and a changed set needs a push, which dismisses each approval (15,
//! Controls 1).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bp_vault::{Body, Code, DeltaState, Filter, Id, Index, Kind, Refusal, Vault};
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
    set: String,
    claimed: String,
}

/// Each named block's hash, and whether it is an error block, by name.
type Blocks = BTreeMap<String, (String, bool)>;

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

/// Reads what `issue` carries. "Since the claim" is the commit that added the issue's earliest claim, under any id
/// the issue had, so neither `release` and `cut` again nor a `rename` moves the baseline past a change. Each owner is
/// read at that commit under the id it had there. A delta's blocks are compared with its design's then, as its body
/// is the proposed design body.
pub(crate) fn read(vault: &Vault, index: &Index, issue: &Id) -> Result<Carried, Refusal> {
    let claims = index.records(issue.as_str(), Kind::Claim)?;
    let first = &claims.first().expect("pr and land act under a claim").id;
    // A claim's path is its issue's id and a suffix a rename keeps. `--follow` is no help: it takes a sibling claim
    // for a copy and follows it.
    let suffix = &first[issue.as_str().len()..];
    let ids = std::iter::once(issue.clone()).chain(index.aliases_of(issue)?);
    let paths: Vec<String> = ids.map(|id| format!("./{id}{suffix}.md")).collect();
    let args = ["log", "--diff-filter=A", "--format=%H", "--"].into_iter().chain(paths.iter().map(String::as_str));
    let at = git(&vault.root, &args.collect::<Vec<_>>())?.lines().last().unwrap_or_default().to_string();
    let file = vault.read(issue)?;
    let project = issue.project().expect("an issue sits under a project");
    let all = Filter { kind: Kind::Design, status: None, project: Some(project), under: None, limit: crate::ALL };
    let (mut deltas, mut diagrams) = (Vec::new(), Vec::new());
    for row in index.list(&all)?.0 {
        let design = Id::parse(&row.id).expect("the index holds ids");
        for d in index.records(design.as_str(), Kind::Delta)? {
            let caused = d.header.get("cause").and_then(Value::as_str) == Some(issue.as_str());
            let body = Body::parse(&d.body);
            if caused && DeltaState::of(&body) == DeltaState::Proposed {
                let delta = Id::parse(&d.id).expect("the index holds ids");
                for (name, spec) in crate::delta::carried(vault, &delta, &d.header)? {
                    let page = format!("{delta}/{name}.diff.html");
                    let diff = vault.root.join(&page).exists().then_some(page);
                    let change = match spec {
                        None => "removed",
                        Some(_) if vault.root.join(format!("{design}/diagrams/{name}.yaml")).exists() => "replaced",
                        Some(_) => "added",
                    };
                    diagrams.push((delta.clone(), name, change, diff));
                }
                deltas.push((delta, design.clone(), body));
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
    let (mut changes, mut set, mut claimed) = (Vec::new(), Vec::new(), Vec::new());
    for (owner, source, now) in owners {
        let (source, then) = then(vault, index, source, &at)?;
        let errors =
            |blocks: &Blocks| blocks.values().filter(|(_, error)| *error).map(|(h, _)| h.clone()).collect::<Vec<_>>();
        set.extend(errors(&now));
        claimed.extend(errors(&then));
        let names: BTreeSet<&String> = then.keys().chain(now.keys()).collect();
        for name in names {
            let hash = |blocks: &Blocks| blocks.get(name).map(|(h, _)| h.clone());
            let (from, to) = (hash(&then), hash(&now));
            if from != to {
                let then = format!("bp show {source} --at {at}");
                let now = format!("bp show {owner}");
                changes.push(Change { owner: owner.clone(), name: name.clone(), from, to, then, now });
            }
        }
    }
    Ok(Carried {
        issue: issue.clone(),
        deltas: deltas.into_iter().map(|(d, ..)| d).collect(),
        diagrams,
        changes,
        set: bp_check::set_hash(set.iter().map(String::as_str)),
        claimed: bp_check::set_hash(claimed.iter().map(String::as_str)),
    })
}

/// The blocks of `id` at commit `at`, under the id it had there: today's, or a past one its aliases keep. A file the
/// commit lacks had no blocks then. One today's schema cannot parse counts as none too: its blocks read as added,
/// which the reviewer sees, where a refusal would stop every later `pr`.
fn then(vault: &Vault, index: &Index, id: Id, at: &str) -> Result<(Id, Blocks), Refusal> {
    for past in std::iter::once(id.clone()).chain(index.aliases_of(&id)?) {
        match vault.read_at(&past, at) {
            Ok(f) => return Ok((past, hashes(&f.body))),
            Err(r) if r.code == Code::SchemaInvalid => return Ok((past, BTreeMap::new())),
            Err(r) if r.code == Code::NoNode => {}
            Err(r) => return Err(r),
        }
    }
    Ok((id, BTreeMap::new()))
}

/// The named blocks of `body`.
fn hashes(body: &Body) -> Blocks {
    body.named_checks().into_iter().map(|c| (c.attrs["name"].clone(), (c.hash(), c.is_error()))).collect()
}

impl Carried {
    /// Whether `commit` in `repo` names the check set now: its `Bp-Checks` trailer, else the set at the claim, which
    /// holds until a `pr` pushes a trailer.
    pub(crate) fn named_by(&self, repo: &Path, commit: &str) -> Result<bool, Refusal> {
        let named = git(repo, &["log", "-1", "--format=%(trailers:key=Bp-Checks,valueonly)", commit])?;
        Ok(named.lines().last().map_or(self.claimed.as_str(), str::trim) == self.set)
    }

    /// Puts an empty commit that names the set now on `head`, the worktree's branch, and returns it. Its push
    /// dismisses each approval, so the reviewer approves again after reading the new rows. The index and the files
    /// stay as they are.
    pub(crate) fn name_set(&self, worktree: &Path, head: &str) -> Result<String, Refusal> {
        let message = format!("bp: the check set changed\n\nBp-Checks: {}", self.set);
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
