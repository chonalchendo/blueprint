//! Red-first evidence (37, design change 10). `pr` runs each `error` block of the issue at the claim's base before it
//! runs the blocks at the head. A block that passes without the change cannot fail, so it proves nothing about the
//! change, and `pr` refuses `check_vacuous` unless the block carries `prior=pass`. A base run is a run record at the
//! base commit (13, Run): the intents read runs at the head only, and a block runs at a base once per text.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bp_check::Verdict;
use bp_vault::{Code, Id, Index, Refusal, Vault};

use crate::claim::git;

/// Runs at `base` each red-first block of `id` that has no run there of its current text, and commits the runs. The
/// blocks run in a detached checkout of `base`, made from the clone at `top` and removed after, never in the claim's
/// worktree, so the agent's work stays as it is. Then refuses `check_vacuous` when the latest run at `base` of the
/// current text of any red-first block passed. A base run that could not run or timed out is no pass, so it does not
/// refuse: the head runs and the review still judge the block.
pub(crate) fn base_runs(
    vault: &Vault,
    index: &Index,
    id: &Id,
    top: &Path,
    base: &str,
    role: &str,
    clock: &dyn Fn() -> String,
) -> Result<(), Refusal> {
    let file = vault.read(id)?;
    let named = file.body.named_checks();
    let runs = bp_read::inputs::runs(index, id.as_str())?;
    let (mut passed, mut unrun) = (Vec::new(), Vec::new());
    for check in named.iter().filter(|c| bp_check::red_first(c)) {
        let (name, hash) = (check.attrs["name"].as_str(), check.hash());
        let known = runs.iter().filter(|r| r.commit == base && r.check == name && r.check_hash == hash);
        match known.max_by_key(|r| r.attempt) {
            Some(r) if r.passed => passed.push((name.to_string(), r.id.clone())),
            Some(_) => {}
            None => unrun.push(name),
        }
    }
    if !unrun.is_empty() {
        let skip: Vec<&str> = named.iter().map(|c| c.attrs["name"].as_str()).filter(|n| !unrun.contains(n)).collect();
        let checkout = Checkout::new(top, base)?;
        let ran = bp_check::run(vault, id, &checkout.path, None, &skip, clock)?;
        drop(checkout);
        if let Some(ran) = ran {
            let writer = ran.writer(vault, &format!("bp pr {id}"))?;
            let (changes, rows) = ran.records(vault);
            writer.commit("pr", role, &format!("pr {id} at base {base}"), &changes)?;
            let pass = rows.into_iter().filter(|r| r.verdict == Verdict::Pass);
            passed.extend(pass.map(|r| (r.name, r.id.to_string())));
        }
    }
    let Some((_, first)) = passed.first() else { return Ok(()) };
    let blocks: Vec<&str> = passed.iter().map(|(n, _)| n.as_str()).collect();
    Err(Refusal::new(
        Code::CheckVacuous,
        first.as_str(),
        "a block proves a change only when it fails without it, so each error block fails at the claim's base (37, design change 10)",
        format!(
            "{} passed at the base {base}; make each fail without the change, or add prior=pass to the attributes of a guard that must hold before and after it; bp set {id} --body-file <file>, then bp pr {id}",
            blocks.join(", ")
        ),
    ))
}

/// A detached checkout of a base commit in the system's temporary directory, removed on drop, so a block that
/// refuses or panics leaves no worktree behind.
struct Checkout {
    top: PathBuf,
    path: PathBuf,
}

impl Checkout {
    fn new(top: &Path, base: &str) -> Result<Self, Refusal> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
        let path = std::env::temp_dir().join(format!("bp-base-{}-{nanos}", std::process::id()));
        git(top, &["worktree", "add", "-q", "--detach", &path.display().to_string(), base])?;
        Ok(Self { top: top.to_path_buf(), path })
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        // `--force`: a block may leave build output in the checkout. If git cannot remove it, the directory goes and
        // the prune drops git's note of it.
        let path = self.path.display().to_string();
        if git(&self.top, &["worktree", "remove", "--force", &path]).is_err() {
            let _ = std::fs::remove_dir_all(&self.path);
            let _ = git(&self.top, &["worktree", "prune"]);
        }
    }
}
