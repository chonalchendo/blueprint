//! git in a code repository, as the repo verbs and the Change page run it: one call, its stdout, or a refusal in git's
//! own words.

use std::path::Path;
use std::process::Command;

use bp_vault::Refusal;

/// Runs git in the code repository and returns its stdout, trimmed. A failure is `could_not_run` with git's own words.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, Refusal> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("LC_ALL", "C")
        // A credential prompt would wait on a terminal no agent reads; the fetch fails instead.
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
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
