//! What the write-verb tests share: a small vault under git, the binary run over it, and git to read the result.

use std::path::Path;
use std::process::Command as Std;

use assert_cmd::Command;

/// Runs git in `root`, asserts it succeeded, and returns its stdout.
pub fn git(root: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// A vault of `files` (path, text) and an ignored `.bp/`, in one commit.
pub fn vault(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, text) in files.iter().chain(&[(".bp/.gitignore", "*\n")]) {
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), text).unwrap();
    }
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@t"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "one"]);
    dir
}

/// `bp <args> --json` in `root`, with the clock fixed: the exit code and the parsed result.
pub fn bp(root: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    bp_env(root, &[], args)
}

/// [`bp`] with extra environment variables.
pub fn bp_env(root: &Path, env: &[(&str, &str)], args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(root)
        .env_remove("BP_VAULT")
        .env_remove("BP_ROLE")
        .env("BP_NOW", "2026-10-19T08:00:00Z")
        .envs(env.iter().copied())
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

/// The number of commits on HEAD.
pub fn commits(root: &Path) -> String {
    git(root, &["rev-list", "--count", "HEAD"]).trim().to_string()
}
