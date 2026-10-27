//! The forge snapshot (27, design change 9): after its forge calls, a repo verb rewrites `.bp/forge/<project>/` in
//! 24's format, so `list` and `walk` read the forge with no call of their own (22, constraint 8). `bp_read::World`
//! reads it.

use std::path::Path;

use bp_forge::Gh;
use bp_read::inputs::held;
use bp_vault::{Id, Index, Refusal, Vault};

use crate::claim::clone_of;
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
