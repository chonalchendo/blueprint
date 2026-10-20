//! `cargo xtask forge <project>`: a fake forge for a repo block (26, issue 1). It prints a fresh directory `T`:
//! `T/origin.git`, a bare repository with one commit on `master`; `T/code`, its clone with its own git identity;
//! `T/bin/gh`, the fake (`gh.rs`); and `T/forge`, a snapshot directory of 24 that also holds the fake's state.
//! `T/code`'s `origin` url is the project's `repository`, so `bp where` resolves it, and an `insteadOf` rule sends
//! fetch to `T/origin.git` instead. An explicit `pushurl` sends push there too: git ignores a global `pushInsteadOf`
//! for a remote that has one, which would otherwise send the push to the real repository. A block still sets
//! `GIT_ALLOW_PROTOCOL=file`, so a url both miss fails before it reaches the network.
//!
//! The shim runs the `xtask` binary that made the forge, by path. A later `cargo xtask` built from another commit of
//! the same worktree replaces that binary under every live forge; a block builds and runs at one commit, so it sees
//! one fake.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::scratch;

/// Forges kept, the new one included. One is a few hundred KB, worktrees included.
const KEEP: usize = 16;

/// Makes the forge. `$BP_VAULT` names the vault that holds the project.
pub fn run(args: &[String]) -> Result<PathBuf> {
    let [project] = args else { bail!("usage: cargo xtask forge <project>") };
    let vault = std::env::var_os("BP_VAULT").context("set BP_VAULT to the vault that holds the project")?;
    let file = Path::new(&vault).join("projects").join(project).join("project.md");
    let text = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    let repository = repository(&text).with_context(|| format!("{} names no repository", file.display()))?;
    // Canonical, so a path a verb derives from its cwd equals one a block builds from `T`: macOS's `/var` is a link.
    let t = scratch::fresh(&std::env::temp_dir().join("bp-forge"), KEEP)?.canonicalize()?;
    make(&t, project, repository)?;
    Ok(t)
}

/// Fills the empty directory `t` with the forge of `project`, whose clone's `origin` url is `repository`.
pub(crate) fn make(t: &Path, project: &str, repository: &str) -> Result<()> {
    let (origin, code) = (t.join("origin.git"), t.join("code"));
    git(t, &["init", "-q", "--bare", "-b", "master", "origin.git"])?;
    git(t, &["init", "-q", "-b", "master", "code"])?;
    git(&code, &["config", "user.name", "bp forge"])?;
    git(&code, &["config", "user.email", "forge@bp.invalid"])?;
    git(&code, &["config", &format!("url.{}.insteadOf", origin.display()), repository])?;
    git(&code, &["remote", "add", "origin", repository])?;
    git(&code, &["config", "remote.origin.pushurl", &origin.display().to_string()])?;
    std::fs::write(code.join("README.md"), format!("# {project}\n"))?;
    git(&code, &["add", "README.md"])?;
    git(&code, &["-c", "gc.auto=0", "commit", "-q", "--no-verify", "-m", "one commit"])?;
    git(&code, &["push", "-q", "--no-verify", "-u", "origin", "master"])?;
    git(&code, &["remote", "set-head", "origin", "master"])?;

    // The fake is this binary behind a shim. It edits JSON with serde_json, where a shell script would need jq,
    // which CI does not install.
    let bin = t.join("bin");
    std::fs::create_dir(&bin)?;
    let exe = std::env::current_exe().context("find the xtask binary")?;
    let shim = format!("#!/bin/sh\nexec '{}' gh '{}' \"$@\"\n", exe.display(), t.display());
    std::fs::write(bin.join("gh"), shim)?;
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    std::fs::create_dir_all(t.join("forge/checks"))?;
    std::fs::write(t.join("forge/open-prs.json"), "[]\n")?;
    Ok(())
}

/// The `repository` field of a project file's header.
fn repository(text: &str) -> Option<&str> {
    let header = text.strip_prefix("---\n")?.split("\n---").next()?;
    let line = header.lines().find_map(|l| l.strip_prefix("repository:"))?;
    Some(line.trim().trim_matches(['"', '\'']).trim()).filter(|r| !r.is_empty())
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<()> {
    // `file` alone: a url the `insteadOf` rule misses fails here instead of reaching the network.
    scratch::run_in(dir, Command::new("git").args(args).env("GIT_ALLOW_PROTOCOL", "file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_repository_from_the_header() {
        let file = "---\nkind: project\nrepository: \"https://h/o/r\"\n---\n\nrepository: body\n";
        assert_eq!(repository(file), Some("https://h/o/r"));
        assert_eq!(repository("---\nkind: project\n---\nrepository: https://h/o/r\n"), None, "the body is no header");
    }

    #[test]
    fn a_global_push_rule_cannot_send_the_push_to_the_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().canonicalize().unwrap();
        make(&t, "bp", "https://github.invalid/o/r").unwrap();
        let global = t.join("gitconfig");
        std::fs::write(&global, "[url \"git@github.invalid:\"]\n\tpushInsteadOf = https://github.invalid/\n").unwrap();
        let out = Command::new("git")
            .args(["remote", "get-url", "--push", "origin"])
            .current_dir(t.join("code"))
            .env("GIT_CONFIG_GLOBAL", &global)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), t.join("origin.git").display().to_string());
    }
}
