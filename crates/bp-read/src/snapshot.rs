//! The forge snapshot's refresh (27, design change 9), shared by the tool's repo verbs and the view (32, design change
//! 4): the clone of a project's repository, and `bp-forge`'s read calls run there into `.bp/forge/<project>/`. The
//! read verbs refresh a stale snapshot by the view's age rule (34, design change 3).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bp_forge::{Gh, Snapshot};
use bp_vault::git::Repo;
use bp_vault::{Code, Id, Index, Refusal, Vault};

use crate::inputs::held;

/// The top of the clone of `project`'s repository that holds `cwd`: a repo verb runs nowhere else (26, design change
/// 2).
pub fn clone_of(cwd: &Path, index: &Index, project: &str) -> Result<PathBuf, Refusal> {
    let repository = index
        .header(&format!("projects/{project}/project"))?
        .and_then(|h| h.get("repository")?.as_str().map(str::to_string))
        .unwrap_or_default();
    // From a linked worktree too, the top is the main worktree's, so a new worktree never nests in another.
    Repo::find(cwd)
        .filter(|r| r.remote().is_some_and(|url| bp_vault::git::same_repository(&url, &repository)))
        .and_then(|r| r.main_top())
        .ok_or_else(|| {
            Refusal::new(
                Code::NoClone,
                cwd.display().to_string(),
                "a repo verb runs inside a clone of the project's repository (26, design change 2)",
                format!("cd into a clone of {repository}, or git clone {repository}, then rerun"),
            )
        })
}

/// Rewrites the snapshot of `project` from the clone that holds `cwd`, with a view of each held claim's pull request.
pub fn refresh(vault: &Vault, index: &Index, cwd: &Path, project: &str) -> Result<(), Refusal> {
    let top = clone_of(cwd, index, project)?;
    let held = |slug: &str| {
        Id::parse(&format!("projects/{project}/issues/{slug}"))
            .is_ok_and(|i| held(index, &i).is_ok_and(|h| h.is_some()))
    };
    Gh::new(top).snapshot(&vault.root.join(".bp/forge").join(project), held)
}

/// How old a snapshot grows before a read rewrites it: the view's minute (32, design change 4), which the read verbs
/// share (34, design change 3).
pub const MAX_AGE: Duration = Duration::from_secs(60);

/// Rewrites the snapshot of `project` as [`refresh`] does, unless its oldest read is younger than `max_age`: a fresh
/// snapshot costs no `gh` call. A snapshot that is absent or does not parse is stale.
pub fn refresh_if_stale(
    vault: &Vault,
    index: &Index,
    cwd: &Path,
    project: &str,
    max_age: Duration,
) -> Result<(), Refusal> {
    let oldest = Snapshot::from_dir(&vault.root.join(".bp/forge").join(project)).ok().and_then(|s| s.oldest);
    // A file dated after now (a skewed clock) reads as fresh, so it costs no call on each read.
    if oldest.is_some_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < max_age) {
        return Ok(());
    }
    refresh(vault, index, cwd, project)
}

/// The read verbs' refresh (34, design change 3): each of `projects` whose snapshot is over [`MAX_AGE`] old is
/// rewritten from the clone that holds `cwd`. With `BP_FORGE_SNAPSHOT` set, or outside a clone of the project, the read
/// stays offline. A failed refresh keeps the stale snapshot and never refuses the read: the note names the error.
pub fn refresh_for_read<'a>(
    vault: &Vault,
    index: &Index,
    cwd: &Path,
    projects: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    if std::env::var_os("BP_FORGE_SNAPSHOT").is_some() {
        return None;
    }
    let notes: Vec<String> = projects
        .into_iter()
        .filter_map(|p| match refresh_if_stale(vault, index, cwd, p, MAX_AGE) {
            Err(r) if r.code != Code::NoClone => {
                Some(format!("snapshot of {p} not refreshed: {}: {}", r.subject, r.fix))
            }
            _ => None,
        })
        .collect();
    (!notes.is_empty()).then(|| notes.join("; "))
}
