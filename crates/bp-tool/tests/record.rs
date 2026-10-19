//! `bp record` through the binary: each kind's path and fields (13), the stamp and its `-2`, a delta `proposed`
//! and `delta_pending`, the kinds another verb owns, and no record rewritten.

use std::path::Path;
use std::process::Command as Std;

use assert_cmd::Command;

const DESIGN: &str = "projects/bp/designs/system";
const MILESTONE: &str = "projects/bp/milestones/m";
const ISSUE: &str = "projects/bp/issues/i";
const LEARNING: &str = "shared/learnings/l";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (id, text) in [
        ("projects/bp/project", "---\nkind: project\ntitle: BP\nstatus: active\n---\n"),
        (DESIGN, "---\nkind: design\ntitle: S\nstatus: active\n---\n"),
        (MILESTONE, "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n"),
        (ISSUE, "---\nkind: issue\ntitle: I\nstatus: open\n---\n"),
        (LEARNING, "---\nkind: learning\ntitle: L\nstatus: draft\n---\n"),
    ] {
        let file = root.join(format!("{id}.md"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::create_dir_all(root.join("shared")).unwrap();
    std::fs::write(root.join(".gitignore"), ".bp/\nchange.md\n").unwrap();
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
        .env("BP_ROLE", "worker")
        .env("BP_NOW", "2026-10-19T10:00:00Z")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

#[test]
fn each_kind_writes_one_file_at_its_path() {
    let dir = vault();
    let root = dir.path();
    let body = root.join("change.md");
    std::fs::write(&body, "One change.\n").unwrap();
    let head = "0123456789abcdef0123456789abcdef01234567";
    let ms = format!("milestone={MILESTONE}");
    let (h, iss, review) = (format!("head={head}"), format!("issue={ISSUE}"), format!("{ISSUE}/reviews/{head}"));
    for (args, id) in [
        (
            vec!["delta", DESIGN, "--field", &ms, "--body-file", body.to_str().unwrap()],
            "projects/bp/designs/system/deltas/2026-10-19T10-00-00Z",
        ),
        (vec!["review", ISSUE, "--field", &h, "--field", "role=reviewer"], review.as_str()),
        (vec!["review", ISSUE, "--field", &h], &format!("{review}-2")),
        (
            vec!["disposition", &review, "--field", "finding=2", "--field", "as=declined"],
            &format!("{review}/dispositions/2"),
        ),
        (
            vec!["concern-answer", "projects/bp/project", "--field", "concern=data-retention", "--field", "state=open"],
            "projects/bp/concerns/data-retention",
        ),
        (
            vec!["evidence", LEARNING, "--field", "outcome=helped", "--field", &iss],
            "shared/learnings/l/evidence/2026-10-19T10-00-00Z",
        ),
        (vec!["evidence", LEARNING, "--field", "outcome=hurt"], "shared/learnings/l/evidence/2026-10-19T10-00-00Z-2"),
        (
            vec!["handoff", "projects/bp/project", "--field", "session=8b8b7784-352c"],
            "projects/bp/handoffs/2026-10-19-8b8b7784",
        ),
        (
            vec!["handoff", "shared", "--field", "session=abc", "--external", "https://x/pull/1"],
            "shared/handoffs/2026-10-19-abc",
        ),
    ] {
        let mut a = vec!["record"];
        a.extend(&args);
        let (exit, v) = bp(root, &a);
        assert_eq!((exit, v["id"].as_str()), (Some(0), Some(id)), "{args:?}: {v}");
        assert_eq!(git(root, &["diff", "--name-only", "HEAD~1", "HEAD"]), format!("{id}.md\n"), "{args:?}");
        assert!(git(root, &["log", "-1", "--format=%B"]).contains("Bp-Verb: record\nBp-Role: worker"));
    }
    let (_, v) = bp(root, &["show", "projects/bp/designs/system/deltas/2026-10-19T10-00-00Z"]);
    assert_eq!(
        (v["state"].as_str(), v["milestone"].as_str(), v["stamp"].as_str(), v["under"].as_str()),
        (Some("proposed"), Some(MILESTONE), Some("2026-10-19T10:00:00Z"), Some(DESIGN))
    );
    assert_eq!(v["body"].as_str().map(str::trim), Some("One change."));
    let (_, v) = bp(root, &["show", "shared/handoffs/2026-10-19-abc"]);
    assert_eq!((v["external"].as_str(), v.get("under")), (Some("https://x/pull/1"), None));
    let (_, v) = bp(root, &["show", "shared/learnings/l/evidence/2026-10-19T10-00-00Z"]);
    assert_eq!((v["outcome"].as_str(), v["at"].as_str()), (Some("helped"), Some("2026-10-19T10:00:00Z")));
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "10");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn record_refuses_what_its_kind_does_not_hold() {
    let dir = vault();
    let root = dir.path();
    let body = root.join("change.md");
    std::fs::write(&body, "One change.\n").unwrap();
    let b = body.to_str().unwrap();
    let ms = format!("milestone={MILESTONE}");
    let (exit, _) = bp(root, &["record", "delta", DESIGN, "--field", &ms, "--body-file", b]);
    assert_eq!(exit, Some(0));
    for (args, exit, code) in [
        (vec!["delta", DESIGN, "--field", &ms, "--body-file", b], 2, "delta_pending"),
        (vec!["run", ISSUE], 4, "usage"),
        (vec!["claim", ISSUE], 4, "usage"),
        (vec!["waiver", MILESTONE], 4, "usage"),
        (vec!["issue", ISSUE], 4, "usage"),
        (vec!["evidence", LEARNING, "--field", "outcome"], 4, "usage"),
        (vec!["evidence", LEARNING], 2, "schema_invalid"),
        (
            vec!["evidence", LEARNING, "--field", "outcome=helped", "--field", "at=2026-01-01T00:00:00Z"],
            2,
            "schema_invalid",
        ),
        (vec!["evidence", LEARNING, "--field", "outcome=great"], 2, "unknown_value"),
        (vec!["evidence", ISSUE, "--field", "outcome=helped"], 2, "schema_invalid"),
        (vec!["evidence", "shared/learnings/nope", "--field", "outcome=helped"], 2, "no_node"),
        (
            vec!["evidence", LEARNING, "--field", "outcome=helped", "--field", "issue=projects/bp/issues/nope"],
            2,
            "unresolved_link",
        ),
        (
            vec!["evidence", LEARNING, "--field", "outcome=helped", "--field", &format!("issue={MILESTONE}")],
            2,
            "schema_invalid",
        ),
        (vec!["delta", DESIGN, "--field", &ms], 2, "schema_invalid"),
        (vec!["delta", DESIGN, "--field", &ms, "--field", "state=accepted", "--body-file", b], 2, "schema_invalid"),
        (vec!["review", ISSUE, "--field", "head=../x"], 2, "schema_invalid"),
        (vec!["handoff", "shared", "--field", "session=S/../x"], 2, "schema_invalid"),
    ] {
        let mut a = vec!["record"];
        a.extend(&args);
        let (got, v) = bp(root, &a);
        assert_eq!((got, v["code"].as_str()), (Some(exit), Some(code)), "{args:?}: {v}");
    }
    let (_, v) = bp(root, &["record", "run", ISSUE]);
    assert!(v["fix"].as_str().unwrap().contains("bp check"), "{v}");
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "2");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}
