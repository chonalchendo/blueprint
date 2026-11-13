//! `bp pr` and `bp land` through the binary: a vault, a bare origin and its clone, and a `gh` that answers from a
//! file the test writes, so each refusal of `land` is set up by hand.

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use common::{commits, git};
use serde_json::{Value, json};

const ISSUE: &str = "projects/bp/issues/a";

const BARE: &str = "projects/bp/issues/bare";

const M: &str = "projects/bp/milestones/m";

const S: &str = "projects/bp/designs/system";

/// The vault, and a directory holding `origin.git`, its clone `code` and the fake `bin/gh`. The fake answers
/// `api graphql` with origin's master as the default branch, at the sha `default-oid` names if any, and logs it, or
/// fails while `graphql-fail` exists; `pr
/// list --head <project>/<slug>` from `prs-<slug>.json`, else `prs.json`; `pr view <n>` with the row of either file
/// that holds pull request `n`. A row with no `baseRefOid` reads origin's sha of its base branch now. It answers `pr
/// create` with a url and logs `pr edit` and `pr merge`; it fails when a git location variable reaches it. A merge,
/// unless `merge-noop` exists, pushes the commit that `squash` names, if any, else the head's tree on master, to
/// origin's master, as the forge's squash merge does, marks the row `MERGED` with that commit as its merge commit,
/// then runs `on-merge`, if any. `pr list`
/// runs `on-list` first, if any. It keeps the body in `body` and prints it as JSON with CRLF line ends, as a forge may
/// store it, escaping only those: a body holds no quote. The vault holds [`ISSUE`], with one block, under [`M`], with
/// none, and [`BARE`], with none.
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    setup_under("approved")
}

/// [`setup`], with [`M`] at `status`.
fn setup_under(status: &str) -> (tempfile::TempDir, tempfile::TempDir) {
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
! env | grep -q '^GIT_\(DIR\|WORK_TREE\|INDEX_FILE\)=' || {{ echo 'a git location variable reached gh' >&2; exit 7; }}
row() {{ grep -l "\"number\":$1[,}}]" '{t}'/prs*.json 2>/dev/null | head -1; }}
show() {{
  if grep -q '"baseRefOid"' "$1"; then cat "$1"; return; fi
  b=$(sed -n 's/.*"baseRefName":"\([^"]*\)".*/\1/p' "$1")
  sed "s/^\[{{/[{{\"baseRefOid\":\"$(git -C '{t}/origin.git' rev-parse -q --verify "refs/heads/$b")\",/" "$1"
}}
case "$1 $2" in
  "api graphql")
    echo "$*" >> '{t}/graphql'; [ ! -f '{t}/graphql-fail' ] || {{ echo 'HTTP 502' >&2; exit 1; }}
    oid=$(cat '{t}/default-oid' 2>/dev/null || git -C '{t}/origin.git' rev-parse master)
    printf '{{"data":{{"repository":{{"defaultBranchRef":{{"name":"master","target":{{"oid":"%s"}}}}}}}}}}' "$oid" ;;
  "pr list")
    [ ! -f '{t}/on-list' ] || sh '{t}/on-list'
    f="{t}/prs-$(basename "$4").json"; [ -f "$f" ] || f='{t}/prs.json'
    if [ -f "$f" ]; then show "$f"; else echo '[]'; fi ;;
  "pr create") echo "$*" > '{t}/created'; shift 9; printf '%s' "$1" > '{t}/body'; echo 'https://forge.invalid/o/r/pull/1' ;;
  "pr edit") if [ "$4" = --body ]; then printf '%s' "$5" > '{t}/body'; else echo "$*" > '{t}/edited'; fi ;;
  "pr view")
    if [ "$5" != body ]; then show "$(row "$3")" | sed 's/^\[//; s/\]$//'; exit 0; fi
    printf '{{"body":"'; awk 'BEGIN {{ ORS = "\\r\\n" }} {{ print }}' '{t}/body'; printf '"}}' ;;
  "pr merge")
    echo "$*" > '{t}/merged'; [ ! -f '{t}/merge-noop' ] || exit 0
    before=$(git -C '{t}/origin.git' rev-parse master)
    if [ -f '{t}/squash' ]; then git -C '{t}/code' push -q origin "$(cat '{t}/squash')":refs/heads/master
    else
      squash=$(git -C '{t}/origin.git' -c user.name=f -c user.email=f@f commit-tree "$6^{{tree}}" -p master -m squash)
      git -C '{t}/origin.git' update-ref refs/heads/master "$squash"
    fi
    f=$(row "$3"); m=$(git -C '{t}/origin.git' rev-parse master)
    merged="s/\"state\":\"OPEN\"/\"state\":\"MERGED\"/; s/^\[{{/[{{\"baseRefOid\":\"$before\",\"mergeCommit\":{{\"oid\":\"$m\"}},/"
    sed "$merged" "$f" > "$f.tmp" && mv "$f.tmp" "$f"
    [ ! -f '{t}/on-merge' ] || sh '{t}/on-merge' ;;
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
        (&format!("{M}.md"), &format!("---\nkind: milestone\ntitle: M\nstatus: {status}\n---\n\n## Finish line\n")),
        (&format!("{S}.md"), "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n"),
    ]);
    (v, t)
}

/// `bp <args> --json` from the clone, with the vault named and the fake first on `PATH`. Each fix and next step of a
/// `land` is one the session can follow ([`session_can_follow`]).
fn bp(v: &Path, t: &Path, env: &[(&str, &str)], args: &[&str]) -> (Option<i32>, Value) {
    let path = format!("{}:{}", t.join("bin").display(), std::env::var("PATH").unwrap());
    let vault = v.display().to_string();
    let base = [("BP_VAULT", vault.as_str()), ("PATH", path.as_str())];
    let out = common::bp_env(&t.join("code"), &base.into_iter().chain(env.iter().copied()).collect::<Vec<_>>(), args);
    if args.first() == Some(&"land") {
        let next = out.1["next"].as_array().into_iter().flatten().filter_map(Value::as_str);
        out.1["fix"].as_str().into_iter().chain(next).for_each(session_can_follow);
    }
    out
}

/// The session runs `land` (37, design change 3), so each `bp <verb>` that a fix of `land` names and the session's
/// role refuses sits in a clause that dispatches a worker or names the human's act: a session that follows the fix
/// never meets `verb_outside_role` (review of #100, C3).
fn session_can_follow(fix: &str) {
    let verbs = bp_operation::Role::Session.verbs().unwrap();
    for (at, _) in fix.match_indices("bp ") {
        if at > 0 && !matches!(fix.as_bytes()[at - 1], b' ' | b'`' | b'(') {
            continue;
        }
        let verb = fix[at + 3..].split(|c: char| !c.is_ascii_alphanumeric() && c != '-').next().unwrap_or_default();
        if verbs.contains(&verb) || bp_operation::READ_VERBS.contains(&verb) {
            continue;
        }
        let clause = fix[..at].rsplit(';').next().unwrap_or_default();
        assert!(
            clause.contains("dispatch a worker") || clause.contains("the human"),
            "the session cannot run bp {verb}, and the fix names no dispatch: {fix}"
        );
    }
}

/// Pull request 1 at `head`, with `checks` conclusions, as `gh pr list` prints it.
fn forge(t: &Path, head: &str, checks: &[&str]) {
    forge_as(t, head, checks, &json!({}));
}

/// [`forge`], with `over` replacing fields: a base, a number, a state or a forge review that `land` reads as nothing.
/// A `MERGED` row with no merge commit gets one: the head's tree on origin's master, pushed there once.
fn forge_as(t: &Path, head: &str, checks: &[&str], over: &Value) {
    let checks: Vec<Value> = checks.iter().map(|c| json!({ "conclusion": c })).collect();
    let mut pr = json!({
        "number": 1, "state": "OPEN", "baseRefName": "master",
        "headRefName": "bp/a", "headRefOid": head, "statusCheckRollup": checks,
    });
    if over["state"] == "MERGED" {
        pr["mergeCommit"] = json!({ "oid": merged_at(t, head) });
        // The forge keeps the base the pull request merged onto, though the branch has moved on.
        pr["baseRefOid"] = json!(std::fs::read_to_string(t.join("merge-base-oid")).unwrap());
    }
    pr.as_object_mut().unwrap().extend(over.as_object().unwrap().clone());
    std::fs::write(t.join("prs.json"), json!([pr]).to_string()).unwrap();
}

/// The forge's squash of `head` onto origin's master, pushed there the first time and read from `merge-oid` after.
fn merged_at(t: &Path, head: &str) -> String {
    if let Ok(oid) = std::fs::read_to_string(t.join("merge-oid")) {
        return oid;
    }
    let code = t.join("code");
    git(&code, &["fetch", "-q", "origin"]);
    let tree = format!("{head}^{{tree}}");
    let oid = git(&code, &["commit-tree", &tree, "-p", "origin/master", "-m", "squash"]).trim().to_string();
    std::fs::write(t.join("merge-base-oid"), git(&code, &["rev-parse", "origin/master"]).trim()).unwrap();
    git(&code, &["push", "-q", "origin", &format!("{oid}:refs/heads/master")]);
    std::fs::write(t.join("merge-oid"), &oid).unwrap();
    oid
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
    forge(t, &h, &[]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "a head with a pull request and runs");
    // The repeat refreshed the snapshot, and `list` reads it with no `gh` on its `PATH` (27, design change 9).
    assert!(v.join(".bp/forge/bp/open-prs.json").is_file() && v.join(".bp/forge/bp/pr-1.json").is_file());
    let (_, list) = common::bp_env(v, &[("PATH", "/usr/bin:/bin")], &["list", "issue"]);
    assert!(list["forge"] == "snapshot" && list["forge_age"].is_i64(), "{list}");
    assert_eq!(list["rows"].as_array().unwrap().iter().find(|r| r["id"] == ISSUE).unwrap()["intent"], "needs-review");

    let before = commits(v);
    let land = || bp(v, t, &[], &["land", ISSUE]).1["code"].clone();
    assert_eq!(land(), "checks_pending", "no check");
    // A forge approval counts for nothing: a medium head lands on the reviews of its round (37, design change 3).
    let approved = json!({ "reviews": [{ "state": "APPROVED", "author": { "login": "r" }, "commit": { "oid": h } }] });
    forge_as(t, &h, &["SUCCESS"], &approved);
    let (_, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!(out["code"], "review_missing", "{out}");
    assert!(out["fix"].as_str().unwrap().contains("missing lenses: conformance, failure-paths;"), "{out}");
    lenses(v, t, &h, &MEDIUM);
    let bye = commit(&w, &["rm", "-q", "hello.txt"], "bye");
    git(&w, &["push", "-q", "origin", "HEAD:bp/a"]);
    forge(t, &bye, &["SUCCESS"]);
    assert_eq!(land(), "evidence_stale", "no run at the head");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str(), &out["pull_request"]), (Some(2), Some("check_failed"), &json!(1)));
    let (code, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_failed")), "{out}");
    assert!(out["subject"].as_str().unwrap().starts_with(&format!("{ISSUE}/runs/hello-{bye}")), "{out}");
    assert!(!t.join("merged").exists(), "no refusal merges");
    assert_eq!(commits(v).parse::<u32>().unwrap(), before.parse::<u32>().unwrap() + 3, "two reviews and one pr");

    git(&w, &["revert", "--no-edit", "HEAD"]);
    let back = git(&w, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).1["runs"][0]["verdict"], "pass");
    // `back` holds the diff the reviews read at `h`, but it is a new head: round 2 needs its verify review.
    forge(t, &back, &["SUCCESS", "SKIPPED"]);
    let (_, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!(out["code"], "review_missing", "{out}");
    assert!(out["fix"].as_str().unwrap().starts_with(&format!("round 2 at {back}: missing lenses: verify;")), "{out}");
    assert_eq!(review(v, t, &back, &["round=2", "lens=verify", "mode=verify"], &[]).0, Some(0));
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
/// it names the head it would merge, and merges, commits and refreshes nothing (32, design change 5). It fetches, so
/// its ancestry test reads origin as `land` does (34, design change 5).
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
    assert_eq!(both("no pull request"), "review_missing");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    forge(t, &h, &[]);
    assert_eq!(both("no check"), "checks_pending");
    forge(t, &h, &["SUCCESS"]);
    assert_eq!(both("no review"), "review_missing");
    lenses(v, t, &h, &MEDIUM);
    std::fs::remove_dir_all(v.join(".bp/forge")).unwrap();
    let fetched = || std::fs::metadata(t.join("code/.git/FETCH_HEAD")).and_then(|m| m.modified()).unwrap();
    let (before, at) = (commits(v), fetched());
    let (code, out) = dry();
    let would = json!({ "land": ISSUE, "head": h, "level": "medium", "dry_run": true, "contract_version": 1 });
    assert_eq!((code, out), (Some(0), would), "a medium head needs no approval");
    assert_eq!(commits(v), before, "no commit");
    assert!(!t.join("merged").exists() && w.exists(), "no merge, and the worktree stays");
    assert!(!v.join(".bp/forge").exists(), "no snapshot refresh");
    assert_ne!(fetched(), at, "the dry run fetches");
}

/// A git hook that runs bp sets git's location variables. Here each names the vault's own index or tree: right for
/// the vault's git, wrong for the worktree's. `cut` still fills the worktree's own index, and `land` still checks the
/// worktree's own files. No variable reaches `gh`, which runs git in the clone (review of #98, the late list).
#[test]
fn git_s_index_and_tree_variables_never_reach_the_claim_s_worktree() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let index = v.join(".git/index").display().to_string();
    let (code, cut) = bp(v, t, &[("GIT_INDEX_FILE", &index)], &["cut", ISSUE]);
    assert_eq!(code, Some(0), "{cut}");
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    assert_eq!(git(&w, &["status", "--porcelain"]), "", "cut checked the worktree out into its own index");
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "hello.txt"], "hello");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    forge(t, &h, &["SUCCESS"]);
    lenses(v, t, &h, &MEDIUM);
    let tree = v.display().to_string();
    let (code, out) = bp(v, t, &[("GIT_WORK_TREE", &tree)], &["land", ISSUE, "--dry-run"]);
    assert_eq!((code, &out["head"]), (Some(0), &json!(h)), "the worktree is clean: {out}");
    let (code, out) = bp(v, t, &[("GIT_INDEX_FILE", &index)], &["land", ISSUE, "--dry-run"]);
    assert_eq!(code, Some(0), "{out}");
}

/// `pr` records the computed level of the head it pushes on each run it writes there, so an intent reads the level
/// with no fetch (22, constraint 8). A head whose runs record no level is no repeat.
#[test]
fn pr_records_the_risk_level_of_its_head_on_each_run() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    let run = v.join(format!("{ISSUE}/runs/hello-{h}-1.md"));
    let text = std::fs::read_to_string(&run).unwrap();
    assert!(text.contains("\nrisk: medium\n"), "hello.txt matches no low pattern: {text}");
    forge(t, &h, &[]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "the level is recorded at the head");
    std::fs::write(&run, text.replace("risk: medium\n", "")).unwrap();
    git(v, &["commit", "-qam", "no level"]);
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["head"]), (Some(0), &json!(h)), "{out}");
    let again = std::fs::read_to_string(v.join(format!("{ISSUE}/runs/hello-{h}-2.md"))).unwrap();
    assert!(again.contains("\nrisk: medium\n"), "{again}");
}

/// A diff that touches a high path outside the low patterns records `security` on each run, and the intents read it
/// there: round one asks for the security lens (37, design change 8; review of #98, finding 11 b and c).
#[test]
fn pr_records_security_and_round_one_asks_for_its_lens() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, out) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    std::fs::create_dir_all(w.join(".github/workflows")).unwrap();
    std::fs::write(w.join(".github/workflows/x.yml"), "on: push\n").unwrap();
    let h = commit(&w, &["add", "-A"], "hello");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    let run = std::fs::read_to_string(v.join(format!("{ISSUE}/runs/hello-{h}-1.md"))).unwrap();
    assert!(run.contains("\nrisk: high\n") && run.contains("\nsecurity: true\n"), "{run}");
    forge(t, &h, &[]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "the repeat refreshes the snapshot");
    let (_, list) = common::bp_env(v, &[("PATH", "/usr/bin:/bin")], &["list", "issue"]);
    let row = list["rows"].as_array().unwrap().iter().find(|r| r["id"] == ISSUE).unwrap().clone();
    let note = "round 1: missing lenses: conformance, failure-paths, tests-and-checks, security";
    assert_eq!((&row["intent"], &row["note"]), (&json!("needs-review"), &json!(note)), "{row}");
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
    forge(t, &h, &[]);
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_failed")), "the snapshot now holds it: {out}");
    assert_eq!(done(), false, "the pull request is open at the head, and its block failed there");
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let fixed = commit(&w, &["add", "hello.txt"], "hello");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["runs"][0]["verdict"]), (Some(0), &json!("pass")), "{out}");
    forge(t, &fixed, &[]);
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
    (w, h)
}

/// Records a round-1 review of each of `lenses` at `head`, with no finding.
fn lenses(v: &Path, t: &Path, head: &str, lenses: &[&str]) {
    for lens in lenses {
        let field = format!("lens={lens}");
        let (code, out) = review(v, t, head, &["round=1", &field, "mode=full"], &[]);
        assert_eq!(code, Some(0), "{lens}: {out}");
    }
}

/// The lenses of medium.
const MEDIUM: [&str; 2] = ["conformance", "failure-paths"];

#[test]
fn land_gates_a_merged_pull_request_and_keeps_a_branch_with_unmerged_commits() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, h) = opened(v, t, &[]);
    let land = || bp(v, t, &[], &["land", ISSUE]);
    git(&t.join("code"), &["push", "-q", "origin", "master:refs/heads/side"]);
    forge_as(t, &h, &["SUCCESS"], &json!({ "baseRefName": "side" }));
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("base_mismatch")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains(&format!("`bp pr {ISSUE}`, which retargets it to master")), "{out}");
    // Merged outside `land` with no review: the gates hold on a merged head too, and nothing is recorded.
    forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED" }));
    assert_eq!(land().1["code"], "review_missing");
    lenses(v, t, &h, &MEDIUM);
    let before = commits(v);
    forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED", "baseRefName": "side" }));
    assert_eq!(land().1["code"], "base_mismatch", "merged into another branch");
    assert_eq!(
        (commits(v), common::bp(v, &["show", ISSUE]).1["status"].clone()),
        (before.clone(), json!("in-progress"))
    );

    // A land that stopped after the merge: the gates pass again at the merged head, so it finishes without a merge.
    // A commit after the reviewed head stays on the kept branch.
    std::fs::write(w.join("later.txt"), "later\n").unwrap();
    let later = commit(&w, &["add", "later.txt"], "later");
    forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED" }));
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
    forge_as(t, &h, &["SUCCESS"], &json!({ "baseRefName": "master" }));
    let before = commits(v);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0), "a base moved off the claim's is retargeted");
    assert_eq!(std::fs::read_to_string(t.join("edited")).unwrap().trim(), "pr edit 1 --base release-1");
    forge_as(t, &h, &["SUCCESS"], &json!({ "baseRefName": "release-1" }));
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
    // The changed set goes to the forge as an empty commit that names it: a new head, which no review has read.
    let named = out["head"].as_str().unwrap().to_string();
    let origin = t.join("origin.git");
    assert_eq!(git(&origin, &["rev-parse", &format!("{named}^")]).trim(), h, "{out}");
    assert_eq!(git(&origin, &["diff", &h, &named]), "", "the commit is empty");
    let trailer = git(&origin, &["log", "-1", "--format=%(trailers:key=Bp-Checks,valueonly)", &named]);
    assert_eq!(trailer.trim().len(), 64, "{trailer}");
    forge_as(t, &named, &["SUCCESS"], &json!({ "baseRefName": "release-1" }));
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
        forge_as(t, &head, &["SUCCESS"], &json!({ "baseRefName": "release-1" }));
    }
    assert_eq!(
        commits(v).parse::<u32>().unwrap(),
        before.parse::<u32>().unwrap() + 8,
        "two pr, two edits, the base run of each edit, two failed pr"
    );
}

/// A push after the reviews is a new head: round 2 waits for its verify review, whatever the diff (37, design
/// change 6).
#[test]
fn a_push_after_the_reviews_needs_a_verify_review() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, h) = opened(v, t, &[]);
    forge(t, &h, &["SUCCESS"]);
    lenses(v, t, &h, &MEDIUM);
    git(&w, &["commit", "--amend", "-qm", "hello, again"]);
    let h2 = git(&w, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    forge(t, &h2, &["SUCCESS"]);
    let (code, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("review_missing")), "{out}");
    assert!(out["fix"].as_str().unwrap().starts_with(&format!("round 2 at {h2}: missing lenses: verify;")), "{out}");
    assert!(!t.join("merged").exists(), "no refusal merges");
}

#[test]
fn a_check_change_after_the_reviews_needs_a_new_head() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    forge(t, &h, &["SUCCESS"]);
    lenses(v, t, &h, &MEDIUM);
    let land = || bp(v, t, &[], &["land", ISSUE]);
    // The milestone gains a block after the reviews: the issue's run still counts, the set the head names does not.
    let path = v.join(format!("{M}.md"));
    let text = std::fs::read_to_string(&path).unwrap() + "\n~~~check shell {name=more expect=0}\ntrue\n~~~\n";
    std::fs::write(&path, text).unwrap();
    git(v, &["commit", "-qam", "more"]);
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("checks_changed")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains(&format!("`bp pr {ISSUE}`, which pushes a commit")), "{out}");
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
    forge(t, &named, &["SUCCESS"]);
    // The carried delta raises the issue to high (37, design change 1). Round one never read every lens of high at
    // one head, so the new head is round one, with each lens; then the human's signed approval.
    let (_, out) = land();
    assert_eq!(out["code"], "review_missing", "{out}");
    let fix = out["fix"].as_str().unwrap();
    assert!(
        fix.starts_with(&format!("round 1 at {named}: missing lenses: conformance, failure-paths, tests-and-checks;")),
        "{fix}"
    );
    lenses(v, t, &named, &HIGH[..3]);
    assert_eq!(land().1["code"], "signer_missing", "high reads the signer, and origin's risk file names none");
}

#[test]
fn the_baseline_is_the_earliest_claim_through_a_release_and_a_rename() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    opened(v, t, &[]);
    let path = v.join(format!("{ISSUE}.md"));
    // Still red without the change, so `pr` does not refuse it as vacuous (37, design change 10).
    let text = std::fs::read_to_string(&path).unwrap().replace("test -f hello.txt", "test -e hello.txt");
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

/// `pr`, `land --dry-run` and `land` refuse a head that lacks origin's base, which the forge would not merge, with the
/// rebase as the fix; once the rebase runs, `pr` passes (34, design change 5; findings 10 and 11 of 33).
#[test]
fn a_head_behind_its_base_refuses_until_the_rebase() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let code = t.join("code");
    let from = git(&code, &["rev-parse", "master"]).trim().to_string();
    let (w, h) = opened(v, t, &[]);
    std::fs::write(code.join("other.txt"), "m\n").unwrap();
    let tip = commit(&code, &["add", "other.txt"], "move master");
    git(&code, &["push", "-q", "origin", "master"]);
    forge(t, &h, &["SUCCESS"]);
    lenses(v, t, &h, &MEDIUM);
    let step =
        format!("git -C {} rebase --onto origin/master {from}, resolve any conflict, then bp pr {ISSUE}", w.display());
    // The worker runs `pr` and takes the step; the session runs `land` and dispatches it (review of #100, C3).
    let fix = format!("{h} lacks origin/master at {tip}; {step}");
    let dispatched = format!("{h} lacks origin/master at {tip}; dispatch a worker to run {step}; then bp land {ISSUE}");
    for args in [&["pr", ISSUE][..], &["land", ISSUE, "--dry-run"], &["land", ISSUE]] {
        let (code, out) = bp(v, t, &[], args);
        let got = (code, out["code"].as_str(), out["fix"].as_str());
        let fix = if args[0] == "pr" { &fix } else { &dispatched };
        assert_eq!(got, (Some(2), Some("head_behind_base"), Some(fix.as_str())), "{args:?}: {out}");
    }
    assert!(!t.join("merged").exists(), "no refusal merges");
    git(&w, &["rebase", "-q", "--onto", "origin/master", &from]);
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, &out["runs"][0]["verdict"]), (Some(0), &json!("pass")), "{out}");
}

const B: &str = "projects/bp/issues/b";

/// A stack ready for `land A`: B, cut from A's pull request head, edits the line A wrote, and the squash of A that the
/// forge merges writes it another way, so B's rebase conflicts.
struct Stack {
    wb: std::path::PathBuf,
    ha: String,
    hb: String,
    squash: String,
    claim: String,
}

fn stacked(v: &Path, t: &Path) -> Stack {
    let code = t.join("code");
    let issue = format!(
        "---\nkind: issue\ntitle: B\nstatus: open\nunder: {M}\ncites: [{S}]\n---\n\nBody.\n\n## Check\n\n```check shell {{name=has expect=0}}\ntest -f b.txt\n```\n"
    );
    std::fs::write(v.join(format!("{B}.md")), issue).unwrap();
    git(v, &["add", "-A"]);
    git(v, &["commit", "-qm", "b"]);
    let (_, a) = bp(v, t, &[], &["cut", ISSUE]);
    let wa = Path::new(a["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(wa.join("hello.txt"), "hi\n").unwrap();
    std::fs::write(wa.join("f.txt"), "a\n").unwrap();
    let ha = commit(&wa, &["add", "."], "a");
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(0));
    let pr = |slug: &str, n: u64, head: &str, base: &str| {
        let pr = json!([{ "number": n, "state": "OPEN", "baseRefName": base, "headRefName": format!("bp/{slug}"),
            "headRefOid": head, "statusCheckRollup": [{ "conclusion": "SUCCESS" }] }]);
        std::fs::write(t.join(format!("prs-{slug}.json")), pr.to_string()).unwrap();
    };
    pr("a", 1, &ha, "master");
    lenses(v, t, &ha, &MEDIUM);
    assert_eq!(common::bp(v, &["link", B, "depends", ISSUE]).0, Some(0));
    let (_, b) = bp(v, t, &[], &["cut", B]);
    assert_eq!(b["base_issue"], ISSUE, "{b}");
    let wb = Path::new(b["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(wb.join("b.txt"), "b\n").unwrap();
    std::fs::write(wb.join("f.txt"), "b\n").unwrap();
    let hb = commit(&wb, &["add", "."], "b");
    assert_eq!(bp(v, t, &[], &["pr", B]).0, Some(0));
    pr("b", 2, &hb, "bp/a");
    std::fs::write(code.join("hello.txt"), "hi\n").unwrap();
    std::fs::write(code.join("f.txt"), "A\n").unwrap();
    let squash = commit(&code, &["add", "."], "squash of a");
    git(&code, &["reset", "-q", "--hard", "HEAD~1"]);
    std::fs::write(t.join("squash"), &squash).unwrap();
    Stack { wb, ha, hb, squash, claim: b["claim"].as_str().unwrap().to_string() }
}

/// A layer whose base has not landed refuses `base_unmerged` before any refusal about the sha of that base: A's land
/// replaces that base and restacks B, so a worker's rebase onto it is wasted (review of #100, V6).
#[test]
fn a_layer_above_an_unlanded_base_waits_for_it_before_its_base_sha() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let Stack { ha, hb, .. } = stacked(v, t);
    let dry = || bp(v, t, &[], &["land", B, "--dry-run"]);
    let (code, out) = dry();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("base_unmerged")), "{out}");
    // A's branch moves, and the forge keeps the base sha it recorded for B.
    let code = t.join("code");
    let moved = git(&code, &["commit-tree", &format!("{ha}^{{tree}}"), "-p", &ha, "-m", "a again"]);
    git(&code, &["push", "-q", "origin", &format!("{}:refs/heads/bp/a", moved.trim())]);
    let row = json!([{ "number": 2, "state": "OPEN", "baseRefName": "bp/a", "baseRefOid": ha, "headRefName": "bp/b",
        "headRefOid": hb, "statusCheckRollup": [{ "conclusion": "SUCCESS" }] }]);
    std::fs::write(t.join("prs-b.json"), row.to_string()).unwrap();
    let (code, out) = dry();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("base_unmerged")), "{out}");
    assert!(out["fix"].as_str().unwrap().starts_with(&format!("bp land {ISSUE} first")), "{out}");
}

/// A layer `land` cannot move onto the base gets its step on its held claim as `## Next`, after the seed in the walks
/// of complete and respond. Its `pr` refuses `head_behind_base` until the rebase; the passing `pr` drops the step
/// (34, design change 5).
#[test]
fn a_stranded_layer_carries_its_rebase_on_its_claim_until_pr_passes() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let Stack { wb, ha, hb, squash, claim } = stacked(v, t);
    let (status, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((status, &out["stranded"]), (Some(0), &json!([B])), "{out}");
    let step =
        format!("in {}: git rebase --onto origin/master {ha}, resolve the conflict, then bp pr {B}", wb.display());
    assert_eq!(out["next"][0], format!("dispatch a worker for {B}: {step}").as_str(), "{out}");
    let text = || std::fs::read_to_string(v.join(format!("{claim}.md"))).unwrap();
    assert!(text().ends_with(&format!("\n\n## Next\n\n{step}\n")), "{}", text());
    for action in ["complete", "respond"] {
        let (_, slice) = bp(v, t, &[], &["walk", B, "--for", action]);
        let bodies = slice["bodies"].as_array().unwrap_or_else(|| panic!("{action}: {slice}"));
        let at = bodies.iter().position(|x| x["id"] == claim.as_str()).unwrap_or_else(|| panic!("{action}: {slice}"));
        assert_eq!((&bodies[0]["id"], at), (&json!(B), 1), "{action}: the seed leads, its claim follows");
        assert!(bodies[at]["body"].as_str().unwrap().contains(&step), "{action}: {}", bodies[at]);
    }
    let fix = format!(
        "{hb} lacks origin/master at {squash}; git -C {} rebase --onto origin/master {ha}, resolve any conflict, then bp pr {B}",
        wb.display()
    );
    let (status, out) = bp(v, t, &[], &["pr", B]);
    assert_eq!(
        (status, out["code"].as_str(), out["fix"].as_str()),
        (Some(2), Some("head_behind_base"), Some(fix.as_str())),
        "{out}"
    );
    let rebase = std::process::Command::new("git")
        .args(["-C", wb.to_str().unwrap(), "rebase", "-q", "--onto", "origin/master", &ha])
        .output()
        .unwrap();
    assert!(!rebase.status.success(), "the rebase conflicts");
    std::fs::write(wb.join("f.txt"), "b\n").unwrap();
    git(&wb, &["add", "f.txt"]);
    git(&wb, &["-c", "core.editor=true", "rebase", "--continue"]);
    let (status, out) = bp(v, t, &[], &["pr", B]);
    assert_eq!((status, &out["runs"][0]["verdict"]), (Some(0), &json!("pass")), "{out}");
    assert!(!text().contains("## Next"), "a passing pr drops the step: {}", text());
}

/// A layer released while the forge merges keeps `## Released` as its closing section: `land` writes no `## Next`
/// on a claim it no longer holds (13, Claim).
#[test]
fn land_notes_no_step_on_a_layer_released_during_the_merge() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let Stack { claim, .. } = stacked(v, t);
    let release = format!("'{}' release {B} --reason escalated --json > /dev/null\n", env!("CARGO_BIN_EXE_bp"));
    std::fs::write(t.join("on-merge"), release).unwrap();
    let (status, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((status, &out["stranded"]), (Some(0), &json!([B])), "{out}");
    let text = std::fs::read_to_string(v.join(format!("{claim}.md"))).unwrap();
    assert!(text.ends_with("reason: escalated\n") && !text.contains("## Next"), "{text}");
    assert_eq!(git(v, &["status", "--porcelain"]), "");
}

/// A layer that moves onto the base needs a worker's `bp pr` and then its verify review. The session reads `next`, so
/// it names the dispatch, not the old reviewer's approval (review of #100, C7, F12 and T5).
#[test]
fn a_restacked_layer_names_the_worker_s_pr_and_its_verify_review() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    stacked(v, t);
    // A squash that writes `f.txt` as A did, so B's rebase onto it applies.
    let code = t.join("code");
    std::fs::write(code.join("hello.txt"), "hi\n").unwrap();
    std::fs::write(code.join("f.txt"), "a\n").unwrap();
    let squash = commit(&code, &["add", "."], "squash of a");
    git(&code, &["reset", "-q", "--hard", "HEAD~1"]);
    std::fs::write(t.join("squash"), &squash).unwrap();
    let (status, out) = bp(v, t, &[], &["land", ISSUE]);
    assert_eq!((status, &out["restacked"]), (Some(0), &json!([B])), "{out}");
    assert_eq!(out["next"], json!([format!("dispatch a worker to run `bp pr {B}`; then its verify review")]), "{out}");
}

/// `bp record review` of [`ISSUE`] at `head` with `fields` and one row per item of `rows`.
fn review(v: &Path, t: &Path, head: &str, fields: &[&str], rows: &[&str]) -> (Option<i32>, Value) {
    let file = t.join("review.md");
    let table = format!("head: {head}\n\n| severity | location | finding |\n|---|---|---|\n{}\n", rows.join("\n"));
    std::fs::write(&file, table).unwrap();
    let head = format!("head={head}");
    let mut args = vec!["record", "review", ISSUE, "--field", &head];
    for f in fields {
        args.extend(["--field", f]);
    }
    args.extend(["--body-file", file.to_str().unwrap()]);
    bp(v, t, &[], &args)
}

/// The intent and note of [`ISSUE`] as `bp list issue` reads them.
fn intent(v: &Path) -> (Value, Value) {
    let (_, list) = common::bp_env(v, &[("PATH", "/usr/bin:/bin")], &["list", "issue"]);
    let row = list["rows"].as_array().unwrap().iter().find(|r| r["id"] == ISSUE).unwrap().clone();
    (row["intent"].clone(), row["note"].clone())
}

/// `pr` at the worktree's head, and the forge's pull request there, green; the repeat refreshes the snapshot.
fn pushed(v: &Path, t: &Path, head: &str) {
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert!(matches!(code, Some(0 | 3)), "{out}");
    forge(t, head, &["SUCCESS"]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "the repeat refreshes the snapshot");
}

/// A branch reset to a reviewed head is a new head: its error row, cleared by a later round at another head, is read
/// again, and the open pull request's head takes the review (review of #98, row 30).
#[test]
fn a_branch_reset_to_a_reviewed_head_needs_a_verify_review() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, out) = bp(v, t, &[], &["cut", ISSUE]);
    let work = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(work.join("hello.txt"), "hi\n").unwrap();
    let first = commit(&work, &["add", "hello.txt"], "hello");
    pushed(v, t, &first);
    let one = |lens: &str| ["round=1".to_string(), format!("lens={lens}"), "mode=full".into()];
    let (code, got) =
        review(v, t, &first, &one("conformance").each_ref().map(String::as_str), &["| error | hello.txt:1 | Wrong. |"]);
    assert_eq!(code, Some(0), "{got}");
    assert_eq!(review(v, t, &first, &one("failure-paths").each_ref().map(String::as_str), &[]).0, Some(0));
    let id = got["id"].as_str().unwrap();
    let (code, answer) = bp(v, t, &[], &["record", "disposition", id, "--field", "finding=1", "--field", "as=fixed"]);
    assert_eq!(code, Some(0), "{answer}");
    std::fs::write(work.join("hello.txt"), "hello\n").unwrap();
    let fixed = commit(&work, &["add", "hello.txt"], "fix");
    pushed(v, t, &fixed);
    assert_eq!(review(v, t, &fixed, &["round=2", "lens=verify", "mode=verify"], &[]).0, Some(0));
    assert_eq!(intent(v), (json!("needs-landing"), json!("")), "the verify round cleared the error at the fix");
    git(&work, &["reset", "-q", "--hard", &first]);
    // Until `pr` refreshes it, the forge's last view names the old head, and the refusal says so (review of #98, row
    // 37).
    let (code, got) = review(v, t, &first, &["round=3", "lens=verify", "mode=verify"], &[]);
    assert_eq!((code, got["code"].as_str()), (Some(2), Some("schema_invalid")), "{got}");
    assert!(got["fix"].as_str().unwrap().contains(&format!("the forge's last view names {fixed}")), "{got}");
    pushed(v, t, &first);
    assert_eq!(intent(v), (json!("needs-review"), json!("round 3: missing lenses: verify")));
    // The open pull request's head takes the review whatever its history; the error is found again there.
    let (code, got) =
        review(v, t, &first, &["round=3", "lens=verify", "mode=verify"], &["| error | hello.txt:1 | Still wrong. |"]);
    assert_eq!(code, Some(0), "{got}");
    // Round 3 is past medium's cap, so the error raises the issue to high, at its cap: the human decides.
    let cap = format!("round cap: the human abandons the issue, or accepts the open rows: bp approve {ISSUE}");
    assert_eq!(intent(v), (json!("blocked"), json!(cap)));
    // The head that the reset left behind is no head of the work (row 33).
    let (code, got) = review(v, t, &fixed, &["round=3", "lens=verify", "mode=verify"], &[]);
    assert_eq!((code, got["code"].as_str()), (Some(2), Some("finding_invalid")), "{got}");
}

/// A level that rises after round one adds the lenses of the new level that no review has read, beside the verify
/// review, whatever the round (review of #98, row 32).
#[test]
fn a_level_rise_after_round_one_adds_its_missing_lenses() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, out) = bp(v, t, &[], &["cut", ISSUE]);
    let work = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(work.join("hello.txt"), "hi\n").unwrap();
    let first = commit(&work, &["add", "hello.txt"], "hello");
    pushed(v, t, &first);
    for lens in ["conformance", "failure-paths"] {
        let field = format!("lens={lens}");
        assert_eq!(review(v, t, &first, &["round=1", &field, "mode=full"], &[]).0, Some(0));
    }
    std::fs::write(work.join("hello.txt"), "hello\n").unwrap();
    let fixed = commit(&work, &["add", "hello.txt"], "fix");
    pushed(v, t, &fixed);
    assert_eq!(review(v, t, &fixed, &["round=2", "lens=verify", "mode=verify"], &[]).0, Some(0));
    assert_eq!(intent(v), (json!("needs-landing"), json!("")), "medium needs no approval");
    // Round three touches a high path: the verify review, and the two high lenses no review has read.
    std::fs::create_dir_all(work.join(".github/workflows")).unwrap();
    std::fs::write(work.join(".github/workflows/x.yml"), "on: push\n").unwrap();
    let high = commit(&work, &["add", "-A"], "workflow");
    pushed(v, t, &high);
    let note = "round 3: missing lenses: verify, tests-and-checks, security";
    assert_eq!(intent(v), (json!("needs-review"), json!(note)));
    // Each missing lens records at the head in its round as a full review; a lens review in verify mode refuses.
    let (code, got) = review(v, t, &high, &["round=3", "lens=security", "mode=verify"], &[]);
    assert_eq!((code, got["code"].as_str()), (Some(2), Some("schema_invalid")), "{got}");
    assert!(got["fix"].as_str().unwrap().ends_with("--field round=3 --field mode=full"), "{got}");
    assert_eq!(review(v, t, &high, &["round=3", "lens=verify", "mode=verify"], &[]).0, Some(0));
    assert_eq!(intent(v), (json!("needs-review"), json!("round 3: missing lenses: tests-and-checks, security")));
    for lens in ["tests-and-checks", "security"] {
        let field = format!("lens={lens}");
        let (code, got) = review(v, t, &high, &["round=3", &field, "mode=full"], &[]);
        assert_eq!(code, Some(0), "{lens}: {got}");
    }
    let note = format!("the human runs bp approve {ISSUE} in a terminal");
    assert_eq!(intent(v), (json!("needs-approval"), json!(note)));
}

/// Replaces the `## Check` section of [`ISSUE`] with `blocks` and commits the vault.
fn checks(v: &Path, blocks: &str) {
    let path = v.join(format!("{ISSUE}.md"));
    let old = std::fs::read_to_string(&path).unwrap();
    let (head, _) = old.split_once("## Check\n").unwrap();
    std::fs::write(&path, format!("{head}## Check\n\n{blocks}")).unwrap();
    git(v, &["commit", "-qam", "checks"]);
}

/// Red-first (37, design change 10): `pr` runs each error block at the claim's base, in a checkout it removes, and
/// refuses `check_vacuous` before the push when one passes there. The base run is a run record at the base commit, so
/// a second `pr` runs nothing there, and a failed base run blocks nothing: the intents read runs at the head.
#[test]
fn pr_refuses_check_that_passes_at_base() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    checks(v, "```check shell {name=hello expect=0}\ntest -f README.md\n```\n");
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");
    let before = commits(v).parse::<u32>().unwrap();
    let worktrees = || git(&t.join("code"), &["worktree", "list", "--porcelain"]).matches("worktree ").count();

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_vacuous")), "{out}");
    let run = format!("{ISSUE}/runs/hello-{base}-1");
    assert_eq!(out["subject"], run.as_str(), "{out}");
    let fix = out["fix"].as_str().unwrap();
    assert!(fix.starts_with(&format!("hello passed at the base {base}")) && fix.contains("prior=pass"), "{fix}");
    assert!(fix.contains(&format!("bp release {ISSUE} --reason escalated")), "the worker's way out: {fix}");
    let (_, shown) = common::bp(v, &["show", &run]);
    assert_eq!((shown["verdict"].as_str(), shown["commit"].as_str()), (Some("pass"), Some(base.as_str())), "{shown}");
    assert!(shown["risk"].is_null(), "the level is of the head, not of a base run: {shown}");
    assert_eq!(commits(v).parse::<u32>().unwrap(), before + 1, "the base runs, in one commit");
    assert!(!t.join("created").exists(), "no pull request");
    assert_eq!(git(&t.join("origin.git"), &["branch", "--list", "bp/a"]), "", "no push");
    assert_eq!(worktrees(), 2, "the clone and the claim's worktree: the base checkout is gone");
    assert_eq!(git(&w, &["status", "--porcelain"]), "", "the claim's worktree is untouched");

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["subject"].as_str()), (Some(2), Some(run.as_str())), "{out}");
    assert_eq!(commits(v).parse::<u32>().unwrap(), before + 1, "a known base run does not run again");

    // The block now fails without the change: `pr` passes, and its failed base run blocks nothing.
    checks(v, "```check shell {name=hello expect=0}\ntest -f hello.txt\n```\n");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["runs"][0]["verdict"].as_str()), (Some(0), Some("pass")), "{out}");
    let (_, red) = common::bp(v, &["show", &format!("{ISSUE}/runs/hello-{base}-2")]);
    assert_eq!((red["verdict"].as_str(), red["commit"].as_str()), (Some("fail"), Some(base.as_str())), "{red}");
    let (_, at_head) = common::bp(v, &["show", out["runs"][0]["id"].as_str().unwrap()]);
    assert!(red["risk"].is_null() && at_head["risk"].is_string(), "{red} {at_head}");
    let head = out["head"].as_str().unwrap().to_string();
    assert_ne!(head, base);
    forge(t, &head, &[]);
    assert_eq!(bp(v, t, &[], &["pr", ISSUE]).0, Some(3), "the evidence at the head holds; nothing runs at the base");
    let (_, list) = common::bp_env(v, &[("PATH", "/usr/bin:/bin")], &["list", "issue"]);
    let row = list["rows"].as_array().unwrap().iter().find(|r| r["id"] == ISSUE).unwrap().clone();
    assert_eq!(row["intent"], "needs-review", "a failed base run is not a failed run at the head: {row}");
    assert_eq!(worktrees(), 2);
}

/// `prior=pass` marks a guard that holds before the change too: `pr` runs it at the head only, so it passes at the
/// base and nothing refuses. Any other value is a `check_malformed` finding and no way out: the block cannot run.
#[test]
fn pr_accepts_prior_pass() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let hello = "```check shell {name=hello expect=0}\ntest -f hello.txt\n```\n";
    checks(v, &format!("{hello}\n```check shell {{name=guard expect=0 prior=pass}}\ntest -f README.md\n```\n"));
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    let verdicts: Vec<(&str, &str)> = out["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap(), r["verdict"].as_str().unwrap()))
        .collect();
    assert_eq!((code, verdicts), (Some(0), vec![("hello", "pass"), ("guard", "pass")]), "{out}");
    let (_, red) = common::bp(v, &["show", &format!("{ISSUE}/runs/hello-{base}-1")]);
    assert_eq!(red["verdict"], "fail", "hello ran at the base: {red}");
    assert!(!v.join(format!("{ISSUE}/runs/guard-{base}-1.md")).exists(), "a guard runs at the head only");

    checks(v, &format!("{hello}\n```check shell {{name=guard expect=0 prior=yes}}\ntest -f README.md\n```\n"));
    let (_, shown) = common::bp(v, &["show", ISSUE]);
    assert!(shown.to_string().contains("prior=yes: the one value is pass"), "{shown}");
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_failed")), "{out}");
    assert!(out["fix"].as_str().unwrap().starts_with("guard could-not-run: prior=yes"), "{out}");
}

/// The red-first base is where the head leaves origin's target, found at each `pr`, not the claim's `base`: after the
/// rebase that `head_behind_base` orders, a block that the target's new commit makes pass refuses `check_vacuous`.
#[test]
fn pr_runs_red_first_where_the_rebased_head_leaves_the_target() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    checks(v, "```check shell {name=shared expect=0}\ntest -f shared.txt\n```\n");
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let cut_at = cut["base"].as_str().unwrap().to_string();
    let code = t.join("code");
    std::fs::write(code.join("shared.txt"), "on master\n").unwrap();
    let moved = commit(&code, &["add", "shared.txt"], "shared");
    git(&code, &["push", "-q", "origin", "master"]);
    std::fs::write(w.join("own.txt"), "mine\n").unwrap();
    commit(&w, &["add", "own.txt"], "own");

    let (code_, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code_, out["code"].as_str()), (Some(2), Some("head_behind_base")), "{out}");
    git(&w, &["rebase", "-q", "--onto", "origin/master", &cut_at]);
    let (code_, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code_, out["code"].as_str()), (Some(2), Some("check_vacuous")), "{out}");
    assert_eq!(out["subject"], format!("{ISSUE}/runs/shared-{moved}-1"), "the base moved with the rebase: {out}");
    assert!(!v.join(format!("{ISSUE}/runs/shared-{cut_at}-1.md")).exists(), "nothing ran at the cut's commit");
}

/// A claim's `base` field is not read: a claim with none runs red-first where the head leaves the target too.
#[test]
fn pr_finds_the_red_first_base_of_a_claim_with_no_base() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    checks(v, "```check shell {name=hello expect=0}\ntest -f README.md\n```\n");
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    let claim = v.join(format!("{}.md", cut["claim"].as_str().unwrap()));
    let text = std::fs::read_to_string(&claim).unwrap();
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with("base:")).collect();
    assert_eq!(kept.len() + 1, text.lines().count(), "the claim had one base line: {text}");
    std::fs::write(&claim, kept.join("\n") + "\n").unwrap();
    git(v, &["commit", "-qam", "a converted claim"]);
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_vacuous")), "{out}");
    assert_eq!(out["subject"], format!("{ISSUE}/runs/hello-{base}-1"), "{out}");
}

/// Cargo judges an artifact fresh by file time, so a build directory that the base run shares with the head run hands
/// the head the base's build. The base run builds into its own directory, whatever `CARGO_TARGET_DIR` says.
#[test]
fn the_base_run_never_shares_a_build_directory_with_the_head() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let code = t.join("code");
    std::fs::write(code.join("src.txt"), "old\n").unwrap();
    commit(&code, &["add", "src.txt"], "src");
    git(&code, &["push", "-q", "origin", "master"]);
    // A build cached by file time: the output is rebuilt only when the source is newer.
    let build = r#"d=${CARGO_TARGET_DIR:-target}; mkdir -p "$d"; [ -f "$d/out" ] && [ ! src.txt -nt "$d/out" ] || cp src.txt "$d/out"; grep -q new "$d/out""#;
    checks(v, &format!("```check shell {{name=built expect=0}}\n{build}\n```\n"));
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    std::fs::write(w.join("src.txt"), "new\n").unwrap();
    commit(&w, &["add", "src.txt"], "new");

    let shared = t.join("shared");
    let env = [("CARGO_TARGET_DIR", shared.to_str().unwrap())];
    let (code_, out) = bp(v, t, &env, &["pr", ISSUE]);
    assert_eq!((code_, out["runs"][0]["verdict"].as_str()), (Some(0), Some("pass")), "{out}");
    assert_eq!(out["base_runs"][0]["verdict"], "fail", "{out}");
    assert_eq!(out["base_runs"][0]["id"], format!("{ISSUE}/runs/built-{base}-1"), "{out}");
    assert_eq!(std::fs::read_to_string(shared.join("out")).unwrap(), "new\n", "the shared directory holds the head's");
}

/// A base run that timed out or could not run is no proof that the block depends on the change, and no refusal: `pr`
/// names it in `base_runs` and in the pull request body, beside the blocks that failed there.
#[test]
fn pr_names_each_base_run_that_did_not_fail() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let hello = "```check shell {name=hello expect=0}\ntest -f hello.txt\n```\n";
    checks(v, &format!("{hello}\n```check shell {{name=inside expect=0 cwd=newdir}}\ntest -f x\n```\n"));
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    std::fs::create_dir(w.join("newdir")).unwrap();
    std::fs::write(w.join("newdir/x"), "x\n").unwrap();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt", "newdir/x"], "hello");

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!(code, Some(0), "a base run that could not run refuses nothing: {out}");
    let rows: Vec<(&str, &str)> = out["base_runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap(), r["verdict"].as_str().unwrap()))
        .collect();
    assert_eq!(rows, [("hello", "fail"), ("inside", "could-not-run")], "{out}");
    let body = std::fs::read_to_string(t.join("body")).unwrap();
    let listed = format!("- `inside`: could-not-run; `bp show {ISSUE}/runs/inside-{base}-1`");
    assert!(body.contains("## Red-first runs that did not fail") && body.contains(&listed), "{body}");
    assert!(!body.contains("`hello`"), "a block that failed at the base proves its change: {body}");
}

/// Red-first reads the latest attempt at the base: a block that failed there once and then passed refuses.
#[test]
fn pr_judges_the_latest_attempt_at_the_base() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    checks(v, "```check shell {name=hello expect=0}\ntest -n \"$FLAG\"\n```\n");
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    // Before the first commit the worktree's head is the base, so `check` runs there.
    assert_eq!(bp(v, t, &[], &["check", ISSUE]).1["runs"][0]["verdict"], "fail");
    assert_eq!(bp(v, t, &[("FLAG", "1")], &["check", ISSUE]).1["runs"][0]["verdict"], "pass");
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");

    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("check_vacuous")), "{out}");
    assert_eq!(out["subject"], format!("{ISSUE}/runs/hello-{base}-2"), "{out}");
}

/// A base checkout as `pr` makes one: detached, in `dir`, locked with the reason that names `pid`.
fn base_checkout(code: &Path, dir: &Path, pid: u32, base: &str) {
    let reason = format!("bp red-first base, pid {pid}");
    git(code, &["worktree", "add", "-q", "--detach", "--lock", "--reason", &reason, dir.to_str().unwrap(), base]);
}

/// A pid that no process holds: a child's, after it exits.
fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// A signal runs no drop, so a killed `pr` leaves its base checkout. The next `pr` removes each one whose `pr` is dead
/// and never touches one whose `pr` is alive, nor one that git does not hold locked as a base checkout.
#[test]
fn pr_sweeps_the_base_checkouts_of_a_killed_pr() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");
    let tmp = t.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let pid = dead_pid();
    let (dead, live, unlocked) = (
        tmp.join(format!("bp-base-{pid}-1")),
        tmp.join(format!("bp-base-{}-2", std::process::id())),
        tmp.join(format!("bp-base-{pid}-3")),
    );
    let code = t.join("code");
    base_checkout(&code, &dead, pid, &base);
    base_checkout(&code, &live, std::process::id(), &base);
    git(&code, &["worktree", "add", "-q", "--detach", unlocked.to_str().unwrap(), &base]);

    let (code_, out) = bp(v, t, &[("TMPDIR", tmp.to_str().unwrap())], &["pr", ISSUE]);
    assert_eq!(code_, Some(0), "{out}");
    assert_eq!(out["base_checkouts_left"], json!([]), "{out}");
    let listed = git(&code, &["worktree", "list", "--porcelain"]);
    assert!(!dead.exists() && !listed.contains(dead.file_name().unwrap().to_str().unwrap()), "{listed}");
    for kept in [&live, &unlocked] {
        assert!(kept.is_dir() && listed.contains(kept.file_name().unwrap().to_str().unwrap()), "{listed}");
    }
    let left: BTreeSet<_> = std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().file_name()).collect();
    let kept = BTreeSet::from([live.file_name().unwrap().to_owned(), unlocked.file_name().unwrap().to_owned()]);
    assert_eq!(left, kept, "this pr's own checkout is gone too");
}

/// A claim's worktree named like a base checkout, in the directory `pr` makes checkouts in and locked with a base
/// checkout's reason and a dead pid, survives another issue's `pr` with its uncommitted work: it carries a branch.
#[test]
fn pr_keeps_a_claim_worktree_named_like_a_base_checkout() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let slug = "bp-base-99999999-quokka";
    let quokka = format!("projects/bp/issues/{slug}");
    let text = "---\nkind: issue\ntitle: Q\nstatus: open\nunder: projects/bp/milestones/m\n---\n\nBody.\n\n## Check\n\n```check shell {name=q expect=0}\ntest -f q.txt\n```\n";
    std::fs::write(v.join(format!("{quokka}.md")), text).unwrap();
    git(v, &["add", "-A"]);
    git(v, &["commit", "-qm", "quokka"]);
    let (code_, cut) = bp(v, t, &[], &["cut", &quokka]);
    assert_eq!(code_, Some(0), "{cut}");
    let q = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    assert_eq!(q.file_name().unwrap(), slug);
    std::fs::write(q.join("work.txt"), "uncommitted\n").unwrap();
    let reason = format!("bp red-first base, pid {}", dead_pid());
    git(&t.join("code"), &["worktree", "lock", "--reason", &reason, q.to_str().unwrap()]);
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");

    let (code_, out) = bp(v, t, &[("TMPDIR", q.parent().unwrap().to_str().unwrap())], &["pr", ISSUE]);
    assert_eq!(code_, Some(0), "{out}");
    assert_eq!(std::fs::read_to_string(q.join("work.txt")).unwrap(), "uncommitted\n");
    let listed = git(&t.join("code"), &["worktree", "list", "--porcelain"]);
    assert!(listed.contains(&format!("worktree {}\n", q.canonicalize().unwrap().display())), "{listed}");
}

/// A clone whose directory is named like a base checkout, in the directory `pr` makes checkouts in, survives its own
/// `pr` with its uncommitted work: it is the first entry git lists, and git holds it unlocked.
#[test]
fn pr_keeps_a_clone_named_like_a_base_checkout() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let clone = t.join("bp-base-99999999-code");
    std::fs::rename(t.join("code"), &clone).unwrap();
    std::os::unix::fs::symlink(&clone, t.join("code")).unwrap();
    std::fs::write(clone.join("work.txt"), "uncommitted\n").unwrap();
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");

    let (code_, out) = bp(v, t, &[("TMPDIR", t.to_str().unwrap())], &["pr", ISSUE]);
    assert_eq!(code_, Some(0), "{out}");
    assert!(clone.join(".git").is_dir(), "the clone holds its git directory");
    assert_eq!(std::fs::read_to_string(clone.join("work.txt")).unwrap(), "uncommitted\n");
}

/// A red-first base that git cannot fetch refuses `could_not_run`, and the fix names the commit.
#[test]
fn pr_names_a_base_it_cannot_fetch() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, cut) = bp(v, t, &[], &["cut", ISSUE]);
    let w = Path::new(cut["worktree"].as_str().unwrap()).to_path_buf();
    let base = cut["base"].as_str().unwrap().to_string();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    commit(&w, &["add", "hello.txt"], "hello");
    // A git that refuses a fetch by sha, as a server does for a commit it no longer holds.
    let real = std::process::Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let real = String::from_utf8(real.stdout).unwrap();
    let fake = t.join("fake");
    std::fs::create_dir(&fake).unwrap();
    let script = format!(
        "#!/bin/sh\ncase \"$*\" in *\" fetch -q origin \"[0-9a-f]*) echo 'fatal: not our ref' >&2; exit 128 ;; esac\nexec {} \"$@\"\n",
        real.trim()
    );
    std::fs::write(fake.join("git"), script).unwrap();
    std::fs::set_permissions(fake.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}:{}", fake.display(), t.join("bin").display(), std::env::var("PATH").unwrap());

    let (code, out) = bp(v, t, &[("PATH", &path)], &["pr", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    let fix = out["fix"].as_str().unwrap();
    assert!(fix.starts_with(&format!("git cannot fetch {base}, origin's master")), "{fix}");
    assert!(!t.join("created").exists(), "no pull request");
}

/// The lenses of a high head that touches a high path.
const HIGH: [&str; 4] = ["conformance", "failure-paths", "tests-and-checks", "security"];

/// Commits `.bp/risk.yml` holding `text` on origin's master.
fn rules(t: &Path, text: &str) {
    let code = t.join("code");
    git(&code, &["fetch", "-q", "origin"]);
    git(&code, &["reset", "-q", "--hard", "origin/master"]);
    std::fs::create_dir_all(code.join(".bp")).unwrap();
    std::fs::write(code.join(".bp/risk.yml"), text).unwrap();
    commit(&code, &["add", ".bp/risk.yml"], "risk");
    git(&code, &["push", "-q", "origin", "master"]);
}

/// The keys `human` and `agent`, with no passphrase, an `ssh-add` that lists no key, and origin's `.bp/risk.yml`:
/// `hello.txt` is high and the human's key signs. The path of the human's private key.
fn signer(t: &Path) -> String {
    for key in ["human", "agent"] {
        let out = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", key, "-f"])
            .arg(t.join(key))
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    let ssh_add = t.join("bin/ssh-add");
    std::fs::write(&ssh_add, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&ssh_add, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let public = std::fs::read_to_string(t.join("human.pub")).unwrap();
    rules(t, &format!("high:\n  - hello.txt\nsigner: {public}"));
    t.join("human").display().to_string()
}

/// Adds `line` to the header of [`ISSUE`] by hand, and commits the vault.
fn label(v: &Path, line: &str) {
    let path = v.join(format!("{ISSUE}.md"));
    let text = std::fs::read_to_string(&path).unwrap().replacen(
        "status: in-progress\n",
        &format!("status: in-progress\n{line}\n"),
        1,
    );
    std::fs::write(&path, text).unwrap();
    git(v, &["commit", "-qam", "by hand"]);
}

/// [`setup`] with [`ISSUE`] declared low while [`M`] is planned, then [`M`] approved: the one way a label lowers
/// (37, design change 1).
fn setup_low() -> (tempfile::TempDir, tempfile::TempDir) {
    let (v, t) = setup_under("planned");
    assert_eq!(common::bp(v.path(), &["set", ISSUE, "risk", "low"]).0, Some(0));
    let (code, out) = common::bp(v.path(), &["transition", M, "approved"]);
    assert_eq!(code, Some(0), "{out}");
    (v, t)
}

/// `land` gates by level (37, design change 3): a low or a medium head lands on green checks, a passing run of each
/// block and the reviews of its round. The session runs it; no human act, and no key is read.
#[test]
fn land_low_and_medium_need_no_approval() {
    for (level, lens) in [("low", &["conformance"][..]), ("medium", &MEDIUM[..])] {
        let (v, t) = if level == "low" { setup_low() } else { setup() };
        let (v, t) = (v.path(), t.path());
        // A signer the forge confirms, so a land that read it would ask the forge's default head.
        signer(t);
        let public = std::fs::read_to_string(t.join("human.pub")).unwrap();
        let low = if level == "low" { "low:\n  - hello.txt\n" } else { "" };
        rules(t, &format!("{low}signer: {public}"));
        let (_, out) = bp(v, t, &[], &["cut", ISSUE]);
        let w = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
        std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
        let h = commit(&w, &["add", "hello.txt"], "hello");
        pushed(v, t, &h);
        lenses(v, t, &h, lens);
        assert_eq!(intent(v), (json!("needs-landing"), json!("")), "{level}");
        let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
        let got = (code, out["status"].as_str(), out["level"].as_str());
        assert_eq!(got, (Some(0), Some("resolved"), Some(level)), "{out}");
        assert!(t.join("merged").exists() && !v.join(format!("{ISSUE}/approvals")).exists(), "{level}");
        assert!(!t.join("graphql").exists(), "{level}: land reads no signer below high");
    }
}

/// A label written by hand after the milestone's approval and the claim lowers nothing (37, design change 1; review of
/// #100, C6 and T1): `land`, the intents and `bp risk` read the higher label it replaced, so medium to low still needs
/// both medium lenses, and high to medium still needs the third lens and the human's signed approval.
#[test]
fn a_label_lowered_by_hand_reads_as_the_one_it_replaced() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    rules(t, "low:\n  - hello.txt\n");
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &["conformance"]);
    assert_eq!(common::bp(v, &["set", ISSUE, "risk", "low"]).1["code"], "not_a_label");
    label(v, "risk: low");
    let (_, risk) = bp(v, t, &[], &["risk", ISSUE]);
    assert_eq!(
        (&risk["level"], &risk["declared"], &risk["computed"]),
        (&json!("medium"), &json!("medium"), &json!("low"))
    );
    assert_eq!(intent(v), (json!("needs-review"), json!("round 1: missing lenses: failure-paths")));
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE, "--dry-run"]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("review_missing")), "{out}");

    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    signer(t);
    let public = std::fs::read_to_string(t.join("human.pub")).unwrap();
    rules(t, &format!("signer: {public}"));
    let (_, h) = opened(v, t, &[]);
    assert_eq!(common::bp(v, &["set", ISSUE, "risk", "high"]).0, Some(0), "a label rises at any time");
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    let path = v.join(format!("{ISSUE}.md"));
    std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("risk: high\n", "")).unwrap();
    git(v, &["commit", "-qam", "by hand"]);
    assert_eq!(bp(v, t, &[], &["risk", ISSUE]).1["level"], "high");
    assert_eq!(intent(v), (json!("needs-review"), json!("round 1: missing lenses: tests-and-checks")));
    lenses(v, t, &h, &["tests-and-checks"]);
    assert_eq!(intent(v), (json!("needs-approval"), json!(format!("the human runs bp approve {ISSUE} in a terminal"))));
    let (code, out) = bp(v, t, &[("BP_ROLE", "session"), ("SSH_AUTH_SOCK", "")], &["land", ISSUE, "--dry-run"]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("approval_missing")), "{out}");
}

/// A label no commit holds reads as the committed one, whoever reads it: `bp risk` and the intents agree with `land`
/// (review of #100, V5).
#[test]
fn a_label_no_commit_holds_is_no_label() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    rules(t, "low:\n  - hello.txt\n");
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &["conformance"]);
    let path = v.join(format!("{ISSUE}.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replacen("status: in-progress\n", "status: in-progress\nrisk: low\n", 1)).unwrap();
    let (_, risk) = bp(v, t, &[], &["risk", ISSUE]);
    assert_eq!((&risk["level"], &risk["declared"]), (&json!("medium"), &json!("medium")), "{risk}");
    assert_eq!(intent(v), (json!("needs-review"), json!("round 1: missing lenses: failure-paths")));
}

/// A high head lands only on the human's signed approval of that head, of that pull request, into that base (37,
/// design changes 3 and 4). The intents read an approval record as it stands; `land` verifies it, so a hand-written
/// label, a record of another pull request or base, a changed field and an approval of an earlier head refuse.
#[test]
fn land_high_refuses_without_signed_review() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let human = signer(t);
    let code = t.join("code");
    git(&code, &["push", "-q", "origin", "master:refs/heads/release"]);
    let (w, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &HIGH);
    let note = format!("the human runs bp approve {ISSUE} in a terminal");
    assert_eq!(intent(v), (json!("needs-approval"), json!(note)));
    let refused = |want: &str, why: &str| {
        let before = commits(v);
        let (code, out) = bp(v, t, &[("BP_ROLE", "session"), ("SSH_AUTH_SOCK", "")], &["land", ISSUE]);
        assert_eq!((code, out["code"].as_str()), (Some(2), Some(want)), "{why}: {out}");
        assert_eq!(commits(v), before, "{why}: a refusal writes nothing");
        assert!(!t.join("merged").exists(), "{why}: no refusal merges");
        out["fix"].as_str().unwrap().to_string()
    };
    let fix = refused("approval_missing", "no approval");
    assert!(fix.starts_with(&format!("the human runs bp approve {ISSUE} from a shell, at head {h}")), "{fix}");
    // A label the agent writes by hand cannot lower a computed high.
    label(v, "risk: low");
    refused("approval_missing", "a hand-written label");
    let approve = || bp(v, t, &[("SSH_AUTH_SOCK", "")], &["approve", ISSUE, "--key", &human]);

    // The human signs pull request 2, and the forge names 1: the intents read the record, `land` refuses it.
    forge_as(t, &h, &["SUCCESS"], &json!({ "number": 2 }));
    assert_eq!(approve().0, Some(0));
    forge(t, &h, &["SUCCESS"]);
    assert_eq!(intent(v), (json!("needs-landing"), json!("")), "the intents read records unverified");
    refused("signature_invalid", "an approval of another pull request");
    // A changed field: the agent edits the record to name pull request 1; the signature no longer holds.
    let record = v.join(format!("{ISSUE}/approvals/{h}.md"));
    let text = std::fs::read_to_string(&record).unwrap();
    std::fs::write(&record, text.replace("pull_request: 2\n", "pull_request: 1\n")).unwrap();
    git(v, &["commit", "-qam", "a forged field"]);
    refused("signature_invalid", "a hand-edited record");
    // The human signs the base release, and the forge names master.
    forge_as(t, &h, &["SUCCESS"], &json!({ "baseRefName": "release" }));
    assert_eq!(approve().0, Some(0));
    forge(t, &h, &["SUCCESS"]);
    refused("signature_invalid", "an approval of another base");

    // An approval of this head, then a push: the approval of an earlier head never counts.
    assert_eq!(approve().0, Some(0));
    let (_, out) = bp(v, t, &[("SSH_AUTH_SOCK", "")], &["land", ISSUE, "--dry-run"]);
    assert_eq!((&out["head"], &out["level"]), (&json!(h), &json!("high")), "{out}");
    git(&w, &["commit", "--amend", "-qm", "hello, again"]);
    let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
    assert_eq!(code, Some(0), "{out}");
    let h2 = out["head"].as_str().unwrap().to_string();
    pushed(v, t, &h2);
    assert_eq!(review(v, t, &h2, &["round=2", "lens=verify", "mode=verify"], &[]).0, Some(0));
    let fix = refused("approval_missing", "an approval of an earlier head");
    assert!(fix.ends_with(&format!("; the approvals sign {h}, {h}, {h}")), "{fix}");
    let (code, out) = approve();
    assert_eq!(code, Some(0), "{out}");
    // The signer counts only at the default branch's head that the forge names (review of #95, row 24).
    std::fs::write(t.join("default-oid"), "0".repeat(40)).unwrap();
    let fix = refused("signer_missing", "origin answers another sha than the forge");
    assert!(fix.starts_with(&format!("the forge's master is at {}", "0".repeat(40))), "{fix}");
    std::fs::remove_file(t.join("default-oid")).unwrap();
    let dry = || bp(v, t, &[("SSH_AUTH_SOCK", "")], &["land", ISSUE, "--dry-run"]);
    let (code, out) = dry();
    assert_eq!((code, &out["level"]), (Some(0), &json!("high")), "{out}");
    // A lowering counts at land only for the forge's pull request and base, signed, by the signer the forge confirms
    // (review of #100, T2). One signed for pull request 2 lowers nothing; edited to name 1, it no longer verifies.
    let lower = || bp(v, t, &[("SSH_AUTH_SOCK", "")], &["lower", ISSUE, "medium", "--reason", "docs", "--key", &human]);
    forge_as(t, &h2, &["SUCCESS"], &json!({ "number": 2 }));
    assert_eq!(lower().0, Some(0));
    forge(t, &h2, &["SUCCESS"]);
    assert_eq!(bp(v, t, &[], &["risk", ISSUE]).1["level"], "medium", "bp risk reads the record's own pull request");
    assert_eq!(dry().1["level"], "high", "a lowering of another pull request");
    let lowering = std::fs::read_dir(v.join(format!("{ISSUE}/lowerings"))).unwrap().next().unwrap().unwrap().path();
    let text = std::fs::read_to_string(&lowering).unwrap();
    std::fs::write(&lowering, text.replace("pull_request: 2\n", "pull_request: 1\n")).unwrap();
    git(v, &["commit", "-qam", "a forged lowering"]);
    assert_eq!(dry().1["level"], "high", "a lowering edited to name the forge's pull request");
    // The human's signed lowering of this head counts at land: the head lands at medium.
    let (code, out) =
        bp(v, t, &[("SSH_AUTH_SOCK", "")], &["lower", ISSUE, "medium", "--reason", "docs", "--key", &human]);
    assert_eq!(code, Some(0), "{out}");
    let (_, out) = bp(v, t, &[], &["risk", ISSUE]);
    assert_eq!(out["level"], "medium", "bp risk reads the signed lowering of this head: {out}");
    assert!(out["reasons"].to_string().contains("the human's signed lowering of this head sets medium"), "{out}");
    std::fs::write(t.join("default-oid"), "0".repeat(40)).unwrap();
    assert_eq!(dry().1["code"], "signer_missing", "a lowering's signer counts only at the forge's default head");
    std::fs::remove_file(t.join("default-oid")).unwrap();
    let (code, out) = bp(v, t, &[("BP_ROLE", "session"), ("SSH_AUTH_SOCK", "")], &["land", ISSUE]);
    let got = (code, out["status"].as_str(), out["level"].as_str());
    assert_eq!(got, (Some(0), Some("resolved"), Some("medium")), "{out}");
}

/// `land` judges the level of the head after its own fetch, and refuses one above the level `pr` recorded there (37,
/// design change 1): a rule that changed on origin, or a run record edited by hand to read low. A merged pull request
/// is judged by the rules at the base it merged into, its merge commit's first parent (review of #100, F8 and T4).
#[test]
fn land_refuses_a_level_that_rose() {
    let (v, t) = setup_low();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    // The issue declares low; the agent edits each run to read low too: the intents read low and ask for one lens.
    let runs: Vec<_> = std::fs::read_dir(v.join(format!("{ISSUE}/runs"))).unwrap().map(|e| e.unwrap().path()).collect();
    let texts: Vec<String> = runs.iter().map(|r| std::fs::read_to_string(r).unwrap()).collect();
    for (run, text) in runs.iter().zip(&texts) {
        std::fs::write(run, text.replace("risk: medium\n", "risk: low\n")).unwrap();
    }
    git(v, &["commit", "-qam", "forged runs"]);
    lenses(v, t, &h, &["conformance"]);
    assert_eq!(intent(v), (json!("needs-landing"), json!("")), "the intents read the records as they stand");
    let land = || bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("evidence_stale")), "{out}");
    let fix = out["fix"].as_str().unwrap();
    assert!(fix.starts_with(&format!("the level at {h} rose from low to medium since pr recorded it")), "{fix}");

    // Origin's rules now read `hello.txt` high, after `pr` recorded medium.
    for (run, text) in runs.iter().zip(&texts) {
        std::fs::write(run, text).unwrap();
    }
    git(v, &["commit", "-qam", "the runs as pr wrote them"]);
    lenses(v, t, &h, &["failure-paths"]);
    rules(t, "high:\n  - hello.txt\n");
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("evidence_stale")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains("rose from medium to high"), "{out}");
    assert!(!t.join("merged").exists(), "no refusal merges");
    // Merged outside `land` onto those rules, the head is judged at the merge's base: a lost merge, not a landed one.
    let before = commits(v);
    forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED" }));
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("evidence_stale")), "{out}");
    let lost = format!(
        "pull request 1 is merged, so nothing can meet this now and nothing is recorded; the human reviews the merge, \
         then bp release {ISSUE} --reason lost"
    );
    assert_eq!(out["fix"].as_str(), Some(lost.as_str()), "{out}");
    assert_eq!(commits(v), before, "a merged head whose level rose is not recorded as landed");
}

/// A head that `pr` recorded high with no high path refuses when its diff now touches one: round one never read the
/// security lens (37, design change 8).
#[test]
fn land_refuses_a_high_path_that_pr_did_not_record() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let human = signer(t);
    let public = std::fs::read_to_string(t.join("human.pub")).unwrap();
    rules(t, &format!("signer: {public}"));
    let (_, out) = bp(v, t, &[], &["cut", ISSUE]);
    label(v, "risk: high");
    let w = Path::new(out["worktree"].as_str().unwrap()).to_path_buf();
    std::fs::write(w.join("hello.txt"), "hi\n").unwrap();
    let h = commit(&w, &["add", "hello.txt"], "hello");
    pushed(v, t, &h);
    lenses(v, t, &h, &HIGH[..3]);
    assert_eq!(bp(v, t, &[("SSH_AUTH_SOCK", "")], &["approve", ISSUE, "--key", &human]).0, Some(0));
    rules(t, &format!("high:\n  - hello.txt\nsigner: {public}"));
    let (code, out) = bp(v, t, &[("BP_ROLE", "session"), ("SSH_AUTH_SOCK", "")], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("evidence_stale")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains(&format!("the level at {h} touches a high path")), "{out}");
    assert!(!t.join("merged").exists(), "no refusal merges");
}

/// An open round holds the land: a finding with no disposition, then an error row that a decline does not clear (37,
/// design changes 3 and 6).
#[test]
fn land_refuses_an_open_round() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    let (code, got) =
        review(v, t, &h, &["round=1", "lens=conformance", "mode=full"], &["| error | hello.txt:1 | Wrong. |"]);
    assert_eq!(code, Some(0), "{got}");
    lenses(v, t, &h, &["failure-paths"]);
    let land = || bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("findings_open")), "{out}");
    assert!(out["fix"].as_str().unwrap().starts_with("no disposition: "), "{out}");
    let id = got["id"].as_str().unwrap();
    let reason = "reason=the reviewer misread the file";
    let (code, answer) = bp(
        v,
        t,
        &[],
        &["record", "disposition", id, "--field", "finding=1", "--field", "as=declined", "--field", reason],
    );
    assert_eq!(code, Some(0), "{answer}");
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("findings_open")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains(" hold the land; dispatch a worker to fix each"), "{out}");
    let (intent, note) = intent(v);
    assert_eq!(intent, "blocked", "{note}");
    assert!(note.as_str().unwrap().ends_with("; the human decides"), "{note}");
    assert!(!t.join("merged").exists(), "no refusal merges");
}

/// At high's round cap the human's signed approval of the head is the disposition `accepted` of the open rows it names
/// (37, design change 6; review of #100, V1): the record names them, `land` proceeds, and no worker is dispatched, since
/// a fix would push a head that voids the approval.
#[test]
fn land_at_high_s_round_cap_takes_the_human_s_approval_as_the_disposition() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let human = signer(t);
    let (w, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &HIGH);
    // Round 2 holds an error row, answered `fixed`; round 3, at the fixed head, holds another: high's cap.
    let verify = |head: &str, round: &str| {
        let (code, got) =
            review(v, t, head, &[round, "lens=verify", "mode=verify"], &["| error | hello.txt:1 | Wrong. |"]);
        assert_eq!(code, Some(0), "{got}");
        got["id"].as_str().unwrap().to_string()
    };
    let next = |message: &str| {
        git(&w, &["commit", "--amend", "-qm", message]);
        let (code, out) = bp(v, t, &[], &["pr", ISSUE]);
        assert_eq!(code, Some(0), "{out}");
        let head = out["head"].as_str().unwrap().to_string();
        pushed(v, t, &head);
        head
    };
    let h2 = next("again");
    let second = verify(&h2, "round=2");
    let fixed = ["record", "disposition", &second, "--field", "finding=1", "--field", "as=fixed"];
    assert_eq!(bp(v, t, &[], &fixed).0, Some(0));
    let h3 = next("and again");
    let third = verify(&h3, "round=3");
    let cap = format!("round cap: the human abandons the issue, or accepts the open rows: bp approve {ISSUE}");
    assert_eq!(intent(v), (json!("blocked"), json!(cap)));
    let session = [("BP_ROLE", "session"), ("SSH_AUTH_SOCK", "")];
    let land = |args: &[&str]| bp(v, t, &session, &[&["land", ISSUE][..], args].concat());
    // A record the agent writes by hand, naming the row, signs nothing: the intents read it, `land` does not.
    let forged = format!(
        "---\nkind: approval\nunder: {ISSUE}\nhead: {h3}\npull_request: 1\nbase: master\naccepts:\n  - {third}#1\n---\n"
    );
    std::fs::create_dir_all(v.join(format!("{ISSUE}/approvals"))).unwrap();
    std::fs::write(v.join(format!("{ISSUE}/approvals/{h3}.md")), forged).unwrap();
    git(v, &["add", "."]);
    git(v, &["commit", "-qm", "a hand-written approval"]);
    let (code, out) = land(&["--dry-run"]);
    assert_eq!(
        (code, out["code"].as_str()),
        (Some(2), Some("findings_open")),
        "an unsigned record accepts nothing: {out}"
    );
    git(v, &["rm", "-q", "-r", "--", &format!("{ISSUE}/approvals")]);
    git(v, &["commit", "-qm", "the hand-written approval goes"]);
    assert_eq!(intent(v), (json!("blocked"), json!(cap)));
    let (code, out) = bp(v, t, &[("SSH_AUTH_SOCK", "")], &["approve", ISSUE, "--key", &human]);
    assert_eq!(code, Some(0), "{out}");
    let names = json!([format!("{third}#1")]);
    assert_eq!(out["accepts"], names, "the record names the rows it accepts: {out}");
    let text = std::fs::read_to_string(v.join(format!("{ISSUE}/approvals/{h3}.md"))).unwrap();
    assert!(text.contains(&format!("{third}#1")), "{text}");
    assert_eq!(intent(v), (json!("needs-landing"), json!("")), "no dispatch answers the accepted row");
    let (code, out) = bp(v, t, &[("SSH_AUTH_SOCK", "")], &["approve", ISSUE, "--key", &human]);
    assert_eq!(code, Some(3), "a repeat finds its record and names no row again: {out}");
    let (code, out) = land(&["--dry-run"]);
    assert_eq!((code, &out["level"]), (Some(0), &json!("high")), "{out}");
    let (code, out) = land(&[]);
    assert_eq!((code, out["status"].as_str()), (Some(0), Some("resolved")), "{out}");
}

/// Each refusal `land` can give, and each fix it names, is one the session can follow (review of #100, C3).
#[test]
fn every_land_fix_names_what_the_session_runs() {
    use bp_change::intents::{Run, Unmet};
    use bp_change::rounds::{Lens, Review, Row};
    let id = bp_vault::Id::parse(ISSUE).unwrap();
    let row = |state: &str| {
        let pr = json!({ "number": 1, "headRefName": "bp/a", "headRefOid": "a".repeat(40), "baseRefName": "side",
            "statusCheckRollup": [{ "conclusion": state }] });
        serde_json::from_value::<bp_forge::PullRequest>(pr).unwrap()
    };
    let run = Run { id: format!("{ISSUE}/runs/r"), check: "hello".into(), ..Run::default() };
    let review =
        Review { id: "r".into(), head: "a".repeat(40), round: 1, lens: Lens::Conformance, rows: vec![Row::default()] };
    for pr in [row("FAILURE"), row("")] {
        for unmet in [
            Unmet::Unmerged("projects/bp/issues/b"),
            Unmet::Mismatch("master"),
            Unmet::Checks,
            Unmet::Failed(&run),
            Unmet::Evidence("hello"),
            Unmet::Lenses { round: 1, missing: vec![Lens::Verify] },
            Unmet::Unanswered(vec![(&review, vec![1])]),
            Unmet::Holding(vec![(&review, vec![1])]),
            Unmet::Approval,
            Unmet::ChecksChanged,
        ] {
            session_can_follow(&bp_read::landing::refusal(&unmet, &id, &pr).fix);
        }
    }
    session_can_follow(&bp_read::landing::no_pull_request(&id).fix);
    session_can_follow(&bp_read::landing::after_merge(1, &id));
    for status in ["open", "abandoned", "in-progress", "resolved"] {
        session_can_follow(&bp_read::landing::unclaimed(&id, Some(status)));
    }
}

/// `land` judges the level by the base the forge names, once origin names the same sha (37, design change 1; review
/// of #100, C2). A `url.<x>.insteadOf` rule that points origin at a copy whose master drops the high pattern makes
/// `pr` record medium; `land` refuses at every level, before it judges one or reads a key.
#[test]
fn land_refuses_a_base_that_origin_and_the_forge_name_apart() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    rules(t, "high:\n  - hello.txt\n");
    let code = t.join("code");
    let copy = t.join("copy.git");
    git(t, &["clone", "-q", "--bare", "origin.git", "copy.git"]);
    std::fs::remove_file(code.join(".bp/risk.yml")).unwrap();
    commit(&code, &["add", "-A"], "no rules");
    git(&code, &["push", "-q", &copy.display().to_string(), "HEAD:refs/heads/master"]);
    git(&code, &["reset", "-q", "--hard", "HEAD~1"]);
    let rewrite = format!("url.{}.insteadOf", copy.display());
    git(&code, &["config", &rewrite, &t.join("origin.git").display().to_string()]);
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    assert_eq!(bp(v, t, &[], &["risk", ISSUE]).1["computed"], "medium", "origin answers the copy");
    assert_eq!(intent(v), (json!("needs-landing"), json!("")));
    for args in [&["land", ISSUE, "--dry-run"][..], &["land", ISSUE]] {
        let (code, out) = bp(v, t, &[("BP_ROLE", "session")], args);
        assert_eq!((code, out["code"].as_str()), (Some(2), Some("base_unconfirmed")), "{out}");
    }
    assert!(!t.join("merged").exists() && !t.join("graphql").exists(), "no merge, and no key read");
}

/// A forge row `land` cannot trust refuses before any git call reads it (review of #100, F4 and F5): a head or a base
/// sha that is not a full sha, as one that starts with `-` would be a git option, and an open pull request with no
/// base branch.
#[test]
fn land_refuses_a_forge_row_it_cannot_read() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    let pwn = t.join("pwn");
    forge_as(t, &format!("--output={}", pwn.display()), &["SUCCESS"], &json!({}));
    let (code, out) = bp(v, t, &[], &["land", ISSUE, "--dry-run"]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    assert!(!pwn.exists(), "no git call read the head");
    // The forge's base sha is held to the same shape before `ls-remote` is compared with it.
    forge_as(t, &h, &["SUCCESS"], &json!({ "baseRefOid": format!("--output={}", pwn.display()) }));
    let (code, out) = bp(v, t, &[], &["land", ISSUE, "--dry-run"]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains("the forge gave base"), "{out}");
    for base in [json!(null), json!("")] {
        forge_as(t, &h, &["SUCCESS"], &json!({ "baseRefName": base }));
        let (code, out) = bp(v, t, &[], &["land", ISSUE]);
        assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{base}: {out}");
        assert!(out["fix"].as_str().unwrap().contains("names no base branch"), "{out}");
    }
    assert!(!t.join("merged").exists());
}

/// `gh pr merge` exits 0 when a merge queue takes the pull request and merges nothing: `land` reads the forge again
/// and removes nothing until it reads `MERGED` at the head (review of #100, F1). The clock refuses before the merge,
/// dry run too (F6).
#[test]
fn land_removes_nothing_until_the_forge_reads_merged() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    for args in [&["land", ISSUE, "--dry-run"][..], &["land", ISSUE]] {
        let (code, out) = bp(v, t, &[("BP_NOW", "never")], args);
        assert_eq!((code, out["code"].as_str()), (Some(4), Some("usage")), "{out}");
    }
    assert!(!t.join("merged").exists(), "a clock that refuses refuses before the merge");
    std::fs::write(t.join("merge-noop"), "").unwrap();
    let before = commits(v);
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains("a merge queue or auto-merge holds it"), "{out}");
    assert!(t.join("merged").exists() && w.is_dir(), "the worktree stays");
    assert_eq!(git(&t.join("origin.git"), &["rev-parse", "bp/a"]).trim(), h, "origin's branch stays");
    assert_eq!(commits(v), before);
    std::fs::remove_file(t.join("merge-noop")).unwrap();
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!((code, out["status"].as_str()), (Some(0), Some("resolved")), "{out}");
}

/// The claim is read again just before the merge: one released while `land` read the gates refuses `claim_lost`, and
/// nothing merges (review of #100, F10).
#[test]
fn land_refuses_a_claim_released_while_it_reads() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    let release = format!(
        "rm '{}'; '{}' release {ISSUE} --reason lost --json > /dev/null\n",
        t.join("on-list").display(),
        env!("CARGO_BIN_EXE_bp")
    );
    std::fs::write(t.join("on-list"), release).unwrap();
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("claim_lost")), "{out}");
    assert!(!t.join("merged").exists() && w.is_dir(), "nothing merged, nothing removed");
}

/// A claim released while `gh pr merge` runs leaves the merge in place and removes nothing: the fix names the merged
/// commit and `bp show` (review of #100, V3).
#[test]
fn a_claim_released_during_the_merge_removes_nothing() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    let release = format!(
        "rm '{}'; '{}' release {ISSUE} --reason lost --json > /dev/null\n",
        t.join("on-merge").display(),
        env!("CARGO_BIN_EXE_bp")
    );
    std::fs::write(t.join("on-merge"), release).unwrap();
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    let merge = git(&t.join("origin.git"), &["rev-parse", "master"]).trim().to_string();
    let want = format!("pull request 1 merged at {merge}, but the claim closed meanwhile, so land removed nothing");
    let fix = out["fix"].as_str().unwrap();
    assert!(fix.starts_with(&want) && fix.ends_with(&format!("bp show {ISSUE} names who closed it")), "{fix}");
    assert!(w.is_dir(), "the worktree stays");
    assert_eq!(git(&t.join("origin.git"), &["rev-parse", "bp/a"]).trim(), h, "origin's branch stays");
}

/// A rerun of a land that stopped after the merge: a tool that cannot run keeps its own code and fix, as the rerun
/// finishes the land, and only a gate's refusal reads as a lost merge (review of #100, F2). A broken `ssh-keygen`
/// refuses `could_not_run`, not as a signature that does not verify (F3).
#[test]
fn a_rerun_after_the_merge_keeps_a_tool_failure_s_own_fix() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let human = signer(t);
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &HIGH);
    assert_eq!(bp(v, t, &[("SSH_AUTH_SOCK", "")], &["approve", ISSUE, "--key", &human]).0, Some(0));
    forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED" }));
    let land = || bp(v, t, &[("BP_ROLE", "session"), ("SSH_AUTH_SOCK", "")], &["land", ISSUE]);
    std::fs::write(t.join("graphql-fail"), "").unwrap();
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    assert!(!out["fix"].as_str().unwrap().contains("--reason lost"), "a failed call is no lost merge: {out}");
    std::fs::remove_file(t.join("graphql-fail")).unwrap();
    let keygen = t.join("bin/ssh-keygen");
    std::fs::write(&keygen, "#!/bin/sh\nexit 255\n").unwrap();
    std::fs::set_permissions(&keygen, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (code, out) = land();
    assert_eq!(
        (code, out["code"].as_str(), out["subject"].as_str()),
        (Some(1), Some("could_not_run"), Some("ssh-keygen -Y verify")),
        "{out}"
    );
    assert_eq!(
        out["fix"], "install OpenSSH 8.2 or later, then rerun",
        "no leading semicolon when ssh-keygen is silent"
    );
    std::fs::remove_file(&keygen).unwrap();
    // A test merge that cannot run reads as git's fault, not as a level that rose or a merge that was lost (V2).
    let git = t.join("bin/git");
    let wrapper = "#!/bin/sh\ncase \" $* \" in *\" merge-tree \"*) echo 'fatal: no merge today' >&2; exit 2 ;; esac\nexec /usr/bin/git \"$@\"\n";
    std::fs::write(&git, wrapper).unwrap();
    std::fs::set_permissions(&git, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let (code, out) = land();
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    let fix = out["fix"].as_str().unwrap();
    assert!(fix.contains("git cannot test-merge the head into the base: fatal: no merge today"), "{fix}");
    assert!(!fix.contains("--reason lost"), "a failed test merge is no lost merge: {fix}");
    std::fs::remove_file(&git).unwrap();
    let (code, out) = land();
    assert_eq!(
        (code, out["status"].as_str(), out["level"].as_str()),
        (Some(0), Some("resolved"), Some("high")),
        "{out}"
    );
    assert!(!t.join("merged").exists(), "a merged pull request is not merged again");
}

/// A land killed inside its vault commit leaves `resolved` on disk with the journal; the rerun puts the pre-images
/// back and writes the land commit (review of #100, F9).
#[test]
fn a_rerun_after_a_kill_in_the_vault_commit_writes_the_land() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (_, h) = opened(v, t, &[]);
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    let marker = t.join("kill");
    let hook = format!(
        "#!/bin/sh\n[ -f '{m}' ] || exit 0\nrm '{m}'\nkill -9 \"$(ps -o ppid= -p \"$PPID\" | tr -d ' ')\"\nexit 1\n",
        m = marker.display()
    );
    let hooks = v.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("pre-commit"), hook).unwrap();
    std::fs::set_permissions(hooks.join("pre-commit"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(&marker, "").unwrap();
    let before = commits(v);
    let (code, _) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!(code, None, "killed");
    assert_eq!(commits(v), before);
    assert!(std::fs::read_to_string(v.join(format!("{ISSUE}.md"))).unwrap().contains("status: resolved"));
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!((code, out["status"].as_str()), (Some(0), Some("resolved")), "{out}");
    assert_eq!(git(v, &["log", "-1", "--format=%s"]).trim(), format!("land {ISSUE}"));
    assert_eq!(git(v, &["status", "--porcelain"]), "");
}

/// The dry run tests a layer above as the land does before its merge: a stranded layer's claim with an edit not
/// committed refuses, and nothing merges (review of #100, F7).
#[test]
fn a_dry_run_tests_the_claim_of_each_layer_above() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let Stack { claim, .. } = stacked(v, t);
    let path = v.join(format!("{claim}.md"));
    std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + "\nmore\n").unwrap();
    for args in [&["land", ISSUE, "--dry-run"][..], &["land", ISSUE]] {
        let (code, out) = bp(v, t, &[], args);
        assert_eq!((code, out["code"].as_str()), (Some(2), Some("path_dirty")), "{out}");
    }
    assert!(!t.join("merged").exists());
}

/// A merged head is judged by the rules at the base it merged into, its merge commit's first parent (review of #100,
/// F8). A rule pushed after the merge does not strand the rerun of a land that stopped there. A rule the merge landed
/// on still counts: the tip after the merge holds the head, so a diff against it is empty and would read low.
#[test]
fn a_merged_head_is_judged_by_the_rules_it_merged_onto() {
    for before in [false, true] {
        let (v, t) = setup();
        let (v, t) = (v.path(), t.path());
        let (_, h) = opened(v, t, &[]);
        pushed(v, t, &h);
        lenses(v, t, &h, &MEDIUM);
        if before {
            rules(t, "high:\n  - hello.txt\n");
        }
        forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED" }));
        if !before {
            rules(t, "high:\n  - hello.txt\n");
        }
        let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
        if before {
            assert_eq!((code, out["code"].as_str()), (Some(2), Some("evidence_stale")), "{out}");
            assert!(out["fix"].as_str().unwrap().contains("pull request 1 is merged"), "{out}");
        } else {
            let got = (code, out["status"].as_str(), out["level"].as_str());
            assert_eq!(got, (Some(0), Some("resolved"), Some("medium")), "{out}");
        }
    }
}

/// A head merged outside `land` is judged by the pull request's own diff, from the forge's base to the head, whatever
/// the merge made. A rebase merge's commit holds the head's earlier commits in its first parent, so a diff against that
/// parent shows the last commit only (review of #100, V4).
#[test]
fn a_rebase_merged_head_is_judged_by_its_whole_diff() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let (w, _) = opened(v, t, &[]);
    std::fs::write(w.join("other.txt"), "o\n").unwrap();
    let h = commit(&w, &["add", "other.txt"], "other");
    pushed(v, t, &h);
    lenses(v, t, &h, &MEDIUM);
    // The rules name `hello.txt` high on the base; the forge then rebases the two commits onto it, one by one.
    rules(t, "high:\n  - hello.txt\n");
    let code = t.join("code");
    let base = git(&code, &["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(code.join("hello.txt"), "hi\n").unwrap();
    commit(&code, &["add", "hello.txt"], "hello");
    std::fs::write(code.join("other.txt"), "o\n").unwrap();
    let last = commit(&code, &["add", "other.txt"], "other");
    git(&code, &["push", "-q", "origin", "master"]);
    let merge = json!({ "oid": last });
    forge_as(t, &h, &["SUCCESS"], &json!({ "state": "MERGED", "mergeCommit": merge, "baseRefOid": base }));
    let (code, out) = bp(v, t, &[("BP_ROLE", "session")], &["land", ISSUE]);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("evidence_stale")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains("pull request 1 is merged"), "{out}");
}
