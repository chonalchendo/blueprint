//! What `land` reads of a red master (37, design change 11), before every other gate: the checks at the base branch's
//! head, the forge's merge of that commit and the land record that names it, the project's master-red records from
//! the vault's history, the human's clears and the revert on the branch. The intents never call the forge (22,
//! constraint 8), and `land` already fetches, so `land` alone reads the checks at the base.

use std::collections::BTreeMap;

use bp_change::intents::{self, Candidate, Red};
use bp_forge::{Gh, PullRequest};
use bp_read::inputs::Reds;
use bp_read::landing;
use bp_vault::{Code, Filter, Id, Index, Kind, Refusal, Vault};
use serde_json::Value;

use super::{Claimed, merged};
use crate::claim::git;

/// What the red read found: no red, the pull request is the exact revert of `Revert`'s red, or a refusal.
enum Read {
    Green,
    Revert(String),
    Refused(Box<Refusal>),
}

/// `land`'s first gate: the red commit that `pr` reverts exactly, `None` with no red, or `master_red`; or
/// `base_pending` while a check still runs at a commit `land` merged. The records the read found missing are
/// committed first, the dry run aside: a new red, and the end of a red that git shows reverted, so the intents read
/// both with no forge call. On a merged pull request the refusal's fix is the human's review of the merge, as for
/// every gate after a merge.
pub(super) fn gate(
    c: &Claimed,
    gh: &Gh,
    vault: &Vault,
    index: &Index,
    id: &Id,
    pr: &PullRequest,
    dry_run: bool,
) -> Result<Option<String>, Refusal> {
    let (read, writes) = read(c, gh, vault, index, id, pr)?;
    if !writes.is_empty() && !dry_run {
        let what = format!("master-red {}", id.project().unwrap_or_default());
        let role = crate::set::role()?;
        vault.writer()?.commit("land", role.as_str(), &what, &writes)?;
    }
    match read {
        Read::Green => Ok(None),
        Read::Revert(sha) => Ok(Some(sha)),
        Read::Refused(mut refusal) => {
            if merged(pr) && refusal.code == Code::MasterRed {
                refusal.fix = landing::after_merge(pr.number, id);
            }
            Err(*refusal)
        }
    }
}

/// The lines a land record adds to the claim's release: the pull request, the merge commit, which a red at the base
/// reads ([`landed`]), and the red that a revert ends; and for a revert, the red's end record, so the intents end it
/// too. A merge the forge names no commit of refuses `could_not_run`, so no land record leaves a red unread; the rerun
/// finishes the land.
pub(super) fn lines(
    vault: &Vault,
    id: &Id,
    pr: &PullRequest,
    revert: Option<&str>,
) -> Result<(String, Option<(Id, String)>), Refusal> {
    let Some(merge) = &pr.merge_commit else {
        return Err(Refusal::failed(
            format!("gh pr view {}", pr.number),
            format!("the forge names no merge commit of pull request {}; rerun bp land when it does", pr.number),
        ));
    };
    let mut more = format!("pull_request: {}\nmerged: {}\n", pr.number, merge.oid);
    more.extend(revert.map(|r| format!("reverts: {r}\n")));
    let ended = revert.map(|r| end(vault, id.project().unwrap_or_default(), r, &merge.oid)).transpose()?;
    Ok((more, ended))
}

/// The end record of the red `sha`, which the revert at `at` ended: `land` writes it when it lands the revert, and
/// when it finds the red reverted on the branch by any other hand, so the intents end the red too.
pub(super) fn end(vault: &Vault, project: &str, sha: &str, at: &str) -> Result<(Id, String), Refusal> {
    let node = Id::parse(&format!("projects/{project}/project")).expect("a project names its node");
    let record = crate::record::free(&vault.root, &format!("projects/{project}/{}/{sha}", Kind::MasterRed.dir()));
    let now = crate::clock::instant(crate::clock::now()?);
    let fields: Vec<(&str, Value)> = vec![("red", sha.into()), ("reverted", at.into()), ("at", now.into())];
    Ok((record, crate::stamp::record(Kind::MasterRed, &node, &[], fields)))
}

/// The red read of `land` for the pull request `pr` of `id`, in the clone of `c`, and the records it found missing. A
/// failed check at the head of the base branch, on a commit that the forge merged as a pull request a land record of
/// the project names, is a red; so is a master-red record that no exact revert on the branch and no valid clear by
/// the human has ended. With a red, only its exact revert lands ([`intents::landable`]); a merged pull request whose
/// merge commit is that revert finishes as one.
fn read(
    c: &Claimed,
    gh: &Gh,
    vault: &Vault,
    index: &Index,
    id: &Id,
    pr: &PullRequest,
) -> Result<(Read, BTreeMap<Id, String>), Refusal> {
    let project = id.project().unwrap_or_default();
    let node = Id::parse(&format!("projects/{project}/project")).expect("a project names its node");
    let mut writes = BTreeMap::new();
    let Some(tip) = crate::risk::at_origin(&c.top, &c.target)? else { return Ok((Read::Green, writes)) };
    let Reds { mut reds, ended } = bp_read::inputs::reds(vault, index, project)?;
    let checks = gh.checks(&tip)?;
    let record =
        if checks.failed.is_empty() && checks.pending.is_empty() { None } else { landed(index, gh, project, &tip)? };
    if record.is_some() && checks.failed.is_empty() {
        return Ok((Read::Refused(Box::new(pending(id, &c.target, &tip, &checks.pending))), writes));
    }
    // A red record names the checks it saw fail; a new failed check at the same sha is a new red, which a new clear
    // must sign.
    if let Some(l) = &record
        && !reds.iter().any(|r| r.sha == tip && checks.failed.iter().all(|f| r.checks.contains(f)))
    {
        let path = format!("projects/{project}/{}/{tip}", Kind::MasterRed.dir());
        let red = Red {
            id: crate::record::free(&vault.root, &path).to_string(),
            sha: tip.clone(),
            pull_request: l.pull_request,
            base: c.target.clone(),
            checks: checks.failed.clone(),
            issue: l.issue.clone(),
            revert: l.revert,
            revert_issue: None,
        };
        writes.insert(Id::parse(&red.id).expect("free names an id"), text(&node, &red)?);
        reds.push(red);
    }
    // The signer is read only when a clear names a red, so a land with no clear reads no key.
    let mut key: Option<String> = None;
    let (mut open, mut ends) = (None, Vec::new());
    for r in &reds {
        if let Some(at) = crate::clear::reverted(&c.top, r, &tip) {
            // A merged pull request whose merge commit is the revert finishes its land as the revert.
            if merged(pr) && pr.merge_commit.as_ref().is_some_and(|m| m.oid == at) {
                return Ok((Read::Revert(r.sha.clone()), writes));
            }
            // Git shows the red reverted by another hand: the end record ends it for the intents too.
            if !ended.contains(&r.sha) && !ends.contains(&r.sha) {
                let (record, text) = end(vault, project, &r.sha, &at)?;
                writes.insert(record, text);
                ends.push(r.sha.clone());
            }
            continue;
        }
        let named = index.records(node.as_str(), Kind::Clear)?;
        if named.iter().any(|x| x.header.get("red").and_then(Value::as_str) == Some(r.sha.as_str())) {
            if key.is_none() {
                key = Some(super::level::forge_signer(c, gh)?);
            }
            if crate::clear::cleared(index, &node, r, key.as_deref().unwrap_or_default())? {
                continue;
            }
        }
        open = Some(r);
        break;
    }
    let Some(red) = open else { return Ok((Read::Green, writes)) };
    // The land record of the tip says whether the red is itself a revert; a hand-written record cannot lower it.
    let red = Red { revert: red.revert || record.as_ref().is_some_and(|l| l.revert), ..red.clone() };
    let head = pr.head_ref_oid.as_str();
    let tree = |r: &str| git(&c.top, &["rev-parse", "--verify", "-q", &format!("{r}^{{tree}}")]).unwrap_or_default();
    let descends = git(&c.top, &["merge-base", "--is-ancestor", &red.sha, head]).is_ok();
    let (tree, before) = (tree(head), tree(&format!("{}^1", red.sha)));
    let base = pr.base_ref_oid.as_deref().unwrap_or_default();
    let failing = record.is_some();
    let candidate = Candidate { tip: &tip, base, descends, tree: &tree, before: &before, failing };
    Ok((
        match intents::landable(Some(&red), &candidate) {
            Ok(_) => Read::Revert(red.sha.clone()),
            Err(red) => Read::Refused(Box::new(refusal(id, red, pr, &candidate))),
        },
        writes,
    ))
}

/// The master-red record of `red` under the project `node`, as `land` writes it.
fn text(node: &Id, red: &Red) -> Result<String, Refusal> {
    let at = crate::clock::instant(crate::clock::now()?);
    let fields: Vec<(&str, Value)> = vec![
        ("red", red.sha.clone().into()),
        ("pull_request", red.pull_request.into()),
        ("base", red.base.clone().into()),
        ("checks", red.checks.clone().into()),
        ("issue", red.issue.clone().into()),
        ("revert", red.revert.into()),
        ("at", at.into()),
    ];
    Ok(crate::stamp::record(Kind::MasterRed, node, &[], fields))
}

/// `base_pending`: a check still runs at the base branch's head, a commit `land` merged. A red found later would
/// fall on the next merge, and its revert would undo the wrong one, so `land` waits. No red record exists, so `bp
/// clear` does not apply: a check that never finishes ends only on the forge (review of #102, V6).
fn pending(id: &Id, base: &str, tip: &str, names: &[String]) -> Refusal {
    Refusal::new(
        Code::BasePending,
        id.as_str(),
        "land merges onto a base branch only when the checks of the last land there are done (37, design change 11)",
        format!(
            "{} still run at {base}'s head {tip}; wait for them, then rerun bp land {id}. A check that never finishes \
             ends only on the forge: the human reruns or cancels it at {tip}. bp clear does not apply, as no red is \
             recorded",
            names.join(", ")
        ),
    )
}

/// `master_red`, with one fix for each case. When no revert can skip its review (the base branch moved past the red,
/// the forge shows no failed check there now, or the red is itself a revert), only the human's clear ends it. Else the
/// revert issue's exact revert lands first.
fn refusal(id: &Id, red: &Red, pr: &PullRequest, c: &Candidate) -> Refusal {
    let project = id.project().unwrap_or_default();
    let checks = if red.checks.is_empty() { "a check".to_string() } else { red.checks.join(", ") };
    let at = format!("{} is red at {}: {checks} failed after pull request {}", red.base, red.sha, red.pull_request);
    let clear = format!("the human runs bp clear {project} from a shell");
    let fix = if intents::revertible(red, c) {
        let why = if c.base != red.sha || !c.descends {
            format!("it is not based on {}", red.sha)
        } else {
            format!("its tree is not the tree of {}^", red.sha)
        };
        format!(
            "{at}, and pull request {} is no exact revert of it ({why}); the revert issue lands first: dispatch a \
             worker to make its pull request one based on {} with the tree of its first parent, then bp land that \
             issue; or {clear}",
            pr.number, red.sha
        )
    } else {
        let why = if c.tip != red.sha {
            format!("{} moved past it to {}", red.base, c.tip)
        } else if red.revert {
            "it is itself the land of a revert".to_string()
        } else {
            format!("the forge shows no failed check at {} now", red.sha)
        };
        format!("{at}; no revert of it lands with no review ({why}), so only its clear ends it: {clear}")
    };
    landing::master_red(red, fix)
}

/// The land record of `project` that merged `sha`: a claim released `landed` whose `merged` line names `sha`, when the
/// forge reads the pull request that the claim names `MERGED`, from the claim's branch, with `sha` as its merge
/// commit. A line of text alone, in a body, a review or a hand-written claim, names no merge the forge made. `None`
/// when no such record exists: a merge made outside `land` is no red of the project.
fn landed(index: &Index, gh: &Gh, project: &str, sha: &str) -> Result<Option<Landed>, Refusal> {
    let f = Filter { kind: Kind::Claim, status: None, project: Some(project), under: None, limit: bp_read::ALL };
    let mut issues: Vec<String> = index.list(&f)?.0.into_iter().filter_map(|row| row.under).collect();
    issues.sort();
    issues.dedup();
    for issue in issues {
        let Ok(issue_id) = Id::parse(&issue) else { continue };
        for claim in index.records(&issue, Kind::Claim)? {
            let release = intents::release(&claim.body);
            let Some(n) = release.get("pull_request").and_then(|n| n.parse().ok()) else { continue };
            if release.get("reason").map(String::as_str) != Some("landed")
                || release.get("merged").map(String::as_str) != Some(sha)
            {
                continue;
            }
            let p = gh.pr(n)?;
            if merged(&p)
                && p.merge_commit.as_ref().is_some_and(|m| m.oid == sha)
                && p.head_ref_name == landing::branch(&issue_id)
            {
                return Ok(Some(Landed { pull_request: n, issue, revert: release.contains_key("reverts") }));
            }
        }
    }
    Ok(None)
}

/// A land of the project that the forge confirms merged a commit.
struct Landed {
    pull_request: u64,
    issue: String,
    /// The land merged a revert: no revert of its commit skips review.
    revert: bool,
}
