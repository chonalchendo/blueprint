//! `bp set` through the binary, over a small vault under git: one line, one commit, the refusals, the no-op and
//! the dry run.

use std::path::Path;
use std::process::Command as Std;

use assert_cmd::Command;

const ISSUE: &str = "projects/bp/issues/0001.a";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, text) in [
        (
            "projects/bp/issues/0001.a.md",
            "---\nkind: issue\ntitle: A\nstatus: open\nseverity: low\ntags: [\"x\"]\n---\n\nLead.\n",
        ),
        (
            "projects/bp/issues/0001.a/claims/s1.md",
            "---\nkind: claim\nunder: projects/bp/issues/0001.a\nowner: me\n---\n",
        ),
        (
            "projects/bp/designs/system.md",
            "---\nkind: design\ntitle: S\nstatus: active\nscope: system\ndesign_kind: system\n---\n",
        ),
        ("shared/.keep", ""),
        (".bp/.gitignore", "*\n"),
    ] {
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

fn bp(root: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    bp_with(root, None, args)
}

fn bp_with(root: &Path, role: Option<&str>, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let mut cmd = Command::cargo_bin("bp").unwrap();
    cmd.current_dir(root).env_remove("BP_VAULT").env_remove("BP_ROLE");
    if let Some(r) = role {
        cmd.env("BP_ROLE", r);
    }
    let out = cmd.args(args).arg("--json").output().unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

#[test]
fn set_writes_one_line_in_one_commit() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["set", ISSUE, "severity", "high"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(
        (v["id"].as_str(), v["was"].as_str(), v["contract_version"].as_u64()),
        (Some(ISSUE), Some("low"), Some(1))
    );
    assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim());
    assert_eq!(git(root, &["diff", "--numstat", "HEAD~1", "HEAD"]), "1\t1\tprojects/bp/issues/0001.a.md\n");
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Role: human"));
    assert_eq!(bp(root, &["show", ISSUE]).1["severity"], "high", "the index took the write");
    let (code, v) = bp(root, &["set", ISSUE, "severity", "high"]);
    assert_eq!((code, v["id"].as_str()), (Some(3), Some(ISSUE)), "a repeat is a no-op");
    let (code, _) = bp(root, &["set", ISSUE, "tags", "a, b"]);
    assert_eq!(code, Some(0));
    assert_eq!(bp(root, &["show", ISSUE]).1["tags"], serde_json::json!(["a", "b"]));
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "3");
}

#[test]
fn set_refuses_what_is_not_a_label() {
    let dir = vault();
    let root = dir.path();
    for (args, code) in [
        (vec!["set", ISSUE, "status", "resolved"], "not_a_label"),
        (vec!["set", ISSUE, "cites", "projects/bp/designs/system"], "not_a_label"),
        (vec!["set", ISSUE, "under", "projects/bp/milestones/m"], "not_a_label"),
        (vec!["set", "projects/bp/issues/0001.a/claims/s1", "owner", "you"], "not_a_label"),
        (vec!["set", ISSUE, "colour", "red"], "schema_invalid"),
        (vec!["set", ISSUE, "severity", "urgent"], "unknown_value"),
        (vec!["set", "projects/bp/designs/system", "scope", "product"], "not_a_label"),
        (vec!["set", "projects/bp/designs/system", "design_kind", "glossary"], "not_a_label"),
        (vec!["set", ISSUE, "title", " "], "schema_invalid"),
        (vec!["set", ISSUE, "tags", "x: y,#z"], "schema_invalid"),
        (vec!["set", ISSUE, "tags", "Domain/X"], "schema_invalid"),
        (vec!["set", "projects/bp/issues/nope", "severity", "low"], "no_node"),
    ] {
        let (exit, v) = bp(root, &args);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some(code)), "{args:?}: {v}");
    }
    let (_, v) = bp(root, &["set", ISSUE, "status", "resolved"]);
    assert!(v["fix"].as_str().unwrap().contains("bp transition"), "{v}");
    let (_, v) = bp(root, &["set", "projects/bp/designs/system", "scope", "product"]);
    assert_eq!(v["fix"], "the path names it (12, Layout); rename moves a design");
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn bp_role_is_a_closed_set_and_empty_reads_as_human() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp_with(root, Some("bogus"), &["set", ISSUE, "severity", "high"]);
    assert_eq!((code, v["code"].as_str(), v["subject"].as_str()), (Some(4), Some("usage"), Some("BP_ROLE=bogus")));
    let (code, _) = bp_with(root, Some(""), &["set", ISSUE, "severity", "high"]);
    assert_eq!(code, Some(0));
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Role: human"));
}

#[test]
fn tags_are_deduplicated_and_a_quoted_key_is_rewritten() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["set", ISSUE, "tags", "a,b/c,a"]);
    assert_eq!((code, &v["value"]), (Some(0), &serde_json::json!(["a", "b/c"])));
    let path = root.join(format!("{ISSUE}.md"));
    let text = std::fs::read_to_string(&path).unwrap().replace("severity: low", "\"severity\": low");
    std::fs::write(&path, text).unwrap();
    git(root, &["commit", "-qam", "quoted"]);
    let (code, v) = bp(root, &["set", ISSUE, "severity", "critical"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(git(root, &["diff", "--numstat", "HEAD~1", "HEAD"]), format!("1\t1\t{ISSUE}.md\n"));
}

#[test]
fn a_dirty_target_refuses_path_dirty_first() {
    let dir = vault();
    let root = dir.path();
    let path = root.join(format!("{ISSUE}.md"));
    let edited = std::fs::read_to_string(&path).unwrap() + "HUMAN WORK\n";
    std::fs::write(&path, &edited).unwrap();
    let (code, v) = bp(root, &["set", ISSUE, "severity", "low"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "even a value the file holds: {v}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "1");
}

#[test]
fn a_dry_run_writes_nothing() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["set", ISSUE, "severity", "high", "--dry-run"]);
    assert_eq!((code, v["dry_run"].as_bool(), v["value"].as_str()), (Some(0), Some(true), Some("high")));
    assert!(v.get("commit").is_none());
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}
