//! `bp cut` and `bp release` through the binary: a vault, a bare origin and its clone, the clock fixed.

mod common;

use std::path::{Path, PathBuf};

use common::{commits, git};
use serde_json::Value;

const ISSUE: &str = "projects/bp/issues/a";

/// An issue that depends on [`ISSUE`] once linked.
const B: &str = "projects/bp/issues/b";

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
        ("projects/bp/issues/b.md", &issue("open")),
        ("projects/bp/issues/x.md", &issue("open")),
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
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("no_clone")), "no clone of the project: {out}");
    let origin = t.join("origin.git").display().to_string();
    assert!(out["fix"].as_str().unwrap().contains(&origin), "the fix names the repository: {out}");
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
    assert_eq!(intent(v, "2026-10-19T09:30:00Z"), "blocked", "escalated, and no write since");
    let (code, out) = bp(v, t, &later, &["release", ISSUE, "--reason", "dropped"]);
    assert_eq!((code, out["id"].as_str()), (Some(3), Some(ISSUE)), "no active claim");
    let again = [("BP_NOW", "2026-10-19T10:00:00Z")];
    let (code, out) = bp(v, t, &again, &["cut", ISSUE]);
    assert_eq!((code, out["worktree"].as_str()), (Some(0), w.to_str()), "{out}");
    assert_eq!(out["base"], cut["base"], "the kept worktree's base is where its branch left master");
    assert_eq!(git(&t.join("code"), &["worktree", "list"]).lines().count(), 2);
    assert_eq!(std::fs::read_dir(v.join(format!("{ISSUE}/claims"))).unwrap().count(), 2);
}

#[test]
fn an_answer_in_the_release_second_ends_the_escalation() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    bp(v, t, &[], &["cut", ISSUE]);
    // Both commits in one second: commit order, not time, says the answer came after.
    let second = [("GIT_COMMITTER_DATE", "@1792400000 +0000"), ("GIT_AUTHOR_DATE", "@1792400000 +0000")];
    let (code, _) = bp(v, t, &second, &["release", ISSUE, "--reason", "escalated"]);
    assert_eq!(code, Some(0));
    assert_eq!(intent(v, "2026-10-19T09:00:00Z"), "blocked");
    let (code, _) = bp(v, t, &second, &["set", ISSUE, "severity", "low"]);
    assert_eq!(code, Some(0));
    assert_eq!(git(v, &["log", "-2", "--format=%ct"]), "1792400000\n1792400000\n");
    assert_eq!(intent(v, "2026-10-19T09:00:00Z"), "ready");
}

#[test]
fn cut_fetches_with_no_lock_held() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    git(&t.join("code"), &["config", "remote.origin.uploadpack", "sleep 3; git-upload-pack"]);
    let mut slow = std::process::Command::new(env!("CARGO_BIN_EXE_bp"))
        .current_dir(t.join("code"))
        .env("BP_VAULT", v)
        .env("BP_NOW", "2026-10-19T08:00:00Z")
        .env_remove("BP_ROLE")
        .args(["cut", ISSUE, "--json"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let (code, out) = bp(v, t, &[], &["set", "projects/bp/issues/b", "severity", "low"]);
    assert_eq!(code, Some(0), "another writer goes on while cut fetches: {out}");
    assert!(slow.try_wait().unwrap().is_none(), "the fetch still runs");
    assert!(slow.wait().unwrap().success());
    assert!(std::fs::read_to_string(v.join(format!("{ISSUE}.md"))).unwrap().contains("status: in-progress"));
}

#[test]
fn cut_from_a_linked_worktree_places_the_next_beside_it() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    bp(v, t, &[], &["cut", ISSUE]);
    let env = [("BP_VAULT", v.to_str().unwrap())];
    let (code, out) = common::bp_env(&worktree(t), &env, &["cut", "projects/bp/issues/b"]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(out["worktree"].as_str(), t.canonicalize().unwrap().join("code-worktrees/b").to_str());
}

#[test]
fn cut_restores_a_removed_worktree_from_its_branch() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let code_dir = t.join("code");
    let (_, first) = bp(v, t, &[], &["cut", ISSUE]);
    let w = worktree(t);
    git(&code_dir, &["worktree", "remove", "--force", w.to_str().unwrap()]);
    let (code, out) = bp(v, t, &[], &["cut", ISSUE]);
    assert_eq!((code, out["worktree"].as_str()), (Some(3), w.to_str()), "a repeat: {out}");
    assert_eq!(git(&w, &["branch", "--show-current"]).trim(), "bp/a", "the repeat put the worktree back");
    bp(v, t, &[("BP_NOW", "2026-10-19T09:00:00Z")], &["release", ISSUE, "--reason", "dropped"]);
    std::fs::remove_dir_all(&w).unwrap();
    let (code, out) = bp(v, t, &[("BP_NOW", "2026-10-19T10:00:00Z")], &["cut", ISSUE]);
    assert_eq!((code, out["worktree"].as_str()), (Some(0), w.to_str()), "a kept branch: {out}");
    assert_eq!(out["base"], first["base"]);
    assert_eq!(git(&w, &["branch", "--show-current"]).trim(), "bp/a");
}

/// A `gh` first on `PATH` whose `pr list --head <project>/<slug>` prints `prs-<slug>.json`, else no pull request.
fn fake_gh(t: &Path) -> String {
    let bin = t.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let fake = format!(
        "#!/bin/sh\n[ \"$1 $2\" = 'pr list' ] && {{ cat \"{}/prs-$(basename \"$4\").json\" 2>/dev/null || echo '[]'; }}\n",
        t.display()
    );
    std::fs::write(bin.join("gh"), fake).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

/// An open pull request of `<project>/<slug>` at `head` into master, as the fake prints it.
fn open_pr(t: &Path, slug: &str, head: &str) {
    let pr = serde_json::json!([{ "number": 1, "state": "OPEN", "headRefName": format!("bp/{slug}"), "headRefOid": head, "baseRefName": "master" }]);
    std::fs::write(t.join(format!("prs-{slug}.json")), pr.to_string()).unwrap();
}

#[test]
fn cut_refuses_dependencies_in_two_stacks() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let path = fake_gh(t);
    let env = [("PATH", path.as_str())];
    for d in [ISSUE, "projects/bp/issues/x"] {
        assert_eq!(common::bp(v, &["link", B, "depends", d]).0, Some(0));
    }
    open_pr(t, "a", "h");
    open_pr(t, "x", "h");
    let (code, out) = bp(v, t, &env, &["cut", B]);
    assert_eq!((code, out["code"].as_str(), out["subject"].as_str()), (Some(2), Some("dependency_split"), Some(B)));
    assert!(out["fix"].as_str().unwrap().ends_with("land all but one, then rerun"), "{out}");
    assert_eq!(git(&t.join("code"), &["branch", "--list", "bp/b"]), "", "a refusal adds no branch");
}

#[test]
fn cut_stacks_on_an_open_dependency_and_refuses_an_unpushed_one() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let path = fake_gh(t);
    let env = [("PATH", path.as_str())];
    assert_eq!(common::bp(v, &["link", B, "depends", ISSUE]).0, Some(0));
    let (code, out) = bp(v, t, &env, &["cut", B]);
    assert_eq!(
        (code, out["code"].as_str(), out["subject"].as_str()),
        (Some(2), Some("dependency_unpushed"), Some(ISSUE))
    );
    assert_eq!(git(&t.join("code"), &["branch", "--list", "bp/b"]), "", "a refusal adds no branch");

    let (_, a) = bp(v, t, &env, &["cut", ISSUE]);
    let w = Path::new(a["worktree"].as_str().unwrap());
    std::fs::write(w.join("a.txt"), "a\n").unwrap();
    git(w, &["add", "a.txt"]);
    git(w, &["commit", "-qm", "a"]);
    git(w, &["push", "-q", "origin", "HEAD:bp/a"]);
    let head = git(w, &["rev-parse", "HEAD"]).trim().to_string();
    open_pr(t, "a", &head);
    let (code, out) = bp(v, t, &env, &["cut", B]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(
        (out["base_issue"].as_str(), out["base_branch"].as_str(), out["base"].as_str()),
        (Some(ISSUE), Some("bp/a"), Some(head.as_str()))
    );
    let wb = Path::new(out["worktree"].as_str().unwrap());
    assert_eq!(git(wb, &["rev-parse", "HEAD"]).trim(), head, "b starts at a's pull request head");
    let (_, claim) = common::bp(v, &["show", out["claim"].as_str().unwrap()]);
    assert_eq!(claim["base_issue"], ISSUE);
}

#[test]
fn a_refusal_before_the_forge_calls_no_gh_and_a_failed_refresh_says_so() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    // A `gh` first on `PATH` that logs each call and fails, as offline.
    let bin = t.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let log = t.join("calls");
    std::fs::write(bin.join("gh"), format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", log.display())).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let cut = |issue: &str| {
        let out = assert_cmd::Command::cargo_bin("bp")
            .unwrap()
            .current_dir(t.join("code"))
            .env("BP_VAULT", v)
            .env_remove("BP_ROLE")
            .env("BP_NOW", "2026-10-19T08:00:00Z")
            .env("PATH", &path)
            .args(["cut", issue, "--json"])
            .output()
            .unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code(), v, String::from_utf8(out.stderr).unwrap())
    };
    for (issue, code) in
        [("projects/bp/issues/no-such-quokka", "no_node"), ("projects/bp/issues/r", "illegal_transition")]
    {
        let (exit, out, err) = cut(issue);
        assert_eq!((exit, out["code"].as_str(), err.as_str()), (Some(2), Some(code), ""), "{out}");
        assert!(!log.exists(), "a refusal before any forge call makes no gh call");
    }
    assert!(!v.join(".bp/forge/bp").exists());
    // The cut writes, so it refreshes; the refresh fails, and the cut's result and exit stand.
    let (exit, out, err) = cut(ISSUE);
    assert_eq!((exit, out["status"].as_str()), (Some(0), Some("in-progress")), "{out}");
    assert!(err.starts_with("snapshot of bp not refreshed: gh pr list --state open"), "{err}");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(log.exists() && !v.join(".bp/forge/bp/open-prs.json").exists());
}

#[test]
fn a_read_verb_refreshes_a_stale_snapshot_once_and_names_a_failed_refresh() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    // A `gh` that logs each call, answers an empty open list, and fails with two lines of stderr while `fail` exists.
    let bin = t.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let (log, fail) = (t.join("calls"), t.join("fail"));
    let fake = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\n[ -e '{}' ] && {{ echo one >&2; echo two >&2; exit 1; }}\necho '[]'\n",
        log.display(),
        fail.display()
    );
    std::fs::write(bin.join("gh"), fake).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let calls = || std::fs::read_to_string(&log).map_or(0, |s| s.lines().count());
    let list = |env: &[(&str, &str)]| {
        let env: Vec<(&str, &str)> = [("PATH", path.as_str())].into_iter().chain(env.iter().copied()).collect();
        bp(v, t, &env, &["list", "issue", "--project", "bp"])
    };
    let open = v.join(".bp/forge/bp/open-prs.json");
    let age = |secs: u64| {
        let then = std::time::SystemTime::now() - std::time::Duration::from_secs(secs);
        std::fs::File::options().write(true).open(&open).unwrap().set_modified(then).unwrap();
    };
    // `BP_NOW` is fixed in the past, so `forge_age` reads 0; the file's own age shows the refresh.
    let secs = || std::fs::metadata(&open).unwrap().modified().unwrap().elapsed().unwrap().as_secs();

    // No snapshot yet: a read in the clone, with `gh` on `PATH`, calls nothing and writes nothing.
    let (code, out) = list(&[]);
    assert_eq!((code, out["forge"].as_str(), calls()), (Some(0), Some("not read"), 0), "{out}");
    assert!(out.get("forge_error").is_none() && !v.join(".bp/forge").exists(), "{out}");
    let (code, _) = bp(v, t, &[("PATH", path.as_str())], &["walk", ISSUE, "--for", "complete", "--done"]);
    assert_eq!((code, calls(), v.join(".bp/forge").exists()), (Some(0), 0, false));
    // A repo verb writes the snapshot; a read then reads it.
    let (code, _) = bp(v, t, &[("PATH", path.as_str())], &["cut", ISSUE]);
    assert_eq!((code, calls()), (Some(0), 1));
    let (code, out) = list(&[]);
    assert_eq!((code, out["forge"].as_str(), calls()), (Some(0), Some("snapshot"), 1), "{out}");
    assert!(out.get("forge_error").is_none(), "{out}");
    // Under a minute old: no call.
    assert_eq!((list(&[]).0, calls()), (Some(0), 1));
    // Three minutes old: one call, and the snapshot is fresh again.
    age(180);
    assert_eq!((list(&[]).0, calls(), secs() < 60), (Some(0), 2, true));
    // Dated after now (a skewed clock): stale, so one call, which rewrites the time.
    let ahead = std::time::SystemTime::now() + std::time::Duration::from_secs(86_400);
    std::fs::File::options().write(true).open(&open).unwrap().set_modified(ahead).unwrap();
    assert_eq!((list(&[]).0, calls(), secs() < 60), (Some(0), 3, true));
    // `BP_FORGE_SNAPSHOT` set, a read outside the clone, or no `gh` on `PATH` stays offline and names no error.
    age(180);
    let over = v.join(".bp/forge/bp").display().to_string();
    assert_eq!((list(&[("BP_FORGE_SNAPSHOT", &over)]).0, calls()), (Some(0), 3));
    let (code, _) = common::bp_env(v, &[("PATH", path.as_str())], &["list", "issue", "--project", "bp"]);
    assert_eq!((code, calls()), (Some(0), 3), "the vault is no clone");
    let (code, out) = bp(v, t, &[("PATH", "/usr/bin:/bin")], &["list", "issue", "--project", "bp"]);
    assert_eq!((code, calls(), out.get("forge_error")), (Some(0), 3, None), "no gh: {out}");
    // A failed refresh keeps the stale snapshot and names the error's first line; the read still answers.
    std::fs::write(&fail, "").unwrap();
    let (code, out) = list(&[]);
    assert_eq!((code, out["forge"].as_str(), calls()), (Some(0), Some("snapshot"), 4), "{out}");
    assert!(secs() >= 180, "the stale snapshot stays");
    let error = out["forge_error"].as_str().unwrap_or_default();
    assert!(error.starts_with("bp: gh exited") && error.ends_with("one"), "{out}");
    assert!(error.chars().count() <= 200 && !error.contains("two"), "{out}");
    // `walk` refreshes its seed's project too, and carries the error.
    let (code, out) = bp(v, t, &[("PATH", path.as_str())], &["walk", ISSUE, "--for", "complete", "--done"]);
    assert_eq!((code, calls()), (Some(0), 5), "{out}");
    assert!(out["forge_error"].as_str().is_some_and(|s| s == error), "{out}");
}
