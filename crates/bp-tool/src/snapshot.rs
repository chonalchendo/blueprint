//! The forge snapshot (27, design change 9): after its forge calls, a repo verb rewrites `.bp/forge/<project>/` in
//! 24's format, so `list` and `walk` read the forge with no call of their own (22, constraint 8). `BP_FORGE_SNAPSHOT`
//! still overrides the read.

use std::collections::BTreeSet;
use std::path::Path;

use bp_forge::{Gh, Snapshot};
use bp_vault::{Id, Index, Refusal, Vault};

use crate::claim::{clone_of, held};
use crate::subject;

/// Rewrites the snapshot of `issue`'s project from the clone that holds `cwd`, with a view of each held claim's pull
/// request. The verb's own result stands either way: a refresh that fails, or runs outside a clone, leaves the last
/// snapshot and says so on stderr, outside the JSON.
pub(crate) fn refresh(cwd: &Path, issue: &str) {
    // Until the issue resolves, the line names the argument.
    let mut project = issue.to_string();
    let refreshed = (|| -> Result<(), Refusal> {
        let vault = Vault::discover(cwd)?;
        let index = Index::open(&vault)?;
        let id = subject(&index, issue)?;
        id.project().unwrap_or_default().clone_into(&mut project);
        let top = clone_of(cwd, &index, &project)?;
        let held = |slug: &str| {
            Id::parse(&format!("projects/{project}/issues/{slug}"))
                .is_ok_and(|i| held(&index, &i).is_ok_and(|h| h.is_some()))
        };
        Gh::new(top).snapshot(&vault.root.join(".bp/forge").join(&project), held)
    })();
    if let Err(r) = refreshed {
        eprintln!("snapshot of {project} not refreshed: {}: {}", r.subject, r.fix);
    }
}

/// The snapshot of each project in `projects` under `.bp/forge/`; a project with no `open-prs.json` has none yet.
/// Only the projects in view are read, so one bad file breaks only the reads that show its project.
pub(crate) fn read(root: &Path, projects: &BTreeSet<&str>) -> Result<Vec<(String, Snapshot)>, Refusal> {
    let mut out = Vec::new();
    for project in projects {
        let dir = root.join(".bp/forge").join(project);
        if !dir.join("open-prs.json").is_file() {
            continue;
        }
        let s = Snapshot::from_dir(&dir).map_err(|(path, why)| {
            Refusal::failed(
                path.display().to_string(),
                format!("{why}; a repo verb in the project's clone rewrites it (bp pr <issue>), or delete the file"),
            )
        })?;
        out.push(((*project).to_string(), s));
    }
    Ok(out)
}
