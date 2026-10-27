//! The view's forge reads (32, design change 4; 18, Forge reader): each project's snapshot is rewritten from the clone
//! at most once a minute. The state is per project, so one project's failure never stops another's reads.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bp_vault::{Code, Index, Refusal, Vault};

/// How often the view reads the forge for one project, at most (18, Forge reader: a choice).
const EVERY: Duration = Duration::from_secs(60);

/// One project's reads.
#[derive(Debug, PartialEq, Eq)]
enum Read {
    /// The last read began then.
    At(Instant),
    /// Never again until the server restarts: `gh` failed, or the clone does not hold the project's repository.
    Off,
}

/// The forge state every request shares.
pub struct Forge {
    /// `--forge off` makes this false, and the view never calls `gh`.
    on: bool,
    /// The clone whose `gh` reads the forge.
    clone: PathBuf,
    reads: Mutex<HashMap<String, Read>>,
}

impl Forge {
    pub fn new(clone: PathBuf, on: bool) -> Self {
        Self { on, clone, reads: Mutex::default() }
    }

    /// The clone `--clone` names: the Change page reads its diff there too, with the forge off.
    pub fn clone_dir(&self) -> &std::path::Path {
        &self.clone
    }

    /// Rewrites the snapshot of `project`, a project the index holds, when its read is due. A failure leaves the
    /// snapshot that exists, which the pages read.
    pub fn refresh(&self, vault: &Vault, index: &Index, project: &str) {
        if !self.on || !due(&mut self.reads.lock().expect("no request panics while it holds the lock"), project) {
            return;
        }
        if let Err(r) = bp_read::snapshot::refresh(vault, index, &self.clone, project) {
            let off = turns_off(&r);
            if off {
                self.reads.lock().expect("no request panics while it holds the lock").insert(project.into(), Read::Off);
            }
            let state = if off { "off" } else { "not read" };
            eprintln!("forge {state} for {project}: {}: {}: {}", r.code, r.subject, r.fix);
        }
    }
}

/// Whether `project`'s read is due now. A due read is marked before it runs, so two requests never read together.
fn due(reads: &mut HashMap<String, Read>, project: &str) -> bool {
    let now = Instant::now();
    match reads.get(project) {
        Some(Read::Off) => false,
        Some(Read::At(at)) if now.duration_since(*at) < EVERY => false,
        _ => {
            reads.insert(project.into(), Read::At(now));
            true
        }
    }
}

/// Whether a failed refresh turns the project's reads off: a `gh` call failed (`Gh` names its refusal `gh <args>`), or
/// the clone does not hold the project's repository, so no later read can succeed. Any other refusal, a snapshot file
/// not written, retries after a minute.
fn turns_off(refusal: &Refusal) -> bool {
    match refusal.code {
        Code::NoClone => true,
        Code::CouldNotRun => refusal.subject.starts_with("gh "),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_is_due_once_a_minute_per_project_and_never_when_off() {
        let mut reads = HashMap::new();
        assert!(due(&mut reads, "bp"), "the first read");
        assert!(!due(&mut reads, "bp"), "within the minute");
        assert!(due(&mut reads, "burgh"), "another project keeps its own minute");
        reads.insert("bp".into(), Read::At(Instant::now().checked_sub(EVERY).unwrap()));
        assert!(due(&mut reads, "bp"), "a minute later");
        reads.insert("bp".into(), Read::Off);
        assert!(!due(&mut reads, "bp"), "off stays off");
        assert!(!due(&mut reads, "burgh"), "bp's state leaves burgh's minute as it was");
        assert_eq!(reads.get("bp"), Some(&Read::Off));
    }

    #[test]
    fn a_gh_failure_or_a_foreign_project_turns_its_reads_off() {
        let gh = Refusal::failed("gh pr list --state open", "gh exited 1");
        let foreign = Refusal::new(Code::NoClone, "/c", "a repo verb runs inside a clone", "cd into a clone");
        assert!(turns_off(&gh) && turns_off(&foreign));
        let write = Refusal::failed("/v/.bp/forge/bp/open-prs.json", "disk full");
        assert!(!turns_off(&write), "a snapshot not written retries");
        assert!(!turns_off(&Refusal::usage("q", "x")));
    }
}
