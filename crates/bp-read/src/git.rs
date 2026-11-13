//! git in a code repository, as the repo verbs and the Change page run it: one call, its stdout, or a refusal in git's
//! own words.

use std::path::Path;
use std::process::Command;

use bp_vault::Refusal;

/// The variables that move where git reads: the repository, its index, its objects, its refs and its history.
const LOCATION: [&str; 14] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_SHALLOW_FILE",
    "GIT_GRAFT_FILE",
    "GIT_REPLACE_REF_BASE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_PREFIX",
];

/// git in the code repository at `dir`, with no argument yet. Every git call on a code repository starts here.
pub fn command(dir: &Path) -> Command {
    let mut git = Command::new("git");
    git.arg("-C").arg(dir).env("LC_ALL", "C");
    // A git hook that runs bp, or the caller's shell, may set a location variable. Each can turn `-C <dir>` to
    // another repository, index or object store, or hide history (`GIT_SHALLOW_FILE`, `GIT_GRAFT_FILE`) from the
    // first-commit read of a review (review of #98, row 25). The transport variables (`GIT_ALLOW_PROTOCOL`,
    // `GIT_SSH_COMMAND`, `GIT_ASKPASS`, `GIT_SSL_*`, `GIT_CONFIG_*`) pass: a sandbox's protocol guard and a user's
    // key or token reach bp's fetch and push (row 31).
    for name in LOCATION {
        git.env_remove(name);
    }
    // A credential prompt would wait on a terminal no agent reads; the fetch fails instead.
    git.env("GIT_TERMINAL_PROMPT", "0");
    git
}

/// Runs git in the code repository and returns its stdout, trimmed. A failure is `could_not_run` with git's own words.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, Refusal> {
    let out = command(dir).args(args).output().map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    Err(Refusal::failed(
        format!("git {}", args.join(" ")),
        format!("{}; fix it in {}, then rerun", String::from_utf8_lossy(&out.stderr).trim(), dir.display()),
    ))
}

/// Origin's default branch: the base of a claim whose `--base` names no origin branch, and the base `land` tests for a
/// claim that records none.
pub fn default_branch(top: &Path) -> Result<String, Refusal> {
    let head = git(top, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).map_err(|e| {
        Refusal::failed(e.subject, "origin has no default branch here: git remote set-head origin --auto")
    })?;
    Ok(head.strip_prefix("origin/").unwrap_or(&head).to_string())
}
