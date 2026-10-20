//! `bp link` through the binary, over a small vault under git: one edge, one changed line, one commit, the repeats,
//! the removal and the refusals.

mod common;

use common::{bp, commits, git};

const A: &str = "projects/bp/issues/0001.a";
const B: &str = "projects/bp/issues/0002.b";
const SYSTEM: &str = "projects/bp/designs/system";
const D: &str = "projects/bp/decisions/d";

fn vault() -> tempfile::TempDir {
    common::vault(&[
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
        ("projects/zz/issues/0001.z.md", "---\nkind: issue\ntitle: Z\nstatus: open\n---\n"),
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
        ("shared/threads/u.md", "---\nkind: thread\ntitle: U\nstatus: open\n---\n"),
    ])
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
    let text = std::fs::read_to_string(root.join(format!("{A}.md"))).unwrap();
    assert!(text.contains(&format!("cites: [\"[[legacy.x]]\", \"{SYSTEM}\"]\n")), "written order, appended: {text}");
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
    let text = std::fs::read_to_string(root.join(format!("{A}.md"))).unwrap();
    assert!(!text.contains("cites"), "the last edge out takes its line: {text}");
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
        (["link", A, "depends", A], "schema_invalid"),
        (["link", A, "depends", "projects/zz/issues/0001.z"], "schema_invalid"),
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

#[test]
fn a_removal_through_an_alias_removes_the_edge() {
    let dir = vault();
    let root = dir.path();
    assert_eq!(bp(root, &["link", A, "depends", "projects/bp/issues/old"]).0, Some(0));
    let (code, v) = bp(root, &["link", A, "depends", "projects/bp/issues/old", "--remove"]);
    assert_eq!((code, v["to"].as_str()), (Some(0), Some(B)), "{v}");
    assert_eq!(bp(root, &["show", A]).1.get("depends"), None);
    assert_eq!(commits(root), "3");
}

#[test]
fn a_dirty_source_refuses_path_dirty_before_the_comparison() {
    let dir = vault();
    let root = dir.path();
    let path = root.join(format!("{A}.md"));
    let edited = std::fs::read_to_string(&path).unwrap().replace("[\"[[legacy.x]]\"]", &format!("[\"{SYSTEM}\"]"));
    std::fs::write(&path, &edited).unwrap();
    let (code, v) = bp(root, &["link", A, "cites", SYSTEM]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "not exit 3 from uncommitted bytes: {v}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    assert_eq!(commits(root), "1");
}

#[test]
fn a_dry_run_refuses_a_dirty_source() {
    let dir = vault();
    let root = dir.path();
    let path = root.join(format!("{A}.md"));
    let edited = std::fs::read_to_string(&path).unwrap() + "HUMAN WORK\n";
    std::fs::write(&path, &edited).unwrap();
    let (code, v) = bp(root, &["link", A, "cites", SYSTEM, "--dry-run"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "{v}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
}
