//! The stack (15, a dependency chain to landed): `cut` puts a dependant on its dependency's open pull request head,
//! `land` reads the base a layer lands on and carries an approval across a rebase by patch id, and `land` of the lower
//! layer rebases, pushes and retargets each layer above before it deletes its branch.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bp_change::IssueStatus;
use bp_change::intents::{self, Base};
use bp_forge::{Gh, PullRequest};
use bp_vault::{Code, Id, Index, Kind, Record, Refusal, Vault};
use serde_json::Value;

use crate::claim::{branch, field, git, held};

/// A stack is at most three layers deep; the cap is a choice, not a measurement (15, a dependency chain to landed).
pub(crate) const STACK_DEPTH: usize = 3;

/// The dependency `cut` stacks on, and its open pull request: the unresolved `depends` target whose stack holds every
/// other one. `None` when each target is resolved. A target with no open pull request refuses `dependency_unpushed`,
/// and a layer past [`STACK_DEPTH`] refuses `stack_too_deep` (15, a dependency chain to landed). `--base` cannot
/// name another start then.
pub(crate) fn below(
    top: &Path,
    index: &Index,
    id: &Id,
    base: Option<&str>,
) -> Result<Option<(Id, PullRequest)>, Refusal> {
    let resolved = |d: &Id| -> Result<bool, Refusal> {
        let status = index.header(d.as_str())?.and_then(|h| h.get("status")?.as_str().map(str::to_string));
        Ok(status.as_deref() == Some(IssueStatus::Resolved.as_str()))
    };
    let header = index.header(id.as_str())?.unwrap_or_default();
    let mut below = Vec::new();
    let depends = header.get("depends").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
    for d in depends.filter_map(|d| Id::parse(d).ok()) {
        if !resolved(&d)? {
            below.push(d);
        }
    }
    if below.is_empty() {
        return Ok(None);
    }
    let rule = "a dependant is cut from the head of its unresolved dependency's open pull request (15, a dependency \
                chain to landed)";
    let gh = Gh::new(top);
    let mut stacks = Vec::new();
    for d in &below {
        let Some(pr) = gh.prs_for_head(&branch(d))?.into_iter().find(PullRequest::is_open) else {
            return Err(Refusal::new(
                Code::DependencyUnpushed,
                d.as_str(),
                rule,
                format!("{id} depends on {d}, which has no open pull request: bp pr {d}, then rerun"),
            ));
        };
        // The layers under the new one, `d` first: each held claim's unresolved `base_issue`, one past the cap at most.
        let mut layers = vec![d.clone()];
        while layers.len() <= STACK_DEPTH {
            let next = held(index, &layers[layers.len() - 1])?.and_then(|h| field(&h, "base_issue"));
            match next.and_then(|n| Id::parse(&n).ok()) {
                Some(n) if !resolved(&n)? => layers.push(n),
                _ => break,
            }
        }
        stacks.push((pr, layers));
    }
    let Some((pr, mut layers)) = stacks.into_iter().find(|(_, layers)| below.iter().all(|d| layers.contains(d))) else {
        let names: Vec<&str> = below.iter().map(Id::as_str).collect();
        return Err(Refusal::new(
            Code::DependencyUnpushed,
            id.as_str(),
            rule,
            format!("{id} depends on {}, which no one stack holds: land all but one, then rerun", names.join(", ")),
        ));
    };
    if layers.len() >= STACK_DEPTH {
        let names: Vec<&str> = layers.iter().map(Id::as_str).collect();
        return Err(Refusal::new(
            Code::StackTooDeep,
            id.as_str(),
            "a stack is at most three layers deep (15, a dependency chain to landed)",
            format!("{id} would sit on {}; bp land {} first, then rerun", names.join(" on "), names[names.len() - 1]),
        ));
    }
    let below = layers.swap_remove(0);
    if let Some(b) = base {
        return Err(Refusal::usage(
            format!("--base {b}"),
            format!("{id} is cut from the head of {below}'s pull request; drop --base, then rerun"),
        ));
    }
    Ok(Some((below, pr)))
}

/// An approval at `reviewed` carries to the pull request's head (15, E7): the patch id of the diff the reviewer saw,
/// against the claim's base `from`, which stands for the base at review time, equals that of the head against the
/// base now, read from the clone at `top`. An empty diff or a commit the clone lacks carries nothing.
pub(crate) fn same_diff(top: &Path, from: &str, reviewed: &str, p: &PullRequest) -> bool {
    let Some(base) = &p.base_ref_name else { return false };
    let id = |range: String| -> Option<String> {
        let diff = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["diff", "--no-ext-diff", "--no-color", "--end-of-options", &range])
            .output()
            .ok()?;
        if !diff.status.success() || diff.stdout.is_empty() {
            return None;
        }
        let mut child = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["patch-id", "--stable"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(&diff.stdout).ok()?;
        let out = child.wait_with_output().ok()?;
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string)
    };
    let reviewed = id(format!("{from}...{reviewed}"));
    reviewed.is_some() && reviewed == id(format!("refs/remotes/origin/{base}...{}", p.head_ref_oid))
}

/// The branch a held `claim`'s pull request lands on (15, a dependency chain to landed): its `base_branch`; or, when
/// it was cut from a dependency's head, `Unmerged` until the dependency resolves, then the branch the dependency's
/// landed claim landed on, followed down the stack.
pub(crate) fn base(index: &Index, claim: &Record) -> Result<Base, Refusal> {
    let (mut below, mut branch) = (field(claim, "base_issue"), field(claim, "base_branch"));
    // A stack is at most `STACK_DEPTH` layers, so the walk ends within as many steps.
    for _ in 0..STACK_DEPTH {
        let Some(issue) = below.take() else { break };
        let status = index.header(&issue)?.and_then(|h| h.get("status")?.as_str().map(str::to_string));
        if status.as_deref() != Some(IssueStatus::Resolved.as_str()) {
            return Ok(Base::Unmerged(issue));
        }
        let landed = index
            .records(&issue, Kind::Claim)?
            .into_iter()
            .rev()
            .find(|c| intents::release(&c.body).get("reason").map(String::as_str) == Some("landed"));
        // A dependency resolved by hand landed nowhere this tool knows.
        let Some(landed) = landed else { return Ok(Base::Unknown) };
        (below, branch) = (field(&landed, "base_issue"), field(&landed, "base_branch"));
    }
    Ok(branch.map_or(Base::Unknown, Base::Branch))
}

/// A layer cut from the head of the issue `land` merges, which `land` moves onto the base before it deletes the
/// branch below.
pub(crate) struct Child {
    pub(crate) id: Id,
    pub(crate) worktree: PathBuf,
    branch: String,
    /// The claim's `base`, the head below it was cut from: its own commits are those past it.
    from: String,
}

/// The layers cut from `below`'s head: each issue whose held claim names it as `base_issue`. The rebase runs in each
/// one's worktree, so it must be there and clean; `land` asks before it merges, so a refusal moves nothing.
pub(crate) fn children(index: &Index, below: &Id) -> Result<Vec<Child>, Refusal> {
    let mut out = Vec::new();
    for n in index.inbound(below)? {
        let Some(h) = held(index, &n)? else { continue };
        if field(&h, "base_issue").as_deref() != Some(below.as_str()) {
            continue;
        }
        let worktree = PathBuf::from(field(&h, "worktree").unwrap_or_default());
        if !worktree.is_dir() {
            return Err(Refusal::failed(
                worktree.display().to_string(),
                format!("land {below} rebases {n} in its worktree, which is gone; bp cut {n} puts it back, then rerun"),
            ));
        }
        if !git(&worktree, &["status", "--porcelain"])?.is_empty() {
            return Err(Refusal::new(
                Code::PathDirty,
                worktree.display().to_string(),
                "land rebases each layer above in its worktree, so it holds no uncommitted work (15, a dependency \
                 chain to landed)",
                format!("commit and bp pr {n}, or stash, the changes in {}, then rerun", worktree.display()),
            ));
        }
        out.push(Child { branch: branch(&n), from: field(&h, "base").unwrap_or_default(), id: n, worktree });
    }
    Ok(out)
}

/// What `land` did to the layers above: those moved onto the base, with their blocks' runs at the new head, and those
/// stranded on the branch below by a conflict.
pub(crate) struct Moved {
    pub(crate) restacked: Vec<Id>,
    pub(crate) stranded: Vec<Id>,
    pub(crate) ran: Vec<bp_check::Ran>,
}

/// Moves each child onto `target` once the layer below has merged, then runs its blocks at its new head with no lock
/// held, as they may take minutes (15, a dependency chain to landed).
pub(crate) fn restack(
    gh: &Gh,
    vault: &Vault,
    children: &[Child],
    target: &str,
    clock: &dyn Fn() -> String,
) -> Result<Moved, Refusal> {
    let mut moved = Moved { restacked: Vec::new(), stranded: Vec::new(), ran: Vec::new() };
    for child in children {
        if move_onto(gh, child, target)? {
            moved.ran.extend(bp_check::run(vault, &child.id, &child.worktree, None, &[], clock)?);
            moved.restacked.push(child.id.clone());
        } else {
            moved.stranded.push(child.id.clone());
        }
    }
    Ok(moved)
}

/// Moves `child` onto `target`: rebases its own commits, those past its claim's base, onto origin's `target`, pushes
/// with a lease on the forge's head, and retargets its pull request. The claim's base, not the merged head, bounds
/// them: a child cut before the layer below moved, or a grandchild whose parent was rebased, holds its own commits
/// past it alone. Each step skips what is done, so a land that stopped part way finishes on its rerun. `false` when
/// the rebase conflicts: it is undone, and the child stays on the branch below, which `land` keeps, so its pull
/// request stays open.
fn move_onto(gh: &Gh, child: &Child, target: &str) -> Result<bool, Refusal> {
    let (w, onto) = (&child.worktree, format!("refs/remotes/origin/{target}"));
    if git(w, &["merge-base", "--is-ancestor", &onto, "HEAD"]).is_err()
        && git(w, &["rebase", "-q", "--onto", &onto, &child.from]).is_err()
    {
        git(w, &["rebase", "--abort"])?;
        return Ok(false);
    }
    let head = git(w, &["rev-parse", "HEAD"])?;
    if let Some(pr) = gh.prs_for_head(&child.branch)?.into_iter().find(PullRequest::is_open) {
        if pr.head_ref_oid != head {
            let lease = format!("--force-with-lease=refs/heads/{}:{}", child.branch, pr.head_ref_oid);
            git(w, &["push", "-q", "origin", &lease, &format!("{head}:refs/heads/{}", child.branch)])?;
        }
        // Before the branch below goes: a pull request whose base is deleted closes for good (15, E7).
        if pr.base_ref_name.as_deref() != Some(target) {
            gh.retarget(pr.number, target)?;
        }
    }
    Ok(true)
}
