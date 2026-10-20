//! `bp pr` and `bp land` through the binary: a vault, a bare origin and its clone, and a `gh` that answers from a
//! file the test writes, so each refusal of `land` is set up by hand.

mod common;

use std::path::Path;

use common::{commits, git};
use serde_json::{Value, json};

const ISSUE: &str = "projects/bp/issues/a";

/// The vault, and a directory holding `origin.git`, its clone `code` and the fake `bin/gh`. The fake answers
/// `api user` with `$FAKE_LOGIN`, `pr list` from `prs.json`, `pr create` with a url, and logs `pr merge`.
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
    let bin = t.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let fake = format!(
        r#"#!/bin/sh
case "$1 $2" in
  "api user") echo "{{\"login\":\"${{FAKE_LOGIN:-agent}}\"}}" ;;
  "pr list") cat '{t}/prs.json' 2>/dev/null || echo '[]' ;;
  "pr create") echo "$*" > '{t}/created'; echo 'https://forge.invalid/o/r/pull/1' ;;
  "pr merge") echo "$*" > '{t}/merged' ;;
  *) exit 9 ;;
esac
"#,
        t = t.path().display()
    );
    std::fs::write(bin.join("gh"), fake).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let issue = "---\nkind: issue\ntitle: A\nstatus: open\n---\n\nBody.\n\n## Check\n\n```check shell {name=hello expect=0}\ntest -f hello.txt\n```\n";
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let v = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{ISSUE}.md"), issue),
    ]);
    (v, t)
}

/// `bp <args> --json` from the clone, with the vault named and the fake first on `PATH`.
fn bp(v: &Path, t: &Path, env: &[(&str, &str)], args: &[&str]) -> (Option<i32>, Value) {
    let path = format!("{}:{}", t.join("bin").display(), std::env::var("PATH").unwrap());
    let vault = v.display().to_string();
    let base = [("BP_VAULT", vault.as_str()), ("PATH", path.as_str())];
    common::bp_env(&t.join("code"), &base.into_iter().chain(env.iter().copied()).collect::<Vec<_>>(), args)
}

/// Pull request 1 at `head`, with `reviews` by (login, head) and `checks` conclusions, as `gh pr list` prints it.
fn forge(t: &Path, head: &str, reviews: &[(&str, &str)], checks: &[&str]) {
    let reviews: Vec<Value> = reviews
        .iter()
        .map(|(login, at)| json!({ "state": "APPROVED", "author": { "login": login }, "commit": { "oid": at } }))
        .collect();
    let checks: Vec<Value> = checks.iter().map(|c| json!({ "conclusion": c })).collect();
    let pr = json!([{
        "number": 1, "state": "OPEN", "author": { "login": "agent" }, "baseRefName": "master",
        "headRefName": "bp/a", "headRefOid": head, "reviews": reviews, "statusCheckRollup": checks,
    }]);
    std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();
}

fn commit(w: &Path, args: &[&str], message: &str) -> String {
    git(w, args);
    git(w, &["commit", "-qm", message]);
    git(w, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn pr_then_land_refuses_in_order_and_lands_at_a_passing_head() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("illegal_transition")), "no claim: {out}");
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "hello.txt"], "hello");

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["pull_request"], out["head"].as_str()), (Some(0), &json!(1), Some(h.as_str())), "{out}");
    assert_eq!(out["runs"][0]["verdict"], "pass");
    let created = std::fs::read_to_string(t.join("created")).unwrap();
    assert!(created.starts_with("pr create --head bp/a --base master --title A"), "{created}");
    assert_eq!(git(&t.join("origin.git"), &["rev-parse", "bp/a"]).trim(), h, "the head is pushed");
    assert!(git(v, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: pr"));
    forge(t, &h, &[("agent", &h)], &[]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "a head with a pull request and runs");

    let before = commits(v);
    let land = || bp(v, t, &[], &["land", ISSUE]).1["code"].clone();
    assert_eq!(land(), "reviewer_invalid", "no reviewer");
    let (code, _) = common::bp(v, &["set", "projects/bp/project", "reviewer", "agent"]);
    assert_eq!(code, Some(0));
    assert_eq!(land(), "reviewer_invalid", "the token's own login");
    common::bp(v, &["set", "projects/bp/project", "reviewer", "r"]);
    assert_eq!(land(), "review_missing", "the author's approval does not count");
    forge(t, &h, &[("r", "old")], &["SUCCESS"]);
    assert_eq!(land(), "review_missing", "an approval of another head does not count");
    forge(t, &h, &[("r", &h)], &[]);
    assert_eq!(land(), "checks_pending");
    let bye = commit(&w, &["rm", "-q", "hello.txt"], "bye");
    forge(t, &bye, &[("r", &bye)], &["SUCCESS"]);
    assert_eq!(land(), "evidence_stale", "no run at the head");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str(), &out["pull_request"]), (Some(2), Some("check_failed"), &json!(1)));
    let (code, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_failed")), "{out}");
    assert!(out["subject"].as_str().unwrap().starts_with(&format!("{ISSUE}/runs/hello-{bye}")), "{out}");
    assert!(!t.join("merged").exists(), "no refusal merges");
    assert_eq!(commits(v).parse::<u32>().unwrap(), before.parse::<u32>().unwrap() + 3, "two sets and one pr");

    git(&w, &["revert", "--no-edit", "HEAD"]);
    let back = git(&w, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).1["runs"][0]["verdict"], "pass");
    forge(t, &back, &[("r", &back)], &["SUCCESS", "SKIPPED"]);
    let (code, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((code, out["status"].as_str()), (Some(0), Some("resolved")), "{out}");
    let merged = std::fs::read_to_string(t.join("merged")).unwrap();
    assert_eq!(merged.trim(), format!("pr merge 1 --squash --match-head-commit {back}"));
    assert!(!w.exists());
    assert_eq!(git(&t.join("code"), &["branch", "--list", "bp/a"]), "");
    assert_eq!(git(&t.join("origin.git"), &["branch", "--list", "bp/a"]), "", "the forge's branch goes too");
    let message = git(v, &["log", "-1", "--format=%B"]);
    let claim = cut["claim"].as_str().unwrap();
    for line in ["Bp-Verb: land", &format!("Bp-Claim: {claim}")] {
        assert!(message.lines().any(|l| l == line), "{line} in {message}");
    }
    let released = std::fs::read_to_string(v.join(format!("{claim}.md"))).unwrap();
    assert!(
        released.ends_with("## Released\n\nto: 2026-10-19T08:00:00Z\nreason: landed\npull_request: 1\n"),
        "{released}"
    );
    let (_, issue) = common::bp(v, &["show", ISSUE]);
    assert_eq!(issue["status"], "resolved");
    assert_eq!(bp(v, t, &[], &["land", ISSUE]).0, Some(3), "a resolved issue");
    assert_eq!(git(v, &["status", "--porcelain"]), "");
}
