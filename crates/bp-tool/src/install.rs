//! `bp install --target <claude|codex>`: the bundle into a harness, at the top of the clone's main worktree (15,
//! Installer; 17, An install; 27, design change 1). The writes are `bp-install`'s; this finds the clone.

use std::path::Path;

use bp_install::{Pin, Target};
use bp_vault::{Code, Filter, Index, Kind, Refusal, Vault};
use clap::Args;

use crate::Wrote;

/// `bp install --target <claude|codex> [--gate <sha>]`.
#[derive(Args)]
pub struct Install {
    /// The harness: `claude` (Claude Code) or `codex`.
    #[arg(long, value_parser = ["claude", "codex"])]
    pub target: String,
    /// The Blueprint commit the forge check `bp-gate` builds `bp` from, written as `gate:` in `.bp/risk.yml`; it
    /// replaces a pin the file holds. Default: the commit this `bp` is built from, written only into a file with no
    /// pin. A `bp` built from a commit off Blueprint's default branch writes no pin, and the check fails until the
    /// human adds one.
    #[arg(long)]
    pub gate: Option<String>,
}

pub(crate) fn install(cwd: &Path, i: &Install) -> Result<Wrote, Refusal> {
    let target: Target = i.target.parse().expect("clap holds the target to the set");
    let built = Some(bp_install::BUILT_FROM).filter(|s| !s.is_empty()).map(Pin::Built);
    let gate = i.gate.as_deref().map(Pin::Given).or(built);
    if let Some(Pin::Given(g) | Pin::Built(g)) = gate
        && !bp_change::risk::is_commit(g)
    {
        return Err(Refusal::usage(
            format!("--gate {g}"),
            "--gate takes the full 40-digit sha of a Blueprint commit that holds bp gate",
        ));
    }
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
    let top = bp_read::snapshot::clone_of(cwd, &index, &project)?;
    let (wrote, v) = bp_install::install(&top, target, &project, gate)?;
    if gate.is_none() && v["gate"].is_null() {
        eprintln!(
            "bp install wrote no pin: this bp is not built from a commit on Blueprint's default branch, so the \
             bp-gate check fails; rerun with --gate <sha of a commit on Blueprint's default branch>"
        );
    }
    Ok(if wrote { Wrote::Changed(v) } else { Wrote::Already(v) })
}
