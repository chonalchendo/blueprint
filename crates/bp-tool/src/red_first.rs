//! Red-first evidence (37, design change 10). `pr` runs each `error` block of the issue at the red-first base before it
//! runs the blocks at the head. The red-first base is the commit where the head leaves the pull request's target: the
//! merge base of the head and the target's sha that `git ls-remote` names, found at each `pr`, so a rebase moves it. A
//! block that passes without the change cannot fail, so it proves nothing about the change, and `pr` refuses
//! `check_vacuous` unless the block carries `prior=pass`. A base run is a run record at the base commit (13, Run): the
//! intents read runs at the head only, and a block runs at a base once per text.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use bp_vault::{Code, Id, Index, Refusal, Vault};
use serde_json::{Value, json};

use crate::claim::git;

/// Each base checkout's directory name starts so, then the pid of its `pr` and an instant.
const PREFIX: &str = "bp-base-";

/// Git locks each base checkout with this reason and the pid of its `pr`, so a later `pr` finds the checkouts of a
/// killed one by git's own record, never by a directory name alone.
const REASON: &str = "bp red-first base, pid ";

/// What `pr` read at the red-first base: each block's latest run, and each base checkout that git did not remove.
#[derive(Default)]
pub(crate) struct Based {
    pub(crate) runs: Vec<BaseRun>,
    pub(crate) left: Vec<String>,
}

/// A red-first block's latest run at the base, of its current text.
pub(crate) struct BaseRun {
    pub(crate) name: String,
    pub(crate) verdict: String,
    pub(crate) id: String,
}

impl BaseRun {
    /// The row of `pr`'s `base_runs`.
    pub(crate) fn row(&self) -> Value {
        json!({ "name": self.name, "verdict": self.verdict, "id": self.id })
    }
}

/// The red-first base of `head`: its merge base with the sha of `target` that `git ls-remote` names, fetched
/// ([`crate::risk::at_origin`]), never a local ref and never the claim's `base`, which `cut` wrote and no rebase moves.
pub(crate) fn base(top: &Path, target: &str, head: &str) -> Result<String, Refusal> {
    let Some(tip) = crate::risk::at_origin(top, target)? else {
        return Err(Refusal::failed(
            format!("origin/{target}"),
            format!("origin holds no branch {target}, the pull request's target; push it, then rerun"),
        ));
    };
    git(top, &["merge-base", &tip, head])
}

/// Runs at `base` each red-first block of `id` that has no run there of its current text, and commits the runs. The
/// blocks run in a detached checkout of `base`, made from the clone at `top` and removed after, never in the claim's
/// worktree, so the agent's work stays as it is. The checkout builds into a `target` of its own: cargo judges an
/// artifact fresh by file time, so a build directory shared with the head run would hand the head the base's build.
/// Then refuses `check_vacuous` when the latest run at `base` of the current text of any red-first block passed. Else
/// returns each block's latest run there, in body order: a verdict other than `fail` refuses nothing, and `pr` names it.
pub(crate) fn base_runs(
    vault: &Vault,
    index: &Index,
    id: &Id,
    top: &Path,
    base: &str,
    role: &str,
    clock: &dyn Fn() -> String,
) -> Result<Based, Refusal> {
    let mut left = sweep(top);
    let file = vault.read(id)?;
    let named = file.body.named_checks();
    let runs = bp_read::inputs::runs(index, id.as_str())?;
    let (mut rows, mut unrun) = (Vec::new(), Vec::new());
    for check in named.iter().filter(|c| bp_check::red_first(c)) {
        let (name, hash) = (check.attrs["name"].as_str(), check.hash());
        let known = runs.iter().filter(|r| r.commit == base && r.check == name && r.check_hash == hash);
        match known.max_by_key(|r| r.attempt) {
            Some(r) => {
                let header = index.header(&r.id)?.unwrap_or_default();
                let verdict = header.get("verdict").and_then(Value::as_str).unwrap_or_default().to_string();
                rows.push(BaseRun { name: name.to_string(), verdict, id: r.id.clone() });
            }
            None => unrun.push(name),
        }
    }
    if !unrun.is_empty() {
        let skip: Vec<&str> = named.iter().map(|c| c.attrs["name"].as_str()).filter(|n| !unrun.contains(n)).collect();
        let checkout = Checkout::new(top, base)?;
        let target = checkout.path.join("target");
        let env = [("CARGO_TARGET_DIR", target.as_path())];
        let ran = bp_check::run(vault, id, &checkout.path, None, &skip, &env, clock)?;
        left.extend(checkout.close());
        if let Some(ran) = ran {
            let writer = ran.writer(vault, &format!("bp pr {id}"))?;
            let (changes, new) = ran.records(vault);
            writer.commit("pr", role, &format!("pr {id} at base {base}"), &changes)?;
            let row =
                |r: bp_check::Run| BaseRun { name: r.name, verdict: r.verdict.as_str().into(), id: r.id.to_string() };
            rows.extend(new.into_iter().map(row));
        }
    }
    let order: Vec<&str> = named.iter().map(|c| c.attrs["name"].as_str()).collect();
    rows.sort_by_key(|r| order.iter().position(|n| *n == r.name));
    let passed: Vec<&BaseRun> = rows.iter().filter(|r| r.verdict == "pass").collect();
    let Some(first) = passed.first() else { return Ok(Based { runs: rows, left }) };
    let blocks: Vec<&str> = passed.iter().map(|r| r.name.as_str()).collect();
    Err(Refusal::new(
        Code::CheckVacuous,
        first.id.as_str(),
        "a block proves a change only when it fails without it, so each error block fails at the red-first base, where the head leaves the pull request's target (37, design change 10)",
        format!(
            "{} passed at the base {base}; make each fail without the change, or add prior=pass to the attributes of a guard that must hold before and after it, with bp set {id} --body-file <file>, then bp pr {id}; a worker edits no issue, so it runs bp release {id} --reason escalated and replies with the blocks' names",
            blocks.join(", ")
        ),
    ))
}

/// Removes each base checkout of the clone at `top` that a killed `pr` left: a signal runs no drop, so the directory and
/// git's entry for it stay. An entry is a base checkout only when git lists it as locked with [`REASON`], detached, and
/// not first (the clone's own working tree), and its directory is in [`checkouts`] with [`PREFIX`]: a claim's worktree
/// carries a branch, so a slug or a clone named like a checkout stays. A checkout whose `pr` is alive, or whose pid
/// `ps` cannot judge, stays. Returns each checkout that git did not remove.
fn sweep(top: &Path) -> Vec<String> {
    let Ok(listed) = git(top, &["worktree", "list", "--porcelain"]) else { return Vec::new() };
    let home = checkouts();
    let mut left = Vec::new();
    for entry in listed.split("\n\n").skip(1) {
        let lines: Vec<&str> = entry.lines().collect();
        let Some(path) = lines.iter().find_map(|l| l.strip_prefix("worktree ")).map(Path::new) else { continue };
        let named = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(PREFIX));
        let ours = named && path.parent() == Some(home.as_path()) && lines.contains(&"detached");
        let pid = lines.iter().find_map(|l| l.strip_prefix("locked ")?.strip_prefix(REASON));
        let Some(pid) = pid.filter(|p| ours && !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) else {
            continue;
        };
        // `ps -p` exits non-zero only when no process has the pid, whoever owns it.
        if Command::new("ps").args(["-p", pid]).output().is_ok_and(|o| !o.status.success()) {
            left.extend(remove(top, path));
        }
    }
    left
}

/// The directory `pr` makes base checkouts in: the system's temporary directory, canonical, as git lists a worktree.
fn checkouts() -> PathBuf {
    let dir = std::env::temp_dir();
    dir.canonicalize().unwrap_or(dir)
}

/// Removes a base checkout and git's entry for it, or returns its path when git refuses. `--force`: a block may leave
/// build output in the checkout. Only git removes it: a path that git refuses may not be a base checkout.
fn remove(top: &Path, path: &Path) -> Option<String> {
    let path = path.display().to_string();
    let _ = git(top, &["worktree", "unlock", &path]);
    git(top, &["worktree", "remove", "--force", &path]).err().map(|_| path)
}

/// A detached checkout of a base commit in [`checkouts`], locked with [`REASON`] and removed on drop, so a block that
/// refuses or panics leaves no worktree behind.
struct Checkout {
    top: PathBuf,
    path: PathBuf,
    open: bool,
}

impl Checkout {
    fn new(top: &Path, base: &str) -> Result<Self, Refusal> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        let pid = std::process::id();
        let path = checkouts().join(format!("{PREFIX}{pid}-{nanos}"));
        let reason = format!("{REASON}{pid}");
        let add =
            ["worktree", "add", "-q", "--detach", "--lock", "--reason", &reason, &path.display().to_string(), base];
        git(top, &add)?;
        Ok(Self { top: top.to_path_buf(), path, open: true })
    }

    /// Removes the checkout, or returns its path when git refuses.
    fn close(mut self) -> Option<String> {
        self.open = false;
        remove(&self.top, &self.path)
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        if self.open {
            let _ = remove(&self.top, &self.path);
        }
    }
}
