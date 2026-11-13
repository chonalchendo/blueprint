//! git in a code repository, as the repo verbs and the Change page run it: one call, its stdout, or a refusal in git's
//! own words.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use bp_vault::Refusal;

/// The variables that move where git reads: the repository, its index, its objects, its refs and its history.
const LOCATION: [&str; 13] = [
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
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_PREFIX",
];

/// How long one git call may run once a verb sets it; unset, a call runs as long as git takes. One process runs one
/// verb, so the verb's choice holds for each of its calls.
static DEADLINE: OnceLock<Duration> = OnceLock::new();

/// Kills each later git call of this process after `limit`, refused `could_not_run`: `land` sets it, so a fetch that
/// hangs refuses before the merge and changes nothing (review of #100, F11).
pub fn within(limit: Duration) {
    let _ = DEADLINE.set(limit);
}

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
    // A local `git replace` swaps an object under every read with no push: a base commit whose tree names another
    // signer, or drops a high path (review of #100, row S1). A `.git/info/grafts` line gives a commit other parents,
    // so a high head reads as the base's own tree (review of #99, row 55). bp reads the objects and the parents the
    // shas name.
    git.env("GIT_NO_REPLACE_OBJECTS", "1").env("GIT_GRAFT_FILE", "/dev/null");
    // A credential prompt would wait on a terminal no agent reads; the fetch fails instead.
    git.env("GIT_TERMINAL_PROMPT", "0");
    git
}

/// Runs git in the code repository and returns its stdout, trimmed. A failure is `could_not_run` with git's own words.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, Refusal> {
    run(dir, args, DEADLINE.get().copied())
}

fn run(dir: &Path, args: &[&str], limit: Option<Duration>) -> Result<String, Refusal> {
    let mut cmd = command(dir);
    cmd.args(args);
    let out = match limit {
        None => cmd.output(),
        Some(limit) => bp_forge::output_within(cmd.stdin(Stdio::null()), limit),
    }
    .map_err(|e| match e.kind() {
        std::io::ErrorKind::TimedOut => {
            Refusal::failed(format!("git {}", args.join(" ")), format!("{e}; check origin and the network, then rerun"))
        }
        _ => Refusal::failed("git", format!("{e}; install git")),
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deadline_kills_a_git_call_that_hangs() {
        let dir = std::env::temp_dir();
        let start = std::time::Instant::now();
        let e = run(&dir, &["-c", "alias.hang=!sleep 5", "hang"], Some(Duration::from_millis(300))).unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(4), "{:?}", start.elapsed());
        assert_eq!((e.exit, e.subject.as_str()), (1, "git -c alias.hang=!sleep 5 hang"));
        assert!(e.fix.starts_with("git did not answer within 300ms"), "{}", e.fix);
        assert!(run(&dir, &["--version"], Some(Duration::from_secs(60))).is_ok(), "a quick call is as it was");
    }
}
