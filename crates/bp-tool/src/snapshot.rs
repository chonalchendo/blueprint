//! The forge snapshot (27, design change 9): after its forge calls, a repo verb rewrites `.bp/forge/<project>/` in
//! 24's format, so `list` and `walk` read the forge with no call of their own (22, constraint 8). `BP_FORGE_SNAPSHOT`
//! still overrides the read.

use std::path::Path;

use bp_forge::{Gh, Snapshot};
use bp_vault::{Id, Index, Refusal, Vault};

use crate::claim::{clone_of, held};
use crate::subject;

/// Rewrites the snapshot of `issue`'s project from the clone that holds `cwd`, with a view of each held claim's pull
/// request. The verb's own result stands either way: a refresh that fails, or runs outside a clone, leaves the last
/// snapshot, and its `forge_age` tells the reader how old it is.
pub(crate) fn refresh(cwd: &Path, issue: &str) {
    let _ = (|| -> Result<(), Refusal> {
        let vault = Vault::discover(cwd)?;
        let index = Index::open(&vault)?;
        let id = subject(&index, issue)?;
        let project = id.project().unwrap_or_default();
        let top = clone_of(cwd, &index, project)?;
        let held = |slug: &str| {
            Id::parse(&format!("projects/{project}/issues/{slug}"))
                .is_ok_and(|i| held(&index, &i).is_ok_and(|h| h.is_some()))
        };
        Gh::new(top).snapshot(&vault.root.join(".bp/forge").join(project), held)
    })();
}

/// Each project's snapshot under `.bp/forge/`. A project with no `open-prs.json` has none yet.
pub(crate) fn read(root: &Path) -> Result<Vec<(String, Snapshot)>, Refusal> {
    let Ok(entries) = std::fs::read_dir(root.join(".bp/forge")) else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let project = e.file_name().to_string_lossy().into_owned();
        if !e.path().join("open-prs.json").is_file() {
            continue;
        }
        let s = Snapshot::from_dir(&e.path()).map_err(|(path, why)| {
            Refusal::failed(
                path.display().to_string(),
                format!("{why}; a repo verb in the project's clone rewrites it (bp pr <issue>), or delete the file"),
            )
        })?;
        out.push((project, s));
    }
    Ok(out)
}
