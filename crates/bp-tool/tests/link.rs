//! `bp link` through the binary, over a small vault under git: one edge, one changed line, one commit, the repeats,
//! the removal and the refusals.

use std::path::Path;
use std::process::Command as Std;

use assert_cmd::Command;

const A: &str = "projects/bp/issues/0001.a";
const B: &str = "projects/bp/issues/0002.b";
const SYSTEM: &str = "projects/bp/designs/system";
const D: &str = "projects/bp/decisions/d";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, text) in [
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n"),
        (
            "projects/bp/issues/0001.a.md",
            "---\nkind: issue\ntitle: A\nstatus: open\ncites: [\"[[legacy.x]]\"]\nseverity: low\n---\n\nLead.\n",
        ),
        (
            "projects/bp/issues/0002.b.md",
            "---\nkind: issue\ntitle: B\nstatus: open\naliases: [\"projects/bp/issues/old\"]\n---\n",
        ),
        (
            "projects/bp/decisions/d.md",
            "---\nkind: decision\ntitle: D\nstatus: proposed\nanswers: [\"shared/threads/t\"]\n---\n",
        ),
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
        ("shared/threads/u.md", "---\nkind: thread\ntitle: U\nstatus: open\n---\n"),
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
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(root)
        .env_remove("BP_VAULT")
        .env_remove("BP_ROLE")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

fn commits(root: &Path) -> String {
    git(root, &["rev-list", "--count", "HEAD"]).trim().to_string()
}

#[test]
fn link_adds_and_removes_one_edge_a_commit() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["link", A, "cites", SYSTEM]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim());
    assert_eq!(git(root, &["diff", "--numstat", "HEAD~1", "HEAD"]), "1\t1\tprojects/bp/issues/0001.a.md\n");
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: link"));
    let shown = bp(root, &["show", A]).1;
    assert_eq!(shown["cites"], serde_json::json!([SYSTEM]), "the index took the write");
    assert_eq!(shown["unresolved"][0]["text"], "[[legacy.x]]", "a link text the write did not name stays");
    assert_eq!(
        bp(root, &["link", A, "cites", SYSTEM]),
        (Some(3), serde_json::json!({ "id": A, "contract_version": 1 }))
    );
    assert_eq!(bp(root, &["link", A, "depends", "projects/bp/issues/old"]).0, Some(0), "an alias names its node");
    assert_eq!(bp(root, &["show", A]).1["depends"], serde_json::json!([B]));
    assert_eq!(bp(root, &["link", A, "cites", SYSTEM, "--remove"]).0, Some(0));
    assert_eq!(bp(root, &["link", A, "cites", SYSTEM, "--remove"]).0, Some(3), "an absent edge is a no-op");
    assert_eq!(bp(root, &["link", A, "cites", "[[legacy.x]]", "--remove"]).0, Some(0), "a broken link text can go");
    assert_eq!(bp(root, &["show", A]).1.get("unresolved"), None);
    assert_eq!(commits(root), "5");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn link_refuses_what_the_kind_schema_does_not_hold() {
    let dir = vault();
    let root = dir.path();
    for (args, want) in [
        (["link", A, "cites", "projects/bp/designs/nope"], "unresolved_link"),
        (["link", A, "depends", SYSTEM], "schema_invalid"),
        (["link", A, "answers", "shared/threads/t"], "schema_invalid"),
        (["link", A, "related", SYSTEM], "schema_invalid"),
        (["link", D, "answers", "shared/threads/u"], "schema_invalid"),
        (["link", "projects/bp/issues/nope", "cites", SYSTEM], "no_node"),
    ] {
        let (code, v) = bp(root, &args);
        assert_eq!((code, v["code"].as_str()), (Some(2), Some(want)), "{args:?}: {v}");
    }
    let (_, v) = bp(root, &["link", D, "answers", "shared/threads/u"]);
    assert!(v["fix"].as_str().unwrap().contains("--remove"), "one target: the fix removes the old first: {v}");
    assert_eq!(commits(root), "1");
}

#[test]
fn a_dry_run_writes_nothing() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["link", A, "cites", SYSTEM, "--dry-run"]);
    assert_eq!((code, v["dry_run"].as_bool()), (Some(0), Some(true)), "{v}");
    assert!(v.get("commit").is_none());
    assert_eq!(commits(root), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}
