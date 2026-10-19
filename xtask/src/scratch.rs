//! `cargo xtask scratch`: a writable copy of the vault for a block that writes (25, Finish line). It copies
//! `projects`, `shared` and `.bp` of `$BP_VAULT` into a fresh `mktemp -d` directory, makes it a git repository with
//! one commit, and returns the directory. Nothing writes into `$BP_VAULT`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Copies the vault and commits the copy once. `.bp` comes too, so the copy's first read reuses the index's hashes
/// instead of parsing every file again.
pub fn run() -> Result<PathBuf> {
    let src = std::env::var_os("BP_VAULT").context("set BP_VAULT to the vault to copy")?;
    let src = Path::new(&src).canonicalize().with_context(|| format!("read BP_VAULT {}", src.display()))?;
    let out = Command::new("mktemp").arg("-d").output().context("run mktemp -d")?;
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
