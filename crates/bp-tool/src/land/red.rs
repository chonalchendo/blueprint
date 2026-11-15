//! What `land` reads of a red master (37, design change 11), before every other gate: the check runs at the base
//! branch's head, the land record of that commit, the project's master-red records, the human's clears and the revert
//! on the branch. The intents never call the forge (22, constraint 8), and `land` already fetches, so `land` alone
//! reads the checks at the base.

use std::collections::BTreeMap;

use bp_change::intents::{self, Candidate, Red};
use bp_forge::{Gh, PullRequest};
use bp_read::landing;
use bp_vault::{Code, Id, Index, Kind, Refusal, Vault};
use serde_json::Value;

use super::{Claimed, merged};
use crate::claim::git;

/// What the red read found: no red, the pull request is the exact revert of `Revert`'s red, or a refusal with the
/// master-red record `land` writes first when the red is new.
enum Read {
    Green,
    Revert(String),
    Red(Box<Refusal>, Option<(Id, String)>),
}

/// `land`'s first gate: the red commit that `pr` reverts exactly, `None` with no red, or `master_red`. A new red is
/// recorded first, the dry run aside, so the intents read it with no forge call. On a merged pull request the
/// refusal's fix is the human's review of the merge, as for every gate after a merge.
pub(super) fn gate(
    c: &Claimed,
    gh: &Gh,
    vault: &Vault,
    index: &Index,
    id: &Id,
    pr: &PullRequest,
    dry_run: bool,
) -> Result<Option<String>, Refusal> {
    match read(c, gh, vault, index, id, pr)? {
        Read::Green => Ok(None),
        Read::Revert(sha) => Ok(Some(sha)),
        Read::Red(mut refusal, write) => {
            if let Some((record, text)) = write.filter(|_| !dry_run) {
                let what = format!("master-red {}", id.project().unwrap_or_default());
                let role = crate::set::role()?;
                vault.writer()?.commit("land", role.as_str(), &what, &BTreeMap::from([(record, text)]))?;
            }
            if merged(pr) {
                refusal.fix = landing::after_merge(pr.number, id);
            }
            Err(*refusal)
        }
    }
}

/// The lines a land record adds to the claim's release: the pull request, the merge commit, which a red at the base
/// reads ([`landed`]), and the red that a revert ends.
pub(super) fn lines(pr: &PullRequest, revert: Option<&str>) -> String {
    let mut more = format!("pull_request: {}\n", pr.number);
    more.extend(pr.merge_commit.as_ref().map(|m| format!("merged: {}\n", m.oid)));
    more.extend(revert.map(|r| format!("reverts: {r}\n")));
    more
}

/// The red read of `land` for the pull request `pr` of `id`, in the clone of `c`. A failed check run at the head of
/// the base branch, on a commit that a land record of the project merged, is a red; so is a master-red record that no
/// exact revert on the branch and no valid clear by the human has ended. With a red, only its exact revert lands
/// ([`intents::landable`]); a merged pull request whose merge commit is that revert finishes as one.
fn read(c: &Claimed, gh: &Gh, vault: &Vault, index: &Index, id: &Id, pr: &PullRequest) -> Result<Read, Refusal> {
    let project = id.project().unwrap_or_default();
    let node = Id::parse(&format!("projects/{project}/project")).expect("a project names its node");
    let Some(tip) = crate::risk::at_origin(&c.top, &c.target)? else { return Ok(Read::Green) };
    let mut reds = bp_read::inputs::reds(index, project)?;
    let failed = gh.failed_checks(&tip)?;
    let mut write = None;
    let merged_by = if failed.is_empty() { None } else { landed(vault, project, &tip)? };
    if let Some(n) = merged_by
        && !reds.iter().any(|r| r.sha == tip)
    {
        let record = crate::record::free(&vault.root, &format!("projects/{project}/{}/{tip}", Kind::MasterRed.dir()));
        let red =
            Red { id: record.to_string(), sha: tip.clone(), pull_request: n, base: c.target.clone(), checks: failed };
        write = Some((record, text(&node, &red)?));
        reds.push(red);
    }
    // The signer is read only when a clear names a red, so a land with no clear reads no key.
    let mut key: Option<String> = None;
    let mut open = None;
    for r in &reds {
        if let Some(at) = crate::clear::reverted(&c.top, r, &tip) {
            // A merged pull request whose merge commit is the revert finishes its land as the revert.
            if merged(pr) && pr.merge_commit.as_ref().is_some_and(|m| m.oid == at) {
                return Ok(Read::Revert(r.sha.clone()));
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
    let Some(red) = open else { return Ok(Read::Green) };
    let head = pr.head_ref_oid.as_str();
    let tree = |r: &str| git(&c.top, &["rev-parse", "--verify", "-q", &format!("{r}^{{tree}}")]).unwrap_or_default();
    let descends = git(&c.top, &["merge-base", "--is-ancestor", &red.sha, head]).is_ok();
    let (tree, before) = (tree(head), tree(&format!("{}^1", red.sha)));
    let base = pr.base_ref_oid.as_deref().unwrap_or_default();
    let failing = merged_by.is_some();
    let candidate = Candidate { tip: &tip, base, descends, tree: &tree, before: &before, failing };
    match intents::landable(Some(red), &candidate) {
        Ok(_) => Ok(Read::Revert(red.sha.clone())),
        Err(red) => Ok(Read::Red(Box::new(refusal(id, red, pr, &candidate)), write)),
    }
}

/// The master-red record of `red` under the project `node`, as `land` writes it.
fn text(node: &Id, red: &Red) -> Result<String, Refusal> {
    let at = crate::clock::instant(crate::clock::now()?);
    let fields: Vec<(&str, Value)> = vec![
        ("red", red.sha.clone().into()),
        ("pull_request", red.pull_request.into()),
        ("base", red.base.clone().into()),
        ("checks", red.checks.clone().into()),
        ("at", at.into()),
    ];
    Ok(crate::stamp::record(Kind::MasterRed, node, &[], fields))
}

/// `master_red`: the pull request is not the exact revert of the red, with the step that is missing.
fn refusal(id: &Id, red: &Red, pr: &PullRequest, c: &Candidate) -> Refusal {
    let project = id.project().unwrap_or_default();
    let why = if c.tip != red.sha {
        format!("{} moved past the red to {}", red.base, c.tip)
    } else if !c.failing {
        format!("the forge shows no failed check at {} now, so only the human's clear ends it", red.sha)
    } else if c.base != red.sha || !c.descends {
        format!("it is not based on {}", red.sha)
    } else {
        format!("its tree is not the tree of {}^", red.sha)
    };
    let checks = if red.checks.is_empty() { "a check".to_string() } else { red.checks.join(", ") };
    Refusal::new(
        Code::MasterRed,
        red.id.as_str(),
        "while the base branch is red after a land, land merges only the exact revert of the red commit (37, design \
         change 11)",
        format!(
            "{} is red at {}: {checks} failed after pull request {}, and pull request {} is no exact revert ({why}); \
             dispatch a worker to revert it on its own issue, a pull request based on {} with the tree of its first \
             parent, then bp land that issue; or the human runs bp clear {project} from a shell",
            red.base, red.sha, red.pull_request, pr.number, red.sha
        ),
    )
}

/// The pull request number of the land record of `project` that merged `sha`: a claim's release line `merged: <sha>`,
/// as the commit that first held it wrote it, so a later edit or removal of the record does not hide it. `None` when
/// no commit of the project's issues ever held the line: a merge made outside `land` is no red of the project.
fn landed(vault: &Vault, project: &str, sha: &str) -> Result<Option<u64>, Refusal> {
    let line = format!("merged: {sha}");
    let dir = format!("./projects/{project}/issues");
    let log = git(&vault.root, &["log", "--text", "--format=%H", "-S", &line, "--", &dir])?;
    let Some(commit) = log.lines().last() else { return Ok(None) };
    let names = git(&vault.root, &["show", "--relative", "--format=", "--name-only", commit, "--", &dir])?;
    for path in names.lines().filter(|p| p.contains("/claims/")) {
        let Ok(text) = git(&vault.root, &["show", &format!("{commit}:./{path}")]) else { continue };
        let release: BTreeMap<String, String> = intents::release(&text);
        if release.get("merged").map(String::as_str) == Some(sha) {
            return Ok(Some(release.get("pull_request").and_then(|n| n.parse().ok()).unwrap_or_default()));
        }
    }
    Ok(Some(0))
}
