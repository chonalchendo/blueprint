//! The forge snapshot's refresh (27, design change 9), shared by the tool's repo verbs and the view (32, design change
//! 4): the clone of a project's repository, and `bp-forge`'s read calls run there into `.bp/forge/<project>/`.

use std::path::{Path, PathBuf};

use bp_forge::Gh;
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
