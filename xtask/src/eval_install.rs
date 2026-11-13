//! The eval sandbox's install: `bp install` writes the forge check's workflow and pin, and each trial's clone stays as
//! the fixture's commit holds it.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// The files `install` writes that a fixture's commit may track: the risk file with its pin, and the forge check's
/// workflow. From PR #99 on, each Blueprint commit tracks the workflow.
const INSTALLED: [&str; 2] = [".bp/risk.yml", ".github/workflows/bp-gate.yml"];

/// Runs `install`, then puts each file of [`INSTALLED`] back as the fixture committed it, or removes it. An arm runs
/// no forge check, and its clone's status stays clean.
pub(crate) fn as_committed(code: &Path, install: impl FnOnce() -> Result<()>) -> Result<()> {
    let saved: Vec<(PathBuf, Option<Vec<u8>>)> =
        INSTALLED.iter().map(|p| (code.join(p), std::fs::read(code.join(p)).ok())).collect();
    install()?;
    for (file, text) in saved {
        match text {
            Some(text) => std::fs::write(&file, text)?,
            None if file.exists() => std::fs::remove_file(&file)?,
            None => {}
        }
    }
    // An empty directory is out of git's sight; these two go only when empty, so a tracked `ci.yml` keeps them.
    std::fs::remove_dir(code.join(".github/workflows")).ok();
    std::fs::remove_dir(code.join(".github")).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_sandbox::git;

    #[test]
    fn a_sandboxed_clone_keeps_the_committed_workflow_and_pin() {
        let t = tempfile::tempdir().unwrap();
        let g = |args: &[&str]| git(t.path(), args).unwrap();
        let write = |path: &str, text: &str| {
            std::fs::create_dir_all(t.path().join(path).parent().unwrap()).unwrap();
            std::fs::write(t.path().join(path), text).unwrap();
        };
        let commit = |msg: &str| {
            let isolated = ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"];
            g(&[&isolated[..], &["commit", "-qm", msg]].concat());
        };
        g(&["init", "-q"]);
        // The bundle's own files are excluded, as `install` excludes them.
        write(".git/info/exclude", "/.claude/\n");
        write(".github/workflows/bp-gate.yml", "committed\n");
        write(".github/workflows/ci.yml", "ci\n");
        g(&["add", "-A"]);
        commit("one");
        // The install rewrites the tracked workflow, adds a risk file the commit lacks, and writes the bundle.
        let ran = std::cell::Cell::new(0);
        let install = || {
            ran.set(ran.get() + 1);
            write(".github/workflows/bp-gate.yml", "installed\n");
            write(".bp/risk.yml", "gate: 0\n");
            write(".claude/x", "bundle\n");
            Ok(())
        };
        as_committed(t.path(), install).unwrap();
        assert_eq!(ran.get(), 1, "the install runs");
        assert_eq!(g(&["status", "--porcelain"]), "", "the clone is as the fixture committed it");
        assert_eq!(std::fs::read_to_string(t.path().join(".github/workflows/bp-gate.yml")).unwrap(), "committed\n");
        assert!(t.path().join(".claude/x").exists(), "the bundle stays");

        // A commit with no workflow: the install's goes, and so do its empty directories.
        g(&["rm", "-q", "-r", ".github"]);
        commit("two");
        as_committed(t.path(), install).unwrap();
        assert_eq!(g(&["status", "--porcelain"]), "");
        assert!(!t.path().join(".github").exists() && !t.path().join(".bp/risk.yml").exists());

        // An install that writes neither file, as a `bp` built off the default branch over a clone with no risk file.
        as_committed(t.path(), || {
            write(".claude/x", "bundle\n");
            Ok(())
        })
        .unwrap();
        assert_eq!(g(&["status", "--porcelain"]), "");
    }
}
