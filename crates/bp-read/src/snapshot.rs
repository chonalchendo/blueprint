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
    rewrite(vault, index, cwd, project, None)
}

/// [`refresh`], with each `gh` call killed after `deadline` when one is given.
fn rewrite(vault: &Vault, index: &Index, cwd: &Path, project: &str, deadline: Option<Duration>) -> Result<(), Refusal> {
    let top = clone_of(cwd, index, project)?;
    let held = |slug: &str| {
        Id::parse(&format!("projects/{project}/issues/{slug}"))
            .is_ok_and(|i| held(index, &i).is_ok_and(|h| h.is_some()))
    };
    let gh = Gh::new(top);
    let gh = if let Some(limit) = deadline { gh.within(limit) } else { gh };
    gh.snapshot(&vault.root.join(".bp/forge").join(project), held)
}

/// How old a snapshot grows before a read rewrites it: the view's minute (32, design change 4), which the read verbs
/// share (34, design change 3).
pub const MAX_AGE: Duration = Duration::from_secs(60);

/// How long a read waits for one `gh` call before it kills it: a read must not hang on the network.
const READ_DEADLINE: Duration = Duration::from_secs(10);

/// The longest `forge_error` a read prints, in characters: the first line of the error, cut there.
const ERROR_MAX: usize = 200;

/// Rewrites the snapshot of `project` as [`refresh`] does, with each `gh` call cut at [`READ_DEADLINE`], unless its
/// oldest read is under [`MAX_AGE`] old: a fresh snapshot costs no `gh` call. A snapshot that is absent or does not
/// parse is stale, and so is one dated after now (a skewed clock): the refresh rewrites its times, so it costs one call.
pub fn refresh_if_stale(vault: &Vault, index: &Index, cwd: &Path, project: &str) -> Result<(), Refusal> {
    let oldest = Snapshot::from_dir(&vault.root.join(".bp/forge").join(project)).ok().and_then(|s| s.oldest);
    if oldest.is_some_and(|t| SystemTime::now().duration_since(t).is_ok_and(|age| age < MAX_AGE)) {
        return Ok(());
    }
    rewrite(vault, index, cwd, project, Some(READ_DEADLINE))
}

/// Whether a `gh` is on `PATH`. With none, a read is offline, as outside a clone.
fn gh_on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("gh").is_file()))
}

/// The read verbs' refresh (34, design change 3): each of `projects` whose snapshot is over [`MAX_AGE`] old is
/// rewritten from the clone that holds `cwd`. A read refreshes only a snapshot a repo verb already wrote, so a vault
/// no repo verb touched stays offline and unwritten: `list` and `walk` never create one. With `BP_FORGE_SNAPSHOT`
/// set, with no `gh` on `PATH`, or outside a clone of the project, the read stays offline too. A failed refresh keeps
/// the stale snapshot and never refuses the read: the `forge_error` names the project and the error's first line.
pub fn refresh_for_read<'a>(
    vault: &Vault,
    index: &Index,
    cwd: &Path,
    projects: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    if std::env::var_os("BP_FORGE_SNAPSHOT").is_some() || !gh_on_path() {
        return None;
    }
    let notes: Vec<String> = projects
        .into_iter()
        .filter(|p| vault.root.join(".bp/forge").join(p).join("open-prs.json").is_file())
        .filter_map(|p| match refresh_if_stale(vault, index, cwd, p) {
            Err(r) if r.code != Code::NoClone => {
                let text = format!("{p}: {} ({})", r.fix, r.subject);
                Some(text.lines().next().unwrap_or_default().chars().take(ERROR_MAX).collect())
            }
            _ => None,
        })
        .collect();
    (!notes.is_empty()).then(|| notes.join("; "))
}
