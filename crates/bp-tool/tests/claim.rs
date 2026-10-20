//! `bp cut` and `bp release` through the binary: a vault, a bare origin and its clone, the clock fixed.

mod common;

use std::path::{Path, PathBuf};

use common::{commits, git};
use serde_json::Value;

const ISSUE: &str = "projects/bp/issues/a";

/// The vault, and a directory holding `origin.git` and its clone `code`, whose origin is the project's repository.
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let t = tempfile::tempdir().unwrap();
    let origin = t.path().join("origin.git");
    git(t.path(), &["init", "-q", "--bare", "-b", "master", "origin.git"]);
    git(t.path(), &["init", "-q", "-b", "master", "code"]);
    let code = t.path().join("code");
    git(&code, &["config", "user.name", "t"]);
    git(&code, &["config", "user.email", "t@t"]);
    std::fs::write(code.join("README.md"), "# bp\n").unwrap();
    git(&code, &["add", "README.md"]);
    git(&code, &["commit", "-qm", "one"]);
    git(&code, &["remote", "add", "origin", &origin.display().to_string()]);
    git(&code, &["push", "-q", "-u", "origin", "master"]);
    git(&code, &["remote", "set-head", "origin", "master"]);
    let issue = |status: &str| format!("---\nkind: issue\ntitle: A\nstatus: {status}\n---\n\nBody.\n");
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let v = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{ISSUE}.md"), &issue("open")),
        ("projects/bp/issues/r.md", &issue("resolved")),
    ]);
    (v, t)
}

/// `bp <args> --json` from the clone, with the vault named and `env` set.
fn bp(v: &Path, t: &Path, env: &[(&str, &str)], args: &[&str]) -> (Option<i32>, Value) {
    let vault = v.display().to_string();
    let env: Vec<(&str, &str)> = [("BP_VAULT", vault.as_str())].into_iter().chain(env.iter().copied()).collect();
    common::bp_env(&t.join("code"), &env, args)
}

fn worktree(t: &Path) -> PathBuf {
    t.canonicalize().unwrap().join("code-worktrees/a")
}

fn intent(v: &Path, now: &str) -> Value {
    let (_, out) = common::bp_env(v, &[("BP_NOW", now)], &["list", "issue", "--project", "bp"]);
    out["rows"].as_array().unwrap().iter().find(|r| r["id"] == ISSUE).unwrap()["intent"].clone()
}

fn last_message(v: &Path) -> String {
    git(v, &["log", "-1", "--format=%B"])
}

#[test]
fn cut_claims_in_one_commit_and_a_repeat_exits_3() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (code, out) = bp(v, t, &[("BP_SESSION", "s1")], &["cut", ISSUE]);
    assert_eq!(code, Some(0), "{out}");
    let w = worktree(t);
    assert_eq!((out["branch"].as_str(), out["worktree"].as_str()), (Some("bp/a"), w.to_str()));
    assert_eq!(git(&w, &["rev-parse", "HEAD"]), git(&t.join("code"), &["rev-parse", "origin/master"]));
    assert_eq!(git(&w, &["branch", "--show-current"]).trim(), "bp/a");
    let claim = out["claim"].as_str().unwrap();
    assert_eq!(claim, "projects/bp/issues/a/claims/2026-10-19T08-00-00Z");
    let message = last_message(v);
    for line in ["Bp-Verb: cut", "Bp-Role: human", &format!("Bp-Claim: {claim}")] {
        assert!(message.lines().any(|l| l == line), "{line} in {message}");
    }
    let (_, held) = common::bp(v, &["show", claim]);
    assert_eq!(
        [&held["from"], &held["owner"], &held["session"], &held["worktree"], &held["base"], &held["under"]],
        [&"2026-10-19T08:00:00Z".into(), &"human".into(), &"s1".into(), &out["worktree"], &out["base"], &ISSUE.into()]
    );
    assert!(std::fs::read_to_string(v.join(format!("{ISSUE}.md"))).unwrap().contains("status: in-progress"));
    assert_eq!(git(v, &["log", "-1", "--format=%cI"]).trim(), "2026-10-19T08:00:00Z", "the commit is the claim's time");
    let before = commits(v);
    let (code, out) = bp(v, t, &[("BP_SESSION", "s1")], &["cut", ISSUE]);
    assert_eq!((code, out["claim"].as_str()), (Some(3), Some(claim)));
    let (code, out) = bp(v, t, &[("BP_SESSION", "s2")], &["cut", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("already_claimed")), "another session");
    let (_, out) = bp(v, t, &[("BP_SESSION", "s1"), ("BP_ROLE", "worker")], &["cut", ISSUE]);
    assert_eq!(out["code"], "already_claimed", "another owner");
    assert_eq!(commits(v), before);
}

#[test]
fn cut_refuses_before_anything_moves() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (code, out) = bp(v, t, &[], &["cut", "projects/bp/issues/r"]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("illegal_transition")));
    std::fs::write(v.join(format!("{ISSUE}.md")), "---\nkind: issue\ntitle: A\nstatus: open\n---\n\nEdited.\n")
        .unwrap();
    let (code, out) = bp(v, t, &[], &["cut", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("path_dirty")));
    git(v, &["checkout", "-q", "--", "."]);
    let (code, out) = bp(v, t, &[], &["cut", ISSUE, "--base", "nope"]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("unknown_value")));
    git(t, &["init", "-q", "other"]);
    let (code, out) = common::bp_env(&t.join("other"), &[("BP_VAULT", v.to_str().unwrap())], &["cut", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "no clone of the project: {out}");
    assert_eq!(commits(v), "1");
    assert!(!worktree(t).exists());
}

#[test]
fn a_failed_commit_takes_the_worktree_and_the_branch_with_it() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let hook = v.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (code, out) = bp(v, t, &[], &["cut", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    assert!(!worktree(t).exists());
    assert_eq!(git(&t.join("code"), &["branch", "--list", "bp/a"]), "");
    assert_eq!(git(&t.join("code"), &["worktree", "list"]).lines().count(), 1);
    assert_eq!(git(v, &["status", "--porcelain"]), "");
}

#[test]
fn release_appends_the_release_and_keeps_the_worktree_for_the_next_cut() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let claim = cut["claim"].as_str().unwrap();
    let w = worktree(t);
    assert_eq!(intent(v, "2026-10-19T08:30:00Z"), "working");
    std::fs::write(w.join("work.txt"), "w\n").unwrap();
    git(&w, &["add", "work.txt"]);
    git(&w, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "work"]);
    let (code, _) = bp(v, t, &[], &["release", ISSUE, "--reason", "sideways"]);
    assert_eq!(code, Some(4));
    let later = [("BP_NOW", "2026-10-19T09:00:00Z")];
    let (code, out) = bp(v, t, &later, &["release", ISSUE, "--reason", "escalated"]);
    assert_eq!((code, out["status"].as_str()), (Some(0), Some("open")), "{out}");
    assert!(last_message(v).lines().any(|l| l == format!("Bp-Claim: {claim}")));
    assert!(last_message(v).lines().any(|l| l == "Bp-Verb: release"));
    let text = std::fs::read_to_string(v.join(format!("{claim}.md"))).unwrap();
    assert!(text.ends_with("---\n\n## Released\n\nto: 2026-10-19T09:00:00Z\nreason: escalated\n"), "{text}");
    assert!(std::fs::read_to_string(v.join(format!("{ISSUE}.md"))).unwrap().contains("status: open"));
    assert!(w.join("work.txt").exists());
    assert_eq!(intent(v, "2026-10-19T09:30:00Z"), "blocked", "escalated, and no write since: the commit is at `to`");
    let (code, out) = bp(v, t, &later, &["release", ISSUE, "--reason", "dropped"]);
    assert_eq!((code, out["id"].as_str()), (Some(3), Some(ISSUE)), "no active claim");
    let again = [("BP_NOW", "2026-10-19T10:00:00Z")];
    let (code, out) = bp(v, t, &again, &["cut", ISSUE]);
    assert_eq!((code, out["worktree"].as_str()), (Some(0), w.to_str()), "{out}");
    assert_eq!(out["base"], cut["base"], "the kept worktree's base is where its branch left master");
    assert_eq!(git(&t.join("code"), &["worktree", "list"]).lines().count(), 2);
    assert_eq!(std::fs::read_dir(v.join(format!("{ISSUE}/claims"))).unwrap().count(), 2);
}
