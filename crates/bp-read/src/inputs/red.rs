//! A project's master-red records as `land`, `bp clear`, the intents and the view read them (37, design change 11).
//! A red denies, so it is read from the vault's history: each record any commit added under `projects/<p>/reds`, as
//! that commit wrote it, so a `git rm` or an edit ends nothing. An end record, which `land` writes when git shows the
//! red reverted, sits beside them and names the red it ends. A record no clear could sign is no red. The intents end
//! a red on an end record, or on a clear whose signature is a valid signature of this red's clear; `land` judges the
//! revert from git, verifies the clear against the forge's signer and holds only a red the forge confirms.

use bp_change::intents::{self, Red};
use bp_vault::{Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde_json::{Map, Value};

use super::text;
use crate::signed::{Act, armour, is_branch, is_sha, signs};

/// Each master-red and end record of `project` ever added, in commit order.
pub struct Reds {
    /// The reds, oldest first.
    pub reds: Vec<Red>,
    /// The shas an end record names.
    pub ended: Vec<String>,
}

/// A master-red record, reduced; `None` when it names no full sha, which no `land` writes, or when it is an end record.
pub fn red_of(r: &Record) -> Option<Red> {
    if r.header.contains_key("reverted") {
        return None;
    }
    let sha = full(&r.header, "red")?;
    let checks = r.header.get("checks").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
    Some(Red {
        id: r.id.clone(),
        sha,
        pull_request: r.header.get("pull_request").and_then(Value::as_u64).unwrap_or_default(),
        base: text(&r.header, "base").unwrap_or_default(),
        checks: checks.map(str::to_string).collect(),
        issue: text(&r.header, "issue").unwrap_or_default(),
        revert: r.header.get("revert").and_then(Value::as_bool).unwrap_or_default(),
        revert_issue: None,
    })
}

/// The value of `key` when it is a full sha: 40 or more hex digits, so no git call reads a flag from it.
fn full(header: &Map<String, Value>, key: &str) -> Option<String> {
    text(header, key).filter(|s| s.len() >= 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Every master-red and end record of `project` that any commit of the vault added, as that commit wrote it. A vault
/// outside git reads the records as they stand.
pub fn reds(vault: &Vault, index: &Index, project: &str) -> Result<Reds, Refusal> {
    let records = if bp_vault::git::Repo::find(&vault.root).is_some() {
        let dir = format!("./projects/{project}/{}", Kind::MasterRed.dir());
        let args = ["log", "--reverse", "--diff-filter=A", "--no-renames", "--relative", "--format=commit: %H"];
        let log = crate::git::git(&vault.root, &[&args[..], &["--name-only", "--", &dir]].concat())?;
        let mut out = Vec::new();
        let mut commit = "";
        for line in log.lines() {
            if let Some(c) = line.strip_prefix("commit: ") {
                commit = c;
            } else if let Some(Ok(id)) = line.strip_suffix(".md").map(Id::parse) {
                // A file today's schema cannot read is no record `land` wrote.
                if let Some(header) = vault.read_at(&id, commit).ok().and_then(|f| f.header_json()) {
                    out.push(Record { id: id.to_string(), header, body: String::new() });
                }
            }
        }
        out
    } else {
        index.records(&format!("projects/{project}/project"), Kind::MasterRed)?
    };
    let ended = records.iter().filter(|r| r.header.contains_key("reverted")).filter_map(|r| full(&r.header, "red"));
    let mut reds = Vec::new();
    for red in records.iter().filter_map(red_of) {
        if signable(&red)? {
            reds.push(red);
        }
    }
    Ok(Reds { ended: ended.collect(), reds })
}

/// Whether each field of `red` that a clear signs is one line of the message: a full sha, a name git takes for a
/// branch, one line per check. `land` writes no other record, and a red that no clear could sign is no red: no gate,
/// no intent and no clear reads it (review of #102, W1).
fn signable(red: &Red) -> Result<bool, Refusal> {
    let lines = red.checks.iter().all(|n| !n.chars().any(char::is_control));
    Ok(is_sha(&red.sha) && lines && is_branch(&red.base)?)
}

/// The message a clear of `red` signs: the act `clear`, the project node, the pull request whose merge made the red,
/// its base branch and the red commit as the head of that branch, then the failed checks it clears and the base
/// branch's head when the human signed. A reader rebuilds it from the master-red record and the clear's `tip`, so a
/// new failed check at the sha needs a new clear.
pub fn clear_message(node: &Id, red: &Red, tip: &str) -> String {
    let checks = red.checks.join(", ");
    let more = [("checks", checks.as_str()), ("tip", tip)];
    Act { verb: "clear", node, pull_request: red.pull_request, base: &red.base, head: &red.sha, more: &more }.message()
}

/// The red of `project` that the intents and the view read as holding: the earliest that no end record names and no
/// clear with a valid signature of its clear ends, with the live issue whose label `reverts` names it. Who signed the
/// clear is `land`'s test: the intents read no signer and no forge (22, constraint 8).
///
/// # Panics
///
/// Never: a project's node is an id.
pub fn red(vault: &Vault, index: &Index, project: &str) -> Result<Option<Red>, Refusal> {
    let Reds { reds, ended } = reds(vault, index, project)?;
    if reds.is_empty() {
        return Ok(None);
    }
    let node = Id::parse(&format!("projects/{project}/project")).expect("a project names its node");
    let clears = index.records(node.as_str(), Kind::Clear)?;
    let signed = |r: &Red| {
        clears.iter().filter(|c| text(&c.header, "red").as_deref() == Some(r.sha.as_str())).any(|c| {
            let tip = text(&c.header, "tip").unwrap_or_default();
            armour(&c.body).is_some_and(|sig| signs(&clear_message(&node, r, &tip), &sig))
        })
    };
    let Some(red) = intents::red_in_force(&reds, |r| ended.contains(&r.sha) || signed(r)) else { return Ok(None) };
    let f = Filter { kind: Kind::Issue, status: None, project: Some(project), under: None, limit: crate::ALL };
    let mut revert_issue = None;
    for row in index.list(&f)?.0 {
        let header = index.header(&row.id)?.unwrap_or_default();
        if intents::is_live_issue(&text(&header, "status").unwrap_or_default())
            && reverts(&header).as_deref() == Some(red.sha.as_str())
        {
            revert_issue = Some(row.id);
            break;
        }
    }
    Ok(Some(Red { revert_issue, ..red.clone() }))
}

/// The red commit an issue's label `reverts` names: the session marks the revert issue with it, so the intents route
/// that issue to its build and its land while every other issue reads `master-red`.
pub fn reverts(header: &Map<String, Value>) -> Option<String> {
    full(header, "reverts")
}

/// The revert the intents and the view read for an issue: the red its label names, when that red holds and a revert
/// of it may skip review.
pub fn revert_of(header: &Map<String, Value>, red: Option<&Red>) -> Option<String> {
    red.filter(|r| !r.revert).map(|r| r.sha.clone()).filter(|sha| reverts(header).as_ref() == Some(sha))
}

#[cfg(test)]
mod tests {
    use bp_vault::Record;
    use serde_json::json;

    use super::{red_of, revert_of};

    #[allow(clippy::needless_pass_by_value)]
    fn record(header: serde_json::Value) -> Record {
        Record { id: "projects/bp/reds/r".into(), header: header.as_object().unwrap().clone(), body: String::new() }
    }

    /// A red names a full sha, so no git call reads a flag from it; an end record is no red (37, design change 11).
    #[test]
    fn a_red_names_a_full_sha_and_an_end_record_is_none() {
        let sha = "a".repeat(40);
        assert_eq!(red_of(&record(json!({ "red": sha, "pull_request": 7 }))).map(|r| r.pull_request), Some(7));
        for bad in [json!({ "red": "a".repeat(39) }), json!({ "red": format!("-{}", "a".repeat(40)) }), json!({})] {
            assert!(red_of(&record(bad.clone())).is_none(), "{bad}");
        }
        assert!(red_of(&record(json!({ "red": sha, "reverted": "b".repeat(40) }))).is_none(), "an end record");
        // The revert issue's label names the red; a red that is itself a revert has no revert issue that skips review.
        let red = red_of(&record(json!({ "red": sha }))).unwrap();
        let label = json!({ "reverts": sha });
        assert_eq!(revert_of(label.as_object().unwrap(), Some(&red)), Some(sha.clone()));
        let looped = bp_change::intents::Red { revert: true, ..red.clone() };
        assert_eq!(revert_of(label.as_object().unwrap(), Some(&looped)), None);
        assert_eq!(revert_of(json!({ "reverts": "b".repeat(40) }).as_object().unwrap(), Some(&red)), None);
    }
}
