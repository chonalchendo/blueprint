//! The stack (15, a dependency chain to landed): `cut` puts a dependant on its dependency's open pull request head,
//! `land` reads the base a layer lands on (`bp_read::inputs::base`), and `land` of the lower layer rebases, pushes
//! and retargets each layer above before it deletes its branch. Each moved layer needs `pr` and a new approval, as
//! the forge requires an approval of the latest push (15, Controls 1).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bp_change::IssueStatus;
use bp_change::intents::{self, Dependency, Unstacked};
use bp_forge::{Gh, PullRequest};
use bp_read::inputs::{field, held, layers, status};
use bp_vault::{Code, Id, Index, Refusal};
use serde_json::Value;

use crate::claim::{branch, git};

/// The dependency `cut` stacks on, and its open pull request, by the stack test ready shares ([`intents::stack`]);
/// `None` when each `depends` target is resolved. `--base` cannot name another start then.
pub(crate) fn below(
    top: &Path,
    index: &Index,
    id: &Id,
    base: Option<&str>,
) -> Result<Option<(Id, PullRequest)>, Refusal> {
    let header = index.header(id.as_str())?.unwrap_or_default();
    let gh = Gh::new(top);
    let (mut depends, mut prs) = (Vec::new(), Vec::new());
    for d in header.get("depends").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let status = status(index, d)?;
        let pr = match Id::parse(d) {
            Ok(d) if status.as_deref() != Some(IssueStatus::Resolved.as_str()) => {
                gh.prs_for_head(&branch(&d))?.into_iter().find(PullRequest::is_open)
            }
            _ => None,
        };
        let layers = if pr.is_some() { layers(index, d)? } else { Vec::new() };
        depends.push(Dependency { id: d.to_string(), status, open_pr: pr.is_some(), layers });
        prs.push(pr);
    }
    let rule = "a dependant is cut from the head of its unresolved dependency's open pull request (15, a dependency \
                chain to landed)";
    let on = match intents::stack(&depends) {
        Ok(None) => return Ok(None),
        Ok(Some(on)) => on,
        Err(Unstacked::Unpushed(d)) => {
            return Err(Refusal::new(
                Code::DependencyUnpushed,
                d[0],
                rule,
                format!("{id} depends on {}, which has no open pull request: bp pr {}, then rerun", d[0], d[0]),
            ));
        }
        Err(Unstacked::Split(d)) => {
            return Err(Refusal::new(
                Code::DependencySplit,
                id.as_str(),
                "an issue's unresolved dependencies sit in one stack (15, a dependency chain to landed)",
                format!("{id} depends on {}, which no one stack holds: land all but one, then rerun", d.join(", ")),
            ));
        }
        Err(Unstacked::Deep(layers)) => {
            return Err(Refusal::new(
                Code::StackTooDeep,
                id.as_str(),
                "a stack is at most three layers deep (15, a dependency chain to landed)",
                format!(
                    "{id} would sit on {}; bp land {} first, then rerun",
                    layers.join(" on "),
                    layers[layers.len() - 1]
                ),
            ));
        }
    };
    if let Some(b) = base {
        return Err(Refusal::usage(
            format!("--base {b}"),
            format!("{id} is cut from the head of {}'s pull request; drop --base, then rerun", on.id),
        ));
    }
    let at = depends.iter().position(|d| std::ptr::eq(d, on)).expect("the stack test returns one of them");
    let pr = prs.swap_remove(at).expect("a dependency on the stack holds an open pull request");
    Ok(Some((Id::parse(&on.id).expect("only a parsed id holds a pull request"), pr)))
}

/// Whether the diff the reviewer approved at `reviewed`, against the claim's base `from`, is the head's against the
/// base now, by `git patch-id --verbatim`, whitespace kept: `rm -rf /x/cache` and `rm -rf /x/ cache` differ. `None`
/// when the clone at `top` lacks a commit or a diff is empty. It only names the refusal: no earlier approval counts.
pub(crate) fn diff_unchanged(top: &Path, from: &str, reviewed: &str, p: &PullRequest) -> Option<bool> {
    let base = p.base_ref_name.as_ref()?;
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
            .args(["patch-id", "--verbatim"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(&diff.stdout).ok()?;
        let out = child.wait_with_output().ok()?;
        String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string)
    };
    let reviewed = id(format!("{from}...{reviewed}"))?;
    Some(reviewed == id(format!("refs/remotes/origin/{base}...{}", p.head_ref_oid))?)
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

/// What `land` did to the layers above: those moved onto the base, those stranded on the branch below, and the next
/// step for each child, then the deletion of the branch below while one is stranded.
pub(crate) struct Moved {
    pub(crate) restacked: Vec<Id>,
    pub(crate) stranded: Vec<Id>,
    pub(crate) next: Vec<String>,
}

/// Moves each child onto `target` once the layer `below` has merged. It runs no blocks: a child's code runs where the
/// agent runs it, at `pr`, which the next step names, and the push needs the reviewer's approval again.
pub(crate) fn restack(gh: &Gh, children: &[Child], target: &str, below: &str) -> Result<Moved, Refusal> {
    let mut moved = Moved { restacked: Vec::new(), stranded: Vec::new(), next: Vec::new() };
    for child in children {
        let (id, w) = (&child.id, child.worktree.display());
        match move_onto(gh, child, target)? {
            Onto::Moved => {
                moved.next.push(format!("bp pr {id}; then the reviewer approves its new head"));
                moved.restacked.push(id.clone());
                continue;
            }
            Onto::Conflict => moved.next.push(format!(
                "in {w}: git rebase --onto origin/{target} {}, resolve the conflict, then bp pr {id}",
                child.from
            )),
            Onto::Behind(at) => moved.next.push(format!(
                "in {w}: git merge {at}, which the forge holds, then git rebase --onto origin/{target} {}, then bp pr {id}",
                child.from
            )),
        }
        moved.stranded.push(id.clone());
    }
    if !moved.stranded.is_empty() {
        moved.next.push(format!("once no pull request targets {below}: git push origin --delete {below}"));
    }
    Ok(moved)
}

/// Where [`move_onto`] left a child.
enum Onto {
    Moved,
    /// The rebase conflicted, and was undone.
    Conflict,
    /// The forge's head holds this commit, which the worktree lacks, so a push would drop it.
    Behind(String),
}

/// Moves `child` onto `target`: rebases its own commits, those past its claim's base, onto origin's `target`, pushes
/// with a lease on the head the forge showed before the rebase, and retargets its pull request. The claim's base, not
/// the merged head, bounds them: a child cut before the layer below moved, or a grandchild whose parent was rebased,
/// holds its own commits past it alone. Each step skips what is done, so a land that stopped part way finishes on its
/// rerun. A child left in place stays on the branch below, which `land` keeps, so its pull request stays open.
fn move_onto(gh: &Gh, child: &Child, target: &str) -> Result<Onto, Refusal> {
    let (w, onto) = (&child.worktree, format!("refs/remotes/origin/{target}"));
    let pr = gh.prs_for_head(&child.branch)?.into_iter().find(PullRequest::is_open);
    if git(w, &["merge-base", "--is-ancestor", &onto, "HEAD"]).is_err() {
        if let Some(p) = &pr
            && git(w, &["merge-base", "--is-ancestor", &p.head_ref_oid, "HEAD"]).is_err()
        {
            return Ok(Onto::Behind(p.head_ref_oid.clone()));
        }
        if let Err(e) = git(w, &["rebase", "-q", "--onto", &onto, &child.from]) {
            // Only a conflict leaves a rebase to undo; any other failure is the tool's, and the rerun recovers.
            if !w.join(git(w, &["rev-parse", "--git-path", "rebase-merge"])?).is_dir() {
                return Err(e);
            }
            git(w, &["rebase", "--abort"])?;
            return Ok(Onto::Conflict);
        }
    }
    let head = git(w, &["rev-parse", "HEAD"])?;
    if let Some(pr) = pr {
        if pr.head_ref_oid != head {
            let lease = format!("--force-with-lease=refs/heads/{}:{}", child.branch, pr.head_ref_oid);
            git(w, &["push", "-q", "origin", &lease, &format!("{head}:refs/heads/{}", child.branch)])?;
        }
        // Before the branch below goes: a pull request whose base is deleted closes for good (15, E7).
        if pr.base_ref_name.as_deref() != Some(target) {
            gh.retarget(pr.number, target)?;
        }
    }
    Ok(Onto::Moved)
}
