//! `bp pr` and `bp land` through the binary: a vault, a bare origin and its clone, and a `gh` that answers from a
//! file the test writes, so each refusal of `land` is set up by hand.

mod common;

use std::path::Path;

use common::{commits, git};
use serde_json::{Value, json};

const ISSUE: &str = "projects/bp/issues/a";

const BARE: &str = "projects/bp/issues/bare";

const M: &str = "projects/bp/milestones/m";

const S: &str = "projects/bp/designs/system";

/// The vault, and a directory holding `origin.git`, its clone `code` and the fake `bin/gh`. The fake answers
/// `api user` with `$FAKE_LOGIN`, `pr list` from `prs.json`, `pr view` with its one row, `pr create` with a url, and
/// logs `pr edit` and `pr merge`. It keeps the body in `body` and prints it as JSON with CRLF line ends, as a forge may
/// store it, escaping only those: a body holds no quote. The vault holds [`ISSUE`], with one block, under [`M`], with
/// none, and [`BARE`], with none.
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
  "pr create") echo "$*" > '{t}/created'; shift 9; printf '%s' "$1" > '{t}/body'; echo 'https://forge.invalid/o/r/pull/1' ;;
  "pr edit") if [ "$4" = --body ]; then printf '%s' "$5" > '{t}/body'; else echo "$*" > '{t}/edited'; fi ;;
  "pr view") if [ "$5" = body ]; then printf '{{"body":"'; awk 'BEGIN {{ ORS = "\\r\\n" }} {{ print }}' '{t}/body'; printf '"}}'; else sed 's/^\[//; s/\]$//' '{t}/prs.json'; fi ;;
  "pr merge") echo "$*" > '{t}/merged' ;;
  *) exit 9 ;;
esac
"#,
        t = t.path().display()
    );
    std::fs::write(bin.join("gh"), fake).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let issue = "---\nkind: issue\ntitle: A\nstatus: open\nunder: projects/bp/milestones/m\n---\n\nBody.\n\n## Check\n\n```check shell {name=hello expect=0}\ntest -f hello.txt\n```\n";
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let v = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{ISSUE}.md"), issue),
        (&format!("{BARE}.md"), "---\nkind: issue\ntitle: Bare\nstatus: open\n---\n\nNo block.\n"),
        (&format!("{M}.md"), "---\nkind: milestone\ntitle: M\nstatus: approved\n---\n\n## Finish line\n"),
        (&format!("{S}.md"), "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n"),
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
    forge_as(t, head, reviews, checks, &json!({}));
}

/// [`forge`], with `over` replacing fields: a base or a state.
fn forge_as(t: &Path, head: &str, reviews: &[(&str, &str)], checks: &[&str], over: &Value) {
    let reviews: Vec<Value> = reviews
        .iter()
        .map(|(login, at)| json!({ "state": "APPROVED", "author": { "login": login }, "commit": { "oid": at } }))
        .collect();
    let checks: Vec<Value> = checks.iter().map(|c| json!({ "conclusion": c })).collect();
    let mut pr = json!({
        "number": 1, "state": "OPEN", "author": { "login": "agent" }, "baseRefName": "master",
        "headRefName": "bp/a", "headRefOid": head, "reviews": reviews, "statusCheckRollup": checks,
    });
    pr.as_object_mut().unwrap().extend(over.as_object().unwrap().clone());
    std::fs::write(t.join("prs.json"), json!([pr]).to_string()).unwrap();
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
    // The repeat refreshed the snapshot, and `list` reads it with no `gh` on its `PATH` (27, design change 9).
    assert!(v.join(".bp/forge/bp/open-prs.json").is_file() && v.join(".bp/forge/bp/pr-1.json").is_file());
    let (_, list) = common::bp_env(v, &[("PATH", "/usr/bin:/bin")], &["list", "issue"]);
    assert!(list["forge"] == "snapshot" && list["forge_age"].is_i64(), "{list}");
    assert_eq!(list["rows"].as_array().unwrap().iter().find(|r| r["id"] == ISSUE).unwrap()["intent"], "needs-review");

    let before = commits(v);
    let land = || bp(v, t, &[], &["land", ISSUE]).1["code"].clone();
    assert_eq!(land(), "reviewer_invalid", "no reviewer");
    let (code, _) = common::bp(v, &["set", "projects/bp/project", "reviewer", "agent"]);
    assert_eq!(code, Some(0));
    assert_eq!(land(), "reviewer_invalid", "the token's own login");
    common::bp(v, &["set", "projects/bp/project", "reviewer", "r"]);
    assert_eq!(land(), "review_missing", "the author's approval does not count");
    forge(t, &h, &[("r", "old")], &["SUCCESS"]);
    let (_, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!(out["code"], "review_missing", "an approval of an earlier head never counts");
    assert!(out["fix"].as_str().unwrap().starts_with("r approved old, which this clone cannot compare"), "{out}");
    forge(t, &h, &[("r", &h)], &[]);
    assert_eq!(land(), "checks_pending");
    let bye = commit(&w, &["rm", "-q", "hello.txt"], "bye");
    git(&w, &["push", "-q", "origin", "HEAD:bp/a"]);
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
    // `back` holds the diff the reviewer approved at `h`, but the forge needs an approval of the latest push.
    forge(t, &back, &[("r", &h), ("r", &bye)], &["SUCCESS", "SKIPPED"]);
    assert_eq!(land(), "review_missing", "an approval of an empty diff compares nothing");
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
    assert!(!v.join(".bp/forge/bp/pr-1.json").exists(), "no claim holds pull request 1 now");
    assert_eq!(git(v, &["status", "--porcelain"]), "");
}

/// `land --dry-run` gives the refusal `land` gives, envelope for envelope, at each gate it reaches; at a passing head
/// it names the head it would merge, and fetches, merges, commits and refreshes nothing (32, design change 5).
#[test]
fn a_dry_run_refuses_as_land_does_and_writes_nothing() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let dry = || bp(v, t, &[], &["land", ISSUE, "--dry-run"]);
    let both = |why: &str| {
        let (d, l) = (dry(), bp(v, t, &[], &["land", ISSUE]));
        assert_eq!(d, l, "{why}");
        d.1["code"].as_str().unwrap().to_string()
    };
    assert_eq!(both("no claim"), "illegal_transition");
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "hello.txt"], "hello");
    assert_eq!(both("no reviewer"), "reviewer_invalid");
    common::bp(v, &["set", "projects/bp/project", "reviewer", "r"]);
    assert_eq!(both("no pull request"), "review_missing");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    forge(t, &h, &[], &["SUCCESS"]);
    assert_eq!(both("no approval"), "review_missing");
    forge(t, &h, &[("r", &h)], &[]);
    assert_eq!(both("no check"), "checks_pending");
    forge(t, &h, &[("r", &h)], &["SUCCESS"]);
    std::fs::remove_dir_all(v.join(".bp/forge")).unwrap();
    let fetched = || std::fs::metadata(t.join("code/.git/FETCH_HEAD")).and_then(|m| m.modified()).unwrap();
    let (before, at) = (commits(v), fetched());
    let (code, out) = dry();
    assert_eq!((code, out), (Some(0), json!({ "land": ISSUE, "head": h, "dry_run": true, "contract_version": 1 })));
    assert_eq!(commits(v), before, "no commit");
    assert!(!t.join("merged").exists() && w.exists(), "no merge, and the worktree stays");
    assert!(!v.join(".bp/forge").exists(), "no snapshot refresh");
    assert_eq!(fetched(), at, "no fetch");
}

/// `check` on an issue with a held claim runs at the claim's worktree head, as `pr` does, wherever the caller stands.
#[test]
fn check_runs_the_blocks_in_the_held_claims_worktree() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "hello.txt"], "hello");
    let (code, out) = bp(v, t, &[], &["check", ISSUE]);
    assert_eq!((code, &out["runs"][0]["verdict"]), (Some(0), &json!("pass")), "run from the clone root: {out}");
    assert!(out["runs"][0]["id"].as_str().unwrap().starts_with(&format!("{ISSUE}/runs/hello-{h}")), "{out}");
}

/// Complete's stop: a pull request at the claim's head, and a passing run of each error block there. `pr` opens the
/// pull request even when it refuses `check_failed`, so the open pull request alone is no stop (27, issue 8).
#[test]
fn complete_stops_when_each_error_block_passes_at_the_head() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let done = || bp(v, t, &[], &["walk", ISSUE, "--for", "complete", "--done"]).1["done"].clone();
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("other.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "other.txt"], "other");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str(), &out["pull_request"]), (Some(2), Some("check_failed"), &json!(1)), "{out}");
    forge(t, &h, &[], &[]);
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_failed")), "the snapshot now holds it: {out}");
    assert_eq!(done(), false, "the pull request is open at the head, and its block failed there");
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let fixed = commit(&w, &["add", "hello.txt"], "hello");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["runs"][0]["verdict"]), (Some(0), &json!("pass")), "{out}");
    forge(t, &fixed, &[], &[]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "the repeat refreshes the snapshot");
    assert_eq!(done(), true);
}

/// Cuts [`ISSUE`] with `cut` args, commits `hello.txt` and opens pull request 1; the worktree and the head.
fn opened(v: &Path, t: &Path, cut: &[&str]) -> (std::path::PathBuf, String) {
    let (_, out) = bp(v, t, &[], &[&["cut", ISSUE], cut].concat());
    let w = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "hello.txt"], "hello");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    common::bp(v, &["set", "projects/bp/project", "reviewer", "r"]);
    (w, h)
}

#[test]
fn land_gates_a_merged_pull_request_and_keeps_a_branch_with_unmerged_commits() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, h) = opened(v, t, &[]);
    let land = || bp(v, t, &[], &["land", ISSUE]);
    let before = commits(v);
    forge_as(t, &h, &[("r", &h)], &["SUCCESS"], &json!({ "baseRefName": "side" }));
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("base_mismatch")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains(&format!("bp pr {ISSUE} retargets it to master")), "{out}");
    // Merged by another login's approval outside `land`: the gates hold on a merged head too, and nothing is recorded.
    forge_as(t, &h, &[("mallory", &h)], &["SUCCESS"], &json!({ "state": "MERGED" }));
    assert_eq!(land().1["code"], "review_missing");
    forge_as(t, &h, &[("r", &h)], &["SUCCESS"], &json!({ "state": "MERGED", "baseRefName": "side" }));
    assert_eq!(land().1["code"], "base_mismatch", "merged into another branch");
    assert_eq!(
        (commits(v), common::bp(v, &["show", ISSUE]).1["status"].clone()),
        (before.clone(), json!("in-progress"))
    );

    // A land that stopped after the merge: the gates pass again at the merged head, so it finishes without a merge.
    // A commit after the approved head stays on the kept branch. Logins ignore case.
    std::fs::write(w.join("later.txt"), "later\n").unwrap();
    let later = commit(&w, &["add", "later.txt"], "later");
    common::bp(v, &["set", "projects/bp/project", "reviewer", "R"]);
    forge_as(t, &h, &[("r", &h)], &["SUCCESS"], &json!({ "state": "MERGED" }));
    let (code, out) = land();
    assert_eq!((code, out["status"].as_str(), &out["branch_kept"]), (Some(0), Some("resolved"), &json!(true)), "{out}");
    assert!(!t.join("merged").exists(), "a merged pull request is not merged again");
    assert!(!w.exists(), "the worktree goes; its commits stay on the branch");
    assert_eq!(git(&t.join("code"), &["rev-parse", "bp/a"]).trim(), later);
    assert_eq!(git(&t.join("origin.git"), &["branch", "--list", "bp/a"]), "", "the forge's copy is the merged head");
}

#[test]
fn pr_targets_the_claim_base_and_reruns_until_each_block_passes_as_written() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, out) = bp(v, t, &[], &["cut", BARE]);
    let wb = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(wb.join("k.txt"), "k\n").unwrap();
    commit(&wb, &["add", "k.txt"], "k");
    let (code, out) = bp(v, t, &[], &["pr", BARE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_empty")), "{out}");
    assert_eq!(git(&t.join("origin.git"), &["branch", "--list", "bp/bare"]), "", "refused before the push");

    let code_dir = t.join("code");
    git(&code_dir, &["push", "-q", "origin", "master:refs/heads/release-1"]);
    git(&code_dir, &["fetch", "-q", "origin"]);
    let (_, h) = opened(v, t, &["--base", "origin/release-1"]);
    let created = std::fs::read_to_string(t.join("created")).unwrap();
    assert!(created.starts_with("pr create --head bp/a --base release-1 "), "{created}");
    forge_as(t, &h, &[("r", &h)], &["SUCCESS"], &json!({ "baseRefName": "master" }));
    let before = commits(v);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0), "a base moved off the claim's is retargeted");
    assert_eq!(std::fs::read_to_string(t.join("edited")).unwrap().trim(), "pr edit 1 --base release-1");
    forge_as(t, &h, &[("r", &h)], &["SUCCESS"], &json!({ "baseRefName": "release-1" }));
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "a passing run of each block at the head");

    // The human tightens the block after its run: the run no longer counts, and `pr` runs it again.
    let path = v.join(format!("{ISSUE}.md"));
    let text = std::fs::read_to_string(&path).unwrap().replace("test -f hello.txt", "test -f hello.txt && true");
    std::fs::write(&path, text).unwrap();
    git(v, &["commit", "-qam", "tighten"]);
    assert_eq!(bp(v, t, &[], &["land", ISSUE]).1["code"], "evidence_stale");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["runs"][0]["verdict"].as_str()), (Some(0), Some("pass")), "{out}");
    let body = std::fs::read_to_string(t.join("body")).unwrap();
    assert!(body.contains(&format!("| {ISSUE} | hello | changed | `")), "the body names the block: {body}");
    assert!(!body.contains("test -f"), "and never its text: {body}");
    // The changed set goes to the forge as an empty commit that names it, so the approval of `h` no longer counts.
    let named = out["head"].as_str().unwrap().to_string();
    let origin = t.join("origin.git");
    assert_eq!(git(&origin, &["rev-parse", &format!("{named}^")]).trim(), h, "{out}");
    assert_eq!(git(&origin, &["diff", &h, &named]), "", "the commit is empty");
    let trailer = git(&origin, &["log", "-1", "--format=%(trailers:key=Bp-Checks,valueonly)", &named]);
    assert_eq!(trailer.trim().len(), 64, "{trailer}");
    forge_as(t, &named, &[("r", &h)], &["SUCCESS"], &json!({ "baseRefName": "release-1" }));
    let (code, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("review_missing")), "{out}");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "the forge stores the body with CRLF ends");

    // A failed run at the head reruns at the same head, as a flaky block needs no code change.
    let text = std::fs::read_to_string(&path).unwrap().replace("&& true", "&& test -f never.txt");
    std::fs::write(&path, text).unwrap();
    git(v, &["commit", "-qam", "never"]);
    for attempt in [1, 2] {
        let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
        assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_failed")), "{out}");
        assert!(out["subject"].as_str().unwrap().ends_with(&format!("-{attempt}")), "{out}");
        assert_eq!(out["check_changes"][0]["name"], "hello", "a failed pr lists the rows too: {out}");
        let head = git(&origin, &["rev-parse", "bp/a"]).trim().to_string();
        forge_as(t, &head, &[], &["SUCCESS"], &json!({ "baseRefName": "release-1" }));
    }
    assert_eq!(
        commits(v).parse::<u32>().unwrap(),
        before.parse::<u32>().unwrap() + 6,
        "two pr, two edits, two failed pr"
    );
}

#[test]
fn a_push_after_the_approval_refuses_and_whitespace_is_part_of_the_diff() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, _) = opened(v, t, &[]);
    let land = || bp(v, t, &[], &["land", ISSUE]);
    std::fs::write(w.join("go.sh"), "rm -rf /x/cache\n").unwrap();
    let h1 = commit(&w, &["add", "go.sh"], "go");
    std::fs::write(w.join("go.sh"), "rm -rf /x/ cache\n").unwrap();
    let h2 = commit(&w, &["add", "go.sh"], "space");
    forge(t, &h2, &[("r", &h1)], &["SUCCESS"]);
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("diff_changed")), "one space deletes /x: {out}");
    assert!(
        out["fix"].as_str().unwrap().starts_with(&format!("r approved {h1}, and the diff at {h2} differs")),
        "{out}"
    );
    git(&w, &["commit", "--amend", "-qm", "space, again"]);
    let h3 = git(&w, &["rev-parse", "HEAD"]).trim().to_string();
    forge(t, &h3, &[("r", &h2)], &["SUCCESS"]);
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("review_missing")), "{out}");
    assert!(
        out["fix"].as_str().unwrap().starts_with(&format!("diff unchanged since {h2}; the human approves")),
        "{out}"
    );
    assert!(!t.join("merged").exists(), "no refusal merges");
}

#[test]
fn a_check_change_after_the_approval_needs_a_new_one() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    forge(t, &h, &[("r", &h)], &["SUCCESS"]);
    let land = || bp(v, t, &[], &["land", ISSUE]);
    // The milestone gains a block after the approval: the issue's run still counts, the set the head names does not.
    let path = v.join(format!("{M}.md"));
    let text = std::fs::read_to_string(&path).unwrap() + "\n~~~check shell {name=more expect=0}\ntrue\n~~~\n";
    std::fs::write(&path, text).unwrap();
    git(v, &["commit", "-qam", "more"]);
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("checks_changed")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains(&format!("bp pr {ISSUE} pushes a commit")), "{out}");
    // A delta the issue caused adds a block too.
    let body = v.join(".bp/body.md");
    std::fs::write(&body, "## One\n\nOld.\n\n~~~check shell {name=dz expect=0}\ntrue\n~~~\n").unwrap();
    let (ms, cause, file) = (format!("milestone={M}"), format!("cause={ISSUE}"), body.to_str().unwrap());
    let record = ["record", "delta", S, "--field", &ms, "--field", &cause, "--field", "change=one block"];
    let (_, d) = common::bp(v, &[&record[..], &["--body-file", file]].concat());
    let delta = d["id"].as_str().unwrap().to_string();

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["deltas"]), (Some(0), &json!([delta])), "{out}");
    let rows: Vec<(&str, &str, &str)> = out["check_changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["owner"].as_str().unwrap(), c["name"].as_str().unwrap(), c["change"].as_str().unwrap()))
        .collect();
    assert_eq!(rows, [(M, "more", "added"), (delta.as_str(), "dz", "added")], "{out}");
    let named = out["head"].as_str().unwrap().to_string();
    assert_ne!(named, h, "the changed set is pushed");
    forge(t, &named, &[("r", &h)], &["SUCCESS"]);
    assert_eq!(land().1["code"], "review_missing", "the push dismissed the approval");
    forge(t, &named, &[("r", &named)], &["SUCCESS"]);
    let (code, out) = land();
    assert_eq!((code, out["status"].as_str()), (Some(0), Some("resolved")), "{out}");
}

#[test]
fn the_baseline_is_the_earliest_claim_through_a_release_and_a_rename() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    opened(v, t, &[]);
    let path = v.join(format!("{ISSUE}.md"));
    let text = std::fs::read_to_string(&path).unwrap().replace("test -f hello.txt", "true");
    std::fs::write(&path, text).unwrap();
    git(v, &["commit", "-qam", "weaken"]);
    let later = [("BP_NOW", "2026-10-19T09:00:00Z")];
    assert_eq!(bp(v, t, &later, &["release", ISSUE, "--reason", "escalated"]).0, Some(0));
    assert_eq!(bp(v, t, &later, &["cut", ISSUE]).0, Some(0), "a new claim");
    let (code, out) = bp(v, t, &later, &["pr", ISSUE]);
    assert_eq!((code, out["check_changes"][0]["name"].as_str()), (Some(0), Some("hello")), "{out}");

    // A rename after the change moves the claims, not the baseline; the old text is read under the old path.
    let renamed = "projects/bp/issues/b";
    assert_eq!(common::bp(v, &["rename", ISSUE, "b"]).0, Some(0));
    let (code, out) = bp(v, t, &later, &["pr", renamed]);
    let row = &out["check_changes"][0];
    assert_eq!(
        (code, row["owner"].as_str(), row["change"].as_str()),
        (Some(0), Some(renamed), Some("changed")),
        "{out}"
    );
    let then = row["show"][0].as_str().unwrap();
    assert!(then.starts_with(&format!("bp show {ISSUE} --at ")), "{out}");
    let at = then.rsplit(' ').next().unwrap();
    assert_eq!(common::bp(v, &["show", ISSUE, "--at", at]).1["title"], "A", "the command reads the old text");
}

#[test]
fn a_chained_delta_carries_only_what_its_own_link_changes() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    opened(v, t, &[]);
    let spec = "type: architecture\nlevel: container\ntitle: One\nlede: One box.\nnodes:\n  - {id: o, noun: One, at: [0, 0]}\n";
    let record = |cause: &str, body: &str, spec: &str, extra: &[&str]| {
        let (file, yaml) = (v.join(".bp/body.md"), v.join(".bp/one.yaml"));
        std::fs::write(&file, body).unwrap();
        std::fs::write(&yaml, spec).unwrap();
        let (ms, cause, diagram) =
            (format!("milestone={M}"), format!("cause={cause}"), format!("one={}", yaml.display()));
        let args = ["record", "delta", S, "--field", &ms, "--field", &cause, "--diagram", &diagram];
        let (code, d) = common::bp(v, &[&args[..], extra, &["--body-file", file.to_str().unwrap()]].concat());
        assert_eq!(code, Some(0), "{d}");
        d["id"].as_str().unwrap().to_string()
    };
    // BARE's link adds a block and a spec; the issue's link builds on it, adds a block and replaces the spec.
    let t_body = "## One\n\nOld.\n\n~~~check shell {name=t-only expect=0}\ntrue\n~~~\n";
    let first = record(BARE, t_body, spec, &[]);
    let u_body = format!("{t_body}\n~~~check shell {{name=u-only expect=0}}\ntrue\n~~~\n");
    let on = format!("chains={first}");
    let second = record(ISSUE, &u_body, &spec.replace("One box.", "The box."), &["--field", &on]);

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["deltas"]), (Some(0), &json!([second])), "{out}");
    let rows: Vec<(&str, &str, &str)> = out["check_changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["owner"].as_str().unwrap(), c["name"].as_str().unwrap(), c["change"].as_str().unwrap()))
        .collect();
    assert_eq!(rows, [(second.as_str(), "u-only", "added")], "t-only is BARE's: {out}");
    assert_eq!(out["check_changes"][0]["show"][0], format!("bp show {first}"), "{out}");
    let drawn = &out["diagrams"][0];
    assert_eq!((drawn["name"].as_str(), drawn["change"].as_str()), (Some("one"), Some("replaced")), "{out}");
    // The design has no `one`: only the first link's spec gives the page a base to compare with.
    assert!(v.join(drawn["diff"].as_str().unwrap()).exists(), "{out}");
}
