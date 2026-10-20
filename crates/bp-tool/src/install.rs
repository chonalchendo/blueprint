//! `bp install --target <claude|codex>`: the bundle into a harness, at the top of the clone's main worktree (15,
//! Installer; 17, An install; 27, design change 1). The writes are `bp-install`'s; this finds the clone.

use std::path::Path;

use bp_install::Target;
use bp_vault::{Code, Filter, Index, Kind, Refusal, Vault};
use clap::Args;

use crate::Wrote;

/// `bp install --target <claude|codex>`.
#[derive(Args)]
pub struct Install {
    /// The harness: `claude` (Claude Code) or `codex`.
    #[arg(long, value_parser = ["claude", "codex"])]
    pub target: String,
}

pub(crate) fn install(cwd: &Path, i: &Install) -> Result<Wrote, Refusal> {
    let target: Target = i.target.parse().expect("clap holds the target to the set");
    let vault = Vault::discover(cwd)?;
    let index = Index::open(&vault)?;
    let Some(project) = vault.project_for(cwd)? else {
        // Outside any clone the tool cannot tell which project is meant, so the fix names each one's repository.
        let all = Filter { kind: Kind::Project, status: None, project: None, under: None, limit: usize::MAX >> 1 };
        let mut urls = Vec::new();
        for row in index.list(&all)?.0 {
            if let Some(url) = index.header(&row.id)?.and_then(|h| h.get("repository")?.as_str().map(str::to_string)) {
                urls.push(format!("{} {url}", row.id));
            }
        }
        return Err(Refusal::new(
            Code::NoClone,
            cwd.display().to_string(),
            "a repo verb runs inside a clone of the project's repository (15)",
            format!("cd into a clone of one project's repository, then rerun: {}", urls.join(", ")),
        ));
    };
    let top = crate::claim::clone_of(cwd, &index, &project)?;
    Ok(match bp_install::install(&top, target)? {
        (true, v) => Wrote::Changed(v),
        (false, v) => Wrote::Already(v),
    })
}
