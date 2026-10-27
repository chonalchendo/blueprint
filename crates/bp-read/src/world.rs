//! The clock and the forge one read sees. The forge is a snapshot and never a live call (22, constraint 8): after its
//! forge calls, a repo verb rewrites `.bp/forge/<project>/` in 24's format (27, design change 9), and
//! `BP_FORGE_SNAPSHOT` overrides it for every project.

use std::collections::BTreeSet;
use std::path::Path;

use bp_forge::Snapshot;
use bp_vault::Refusal;

/// The clock and the forge one call reads.
pub struct World {
    /// Unix seconds. `BP_NOW` fixes it for parity runs and replays, which must see the clock the reference saw.
    pub now: i64,
    /// `BP_FORGE_SNAPSHOT`'s one snapshot, for every project.
    over: Option<Snapshot>,
    /// The snapshot of each project in view under the vault's `.bp/forge/`, as the repo verbs wrote it (27, design
    /// change 9).
    projects: Vec<(String, Snapshot)>,
}

impl World {
    /// Reads `BP_NOW`, then `BP_FORGE_SNAPSHOT` or the snapshot of each of `projects` in the vault at `root`.
    pub fn read(root: &Path, projects: &BTreeSet<&str>) -> Result<Self, Refusal> {
        let now = crate::clock::now()?;
        if let Some(dir) = std::env::var_os("BP_FORGE_SNAPSHOT") {
            let over = Snapshot::from_dir(Path::new(&dir)).map_err(|(path, e)| {
                Refusal::failed(
                    path.display().to_string(),
                    format!("{e}; point BP_FORGE_SNAPSHOT at a directory of gh JSON, or unset it"),
                )
            })?;
            return Ok(Self { now, over: Some(over), projects: Vec::new() });
        }
        Ok(Self { now, over: None, projects: snapshots(root, projects)? })
    }

    /// The snapshot of the project, if one was read.
    fn snapshot(&self, project: &str) -> Option<&Snapshot> {
        self.over.as_ref().or_else(|| self.projects.iter().find(|(p, _)| p == project).map(|(_, s)| s))
    }

    /// The seconds since the oldest forge read in view; `None` when no snapshot was read. A `BP_NOW` before the
    /// snapshot reads as 0.
    pub fn forge_age(&self) -> Option<i64> {
        let oldest = self.over.iter().chain(self.projects.iter().map(|(_, s)| s)).filter_map(|s| s.oldest).min()?;
        let at = i64::try_from(oldest.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
        Some((self.now - at).max(0))
    }

    /// The open pull request of the issue `id`, in its project's snapshot.
    pub fn pr(&self, id: &str) -> Option<&bp_forge::PullRequest> {
        let (project, slug) = split(id)?;
        self.snapshot(project)?.open_for(slug)
    }

    pub(crate) fn merged(&self, id: &str) -> Option<&bp_forge::PullRequest> {
        let (project, slug) = split(id)?;
        self.snapshot(project)?.merged_for(slug)
    }
}

/// An issue id's project and slug: `projects/<project>/issues/<slug>`.
pub fn split(id: &str) -> Option<(&str, &str)> {
    let mut parts = id.split('/');
    Some((parts.nth(1)?, parts.next_back()?))
}

/// The snapshot of each project in `projects` under `.bp/forge/`; a project with no `open-prs.json` has none yet.
/// Only the projects in view are read, so one bad file breaks only the reads that show its project.
fn snapshots(root: &Path, projects: &BTreeSet<&str>) -> Result<Vec<(String, Snapshot)>, Refusal> {
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
