//! `cargo xtask scratch`: a writable copy of the vault for a block that writes (25, Finish line). It copies
//! `projects`, `shared` and `.bp` of `$BP_VAULT` into a fresh directory under `$TMPDIR/bp-scratch`, makes it a git
//! repository with one commit, and returns the directory. Nothing writes into `$BP_VAULT`.
//!
//! A block cannot delete its copy when `bash -e` stops it early, so the copies leaked: 120 of them, 15 GB, filled the
//! disk mid-run. Each call now keeps only the newest copies and deletes the rest before it makes its own.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Copies kept, the new one included: about 125 MB each on the converted vault, so at most 2 GB. Enough for parallel
/// block runs; a copy older than the newest 15 may vanish under a long-held shell.
const KEEP: usize = 16;

/// Copies the vault and commits the copy once. `.bp` comes too, so the copy's first read reuses the index's hashes
/// instead of parsing every file again.
pub fn run() -> Result<PathBuf> {
    let src = std::env::var_os("BP_VAULT").context("set BP_VAULT to the vault to copy")?;
    let src = Path::new(&src).canonicalize().with_context(|| format!("read BP_VAULT {}", src.display()))?;
    let parent = std::env::temp_dir().join("bp-scratch");
    std::fs::create_dir_all(&parent).with_context(|| format!("create {}", parent.display()))?;
    prune(&parent)?;
    let out = Command::new("mktemp").arg("-d").arg(parent.join("XXXXXXXX")).output().context("run mktemp -d")?;
    if !out.status.success() {
        bail!("mktemp -d exited {}", out.status);
    }
    let dir = PathBuf::from(String::from_utf8(out.stdout).context("mktemp printed a path")?.trim());
    let parts: Vec<PathBuf> =
        ["projects", "shared", ".bp"].iter().map(|p| src.join(p)).filter(|p| p.exists()).collect();
    if !parts.iter().any(|p| p.ends_with("projects")) {
        bail!("{} holds no projects/: BP_VAULT names the vault root", src.display());
    }
    run_in(&dir, Command::new("cp").arg("-R").args(&parts).arg(&dir))?;
    // The copy's own identity, so a commit works where git has none configured, as on CI.
    run_in(&dir, Command::new("git").args(["init", "-q"]))?;
    run_in(&dir, Command::new("git").args(["config", "user.name", "bp scratch"]))?;
    run_in(&dir, Command::new("git").args(["config", "user.email", "scratch@bp.invalid"]))?;
    run_in(&dir, Command::new("git").args(["add", "-A"]))?;
    run_in(&dir, Command::new("git").args(["commit", "-q", "--no-verify", "-m", "scratch copy"]))?;
    Ok(dir)
}

/// Deletes all but the newest `KEEP - 1` copies, by the time each was made.
fn prune(parent: &Path) -> Result<()> {
    let mut copies: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(parent)?
        .filter_map(|e| {
            let e = e.ok()?;
            Some((e.metadata().ok()?.modified().ok()?, e.path()))
        })
        .collect();
    copies.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in copies.iter().skip(KEEP - 1) {
        // A copy another run deletes first is already gone, which is the goal.
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(())
}

fn run_in(dir: &Path, cmd: &mut Command) -> Result<()> {
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
