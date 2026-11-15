//! Red-first evidence (37, design change 10). `pr` runs each `error` block of the issue at the red-first base before it
//! runs the blocks at the head. The red-first base is the commit where the head leaves the pull request's target: the
//! merge base of the head and the target's sha that `git ls-remote` names, found at each `pr`, so a rebase moves it. A
//! block that passes without the change cannot fail, so it proves nothing about the change, and `pr` refuses
//! `check_vacuous` unless the block carries `prior=pass`. A base run is a run record at the base commit (13, Run): the
//! intents read runs at the head only, and a block runs at a base once per text.

use std::ffi::OsStr;
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
    let mut left = sweep(top, OsStr::new("ps"));
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
    if passed.is_empty() {
        return Ok(Based { runs: rows, left });
    }
    Err(vacuous(id, base, &passed, &left))
}

/// `check_vacuous` for the blocks that `passed` at `base`. The fix names each base checkout that git did not remove, as
/// a passing `pr` names them in its result, so a refused run does not drop them (review of #97, row 17).
fn vacuous(id: &Id, base: &str, passed: &[&BaseRun], left: &[String]) -> Refusal {
    let blocks: Vec<&str> = passed.iter().map(|r| r.name.as_str()).collect();
    let left = if left.is_empty() {
        String::new()
    } else {
        format!("; git did not remove the base checkouts {}, so remove each by hand", left.join(", "))
    };
    Refusal::new(
        Code::CheckVacuous,
        passed[0].id.as_str(),
        "a block proves a change only when it fails without it, so each error block fails at the red-first base, where the head leaves the pull request's target (37, design change 10)",
        format!(
            "{} passed at the base {base}; make each fail without the change, or add prior=pass to the attributes of a guard that must hold before and after it, with bp set {id} --body-file <file>, then bp pr {id}; a worker edits no issue, so it runs bp release {id} --reason escalated and replies with the blocks' names{left}",
            blocks.join(", ")
        ),
    )
}

/// Removes each base checkout of the clone at `top` that a killed `pr` left: a signal runs no drop, so the directory and
/// git's entry for it stay. An entry is a base checkout only when git lists it as locked with [`REASON`], detached, and
/// not first (the clone's own working tree), and its directory is in [`checkouts`] with [`PREFIX`]: a claim's worktree
/// carries a branch, so a slug or a clone named like a checkout stays. A checkout whose `pr` is alive, or whose pid
/// `ps` cannot judge, stays. `ps` is trusted only when it finds this process: a `ps` that a sandbox denies, or one with
/// no `-p`, exits non-zero for every pid, and would read a live `pr`'s checkout as dead (review of #97, row 16). Then
/// nothing is swept. Returns each checkout that git did not remove.
fn sweep(top: &Path, ps: &OsStr) -> Vec<String> {
    let alive = |pid: &str| Command::new(ps).args(["-p", pid]).output().map(|o| o.status.success());
    if !alive(&std::process::id().to_string()).unwrap_or(false) {
        return Vec::new();
    }
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
        // A `ps` that found this process exits non-zero only when no process has the pid, whoever owns it.
        if alive(pid).is_ok_and(|ok| !ok) {
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
/// build output in the checkout; twice, so git removes it locked. It is never unlocked first: a remove that fails, or
/// a `pr` killed in it, leaves the lock, so each later sweep tries again and names it (review of #97, row 18). Only
/// git removes it: a path that git refuses may not be a base checkout.
fn remove(top: &Path, path: &Path) -> Option<String> {
    let path = path.display().to_string();
    git(top, &["worktree", "remove", "--force", "--force", &path]).err().map(|_| path)
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

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command;

    use bp_vault::Id;

    use super::{BaseRun, PREFIX, REASON, checkouts, sweep, vacuous};

    fn git(dir: &Path, args: &[&str]) -> String {
        let isolated = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1")];
        let out = Command::new("git").current_dir(dir).envs(isolated).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// A pid no process holds now: a child that has exited and been reaped.
    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    /// A clone with one commit and a base checkout of a killed `pr`, locked as `pr` locks one; the clone and the path.
    fn killed(tag: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "one"]);
        let pid = dead_pid();
        let path = checkouts().join(format!("{PREFIX}{pid}-{tag}-{}", std::process::id())).display().to_string();
        let reason = format!("{REASON}{pid}");
        git(dir.path(), &["worktree", "add", "-q", "--detach", "--lock", "--reason", &reason, &path, "HEAD"]);
        (dir, path)
    }

    fn locked(top: &Path, path: &str) -> bool {
        git(top, &["worktree", "list", "--porcelain"])
            .split("\n\n")
            .any(|e| e.contains(path) && e.contains("\nlocked "))
    }

    /// A `ps` that cannot find this process judges no pid: the sweep removes nothing (review of #97, row 16).
    #[test]
    fn a_ps_that_cannot_find_this_process_sweeps_nothing() {
        let (dir, path) = killed("ps");
        let stub = tempfile::tempdir().unwrap();
        let ps = stub.path().join("ps");
        std::fs::write(&ps, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&ps, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        assert!(sweep(dir.path(), ps.as_os_str()).is_empty());
        assert!(Path::new(&path).is_dir() && locked(dir.path(), &path), "a stub ps that fails keeps the checkout");
        assert!(sweep(dir.path(), OsStr::new("ps")).is_empty(), "git removed it");
        assert!(!Path::new(&path).exists() && !locked(dir.path(), &path), "the system ps finds the pid dead");
    }

    /// A checkout git refuses to remove keeps its lock, so each later sweep tries it again and names it (review of #97,
    /// row 18). Git refuses one whose `.git` file is gone before it deletes anything; a remove that fails while it
    /// deletes drops git's entry, lock and all, so that checkout is named once (git 2.50).
    #[test]
    fn a_checkout_git_refuses_to_remove_stays_locked_and_named() {
        let (dir, path) = killed("stuck");
        std::fs::remove_file(Path::new(&path).join(".git")).unwrap();
        for _ in 0..2 {
            assert_eq!(sweep(dir.path(), OsStr::new("ps")), std::slice::from_ref(&path), "each sweep names it");
            assert!(locked(dir.path(), &path), "a refused remove leaves the lock");
        }
        std::fs::remove_dir_all(&path).unwrap();
    }

    /// A `check_vacuous` refusal names the checkouts that git left (review of #97, row 17).
    #[test]
    fn a_vacuous_refusal_names_the_checkouts_left() {
        let id = Id::parse("projects/bp/issues/a").unwrap();
        let run = BaseRun { name: "b".into(), verdict: "pass".into(), id: "projects/bp/issues/a/runs/r".into() };
        let r = vacuous(&id, "abc", &[&run], &["/tmp/bp-base-1-2".into()]);
        assert!(r.fix.contains("b passed at the base abc") && r.fix.contains("/tmp/bp-base-1-2"), "{}", r.fix);
        assert!(!vacuous(&id, "abc", &[&run], &[]).fix.contains("checkouts"), "none left, none named");
    }
}
