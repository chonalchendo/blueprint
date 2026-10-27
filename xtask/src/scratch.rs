//! `cargo xtask scratch`: a writable copy of the vault for a block that writes (25, Finish line). It copies
//! `projects`, `shared` and `.bp` (all but the view's use log) of `$BP_VAULT` into a fresh directory under
//! `$TMPDIR/bp-scratch`, makes it a git repository with one commit in one pack, and returns the directory. Nothing
//! writes into `$BP_VAULT`.
//!
//! A block cannot delete its copy when `bash -e` stops it early, so the copies leaked: 120 of them, 15 GB, filled the
//! disk mid-run. Each call now keeps the newest copies, and any modified in the last ten minutes, and deletes the
//! rest before it makes its own.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Copies kept, the new one included: about 125 MB each on the converted vault, so about 2 GB outside a burst.
const KEEP: usize = 16;

/// No prune deletes a directory modified this recently, however many are newer. Parallel workers make forges and
/// copies together: four workers running six blocks make 24 in one round, and a block runs for tens of seconds.
const GRACE: std::time::Duration = std::time::Duration::from_secs(600);

/// Copies the vault and commits the copy once. `.bp` comes too, so the copy's first read reuses the index's hashes
/// instead of parsing every file again.
pub fn run() -> Result<PathBuf> {
    let src = std::env::var_os("BP_VAULT").context("set BP_VAULT to the vault to copy")?;
    let src = Path::new(&src).canonicalize().with_context(|| format!("read BP_VAULT {}", src.display()))?;
    let dir = fresh(&std::env::temp_dir().join("bp-scratch"), KEEP)?;
    copy(&src, &dir)?;
    Ok(dir)
}

/// Copies `projects`, `shared` and `.bp` of the vault at `src` into the empty `dir`, and commits them in one pack.
/// `.bp/use.log` stays behind: it is the source's census of the view's pages, and a block counts the copy's own lines.
fn copy(src: &Path, dir: &Path) -> Result<()> {
    let parts: Vec<PathBuf> =
        ["projects", "shared", ".bp"].iter().map(|p| src.join(p)).filter(|p| p.exists()).collect();
    if !parts.iter().any(|p| p.ends_with("projects")) {
        bail!("{} holds no projects/: BP_VAULT names the vault root", src.display());
    }
    run_in(dir, Command::new("cp").arg("-R").args(&parts).arg(dir))?;
    match std::fs::remove_file(dir.join(".bp/use.log")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e).context("remove the copied .bp/use.log"),
        _ => {}
    }
    // The copy's own identity, so a commit works where git has none configured, as on CI.
    run_in(dir, Command::new("git").args(["init", "-q"]))?;
    run_in(dir, Command::new("git").args(["config", "user.name", "bp scratch"]))?;
    run_in(dir, Command::new("git").args(["config", "user.email", "scratch@bp.invalid"]))?;
    run_in(dir, Command::new("git").args(["add", "-A"]))?;
    // `add -A` leaves every file as a loose object, about 10,000 on the converted vault, where the vault's own
    // repository holds one pack. A commit then forked `git gc --auto`, which repacked in the background for seconds
    // while a timed block ran, and charged whichever loop came first (25, issue 9). So the copy is packed here, and
    // its own commit forks no gc to race the repack.
    run_in(dir, Command::new("git").args(["-c", "gc.auto=0", "commit", "-q", "--no-verify", "-m", "scratch copy"]))?;
    run_in(dir, Command::new("git").args(["repack", "-adq"]))?;
    Ok(())
}

/// A new empty directory under `parent`, after pruning there.
pub(crate) fn fresh(parent: &Path, keep: usize) -> Result<PathBuf> {
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    prune(parent, keep)?;
    let out = Command::new("mktemp").arg("-d").arg(parent.join("XXXXXXXX")).output().context("run mktemp -d")?;
    if !out.status.success() {
        bail!("mktemp -d exited {}", out.status);
    }
    Ok(PathBuf::from(String::from_utf8(out.stdout).context("mktemp printed a path")?.trim()))
}

/// Deletes all but the newest `keep - 1` directories under `parent`, by the time each was last modified, and spares
/// any modified within `GRACE`.
fn prune(parent: &Path, keep: usize) -> Result<()> {
    let cutoff = std::time::SystemTime::now() - GRACE;
    let mut copies: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(parent)?
        .filter_map(|e| {
            let e = e.ok()?;
            Some((e.metadata().ok()?.modified().ok()?, e.path()))
        })
        .collect();
    copies.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in copies.iter().skip(keep - 1).filter(|(at, _)| *at < cutoff) {
        // A copy another run deletes first is already gone, which is the goal.
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(())
}

pub(crate) fn run_in(dir: &Path, cmd: &mut Command) -> Result<()> {
    // Stdout carries the directory alone.
    let status = cmd
        .current_dir(dir)
        .env("LC_ALL", "C")
        .stdout(std::process::Stdio::null())
        .status()
        .with_context(|| format!("run {cmd:?}"))?;
    if !status.success() {
        bail!("{cmd:?} exited {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_leaves_the_use_log_behind() {
        let (src, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let kept = ["projects/bp/project.md", "shared/assets/g.md", ".bp/index.db", ".bp/forge/bp/x.json"];
        for rel in kept.iter().chain(&[".bp/use.log"]) {
            let path = src.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x\n").unwrap();
        }
        copy(src.path(), dir.path()).unwrap();
        assert!(!dir.path().join(".bp/use.log").exists(), "the source's use log came with the copy");
        for rel in kept {
            assert!(dir.path().join(rel).is_file(), "{rel} did not come");
        }
    }

    #[test]
    fn prune_spares_a_directory_modified_within_the_grace() {
        let parent = tempfile::tempdir().unwrap();
        let (old, young) = (parent.path().join("old"), parent.path().join("young"));
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&young).unwrap();
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::open(&old).unwrap().set_modified(hour_ago).unwrap();
        prune(parent.path(), 1).unwrap();
        assert!(!old.exists() && young.exists(), "past the count, only the old one goes");
    }
}
