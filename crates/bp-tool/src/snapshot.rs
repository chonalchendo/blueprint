//! The forge snapshot (27, design change 9): after its forge calls, a repo verb rewrites `.bp/forge/<project>/` in
//! 24's format. `list` and `walk` call `gh` only to refresh a snapshot a minute old or older (34, design change 3). `bp_read::World`
//! reads it, and `bp_read::snapshot` writes it for the verbs and the view alike.

use std::path::Path;

use bp_vault::{Index, Refusal, Vault};

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
        bp_read::snapshot::refresh(&vault, &index, cwd, &project)
    })();
    if let Err(r) = refreshed {
        eprintln!("snapshot of {project} not refreshed: {}: {}", r.subject, r.fix);
    }
}
