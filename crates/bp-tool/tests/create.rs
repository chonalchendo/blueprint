//! `bp create` through the binary, over a small vault under git: one node, one commit, the slug, the repeat, the
//! refusals and the dry run.

use std::path::Path;
use std::process::Command as Std;

use assert_cmd::Command;

const M: &str = "projects/bp/milestones/m";

fn git(root: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (rel, text) in [
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        (
            "projects/bp/designs/system.md",
            "---\nkind: design\ntitle: S\nstatus: active\nunder: projects/bp/project\n---\n\nThe system.\n",
        ),
        (
            "projects/bp/milestones/m.md",
            "---\nkind: milestone\ntitle: M\nstatus: planned\nunder: projects/bp/designs/system\n---\n",
        ),
        (
            "projects/bp/issues/0002.b.md",
            "---\nkind: issue\ntitle: B\nstatus: open\nunder: projects/bp/milestones/m\naliases: [\"projects/bp/issues/old\"]\n---\n",
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
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(root)
        .env_remove("BP_VAULT")
        .env_remove("BP_ROLE")
        .env("BP_NOW", "2026-10-19T08:00:00Z")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

fn body(root: &Path, text: &str) -> String {
    let path = root.join(".bp/body.md");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

fn commits(root: &Path) -> String {
    git(root, &["rev-list", "--count", "HEAD"]).trim().to_string()
}

#[test]
fn create_writes_one_node_in_one_commit() {
    let dir = vault();
    let root = dir.path();
    let text = "Lead [[projects/bp/designs/system]].\n\n## Check\n\n```check shell {name=a expect=0}\ntrue\n```";
    let f = body(root, &format!("{text}\n"));
    let args = ["create", "issue", "--project", "bp", "--title", "Round trip quokka", "--under", M, "--body-file", &f];
    let (code, v) = bp(root, &args);
    let id = "projects/bp/issues/round-trip-quokka";
    assert_eq!((code, v["id"].as_str(), v["status"].as_str()), (Some(0), Some(id), Some("open")), "{v}");
    assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim());
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: create"));
    let shown = bp(root, &["show", id]).1;
    assert_eq!((shown["body"].as_str(), shown["under"].as_str()), (Some(text), Some(M)), "the index took the write");
    let (code, v) = bp(root, &args);
    assert_eq!((code, v["id"].as_str()), (Some(3), Some(id)), "a retried create is a no-op");
    assert_eq!(bp(root, &["set", id, "severity", "high"]).0, Some(0));
    assert_eq!(bp(root, &args).0, Some(3), "a label a later verb wrote does not make it another node");
    let other = body(root, "Another body.\n");
    let (code, v) = bp(
        root,
        &["create", "issue", "--project", "bp", "--title", "Round trip quokka", "--under", M, "--body-file", &other],
    );
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("id_taken")), "{v}");
    assert_eq!(commits(root), "3");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn the_slug_is_cut_at_a_word_boundary_and_an_inbox_slug_starts_with_the_date() {
    let dir = vault();
    let root = dir.path();
    let title = "A title long enough that its slug must be cut at a word boundary well past eighty characters";
    let (code, v) = bp(root, &["create", "issue", "--project", "bp", "--title", title, "--under", M]);
    assert_eq!(code, Some(0), "{v}");
    let slug = v["id"].as_str().unwrap().strip_prefix("projects/bp/issues/").unwrap();
    assert_eq!(slug, "a-title-long-enough-that-its-slug-must-be-cut-at-a-word-boundary-well-past");
    let (code, v) = bp(root, &["create", "inbox", "--title", "Bench: one thought"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some("shared/inbox/2026-10-19-bench-one-thought")), "{v}");
    assert_eq!(bp(root, &["show", "shared/inbox/2026-10-19-bench-one-thought"]).1["status"], "raw");
    let (code, v) = bp(root, &["create", "thread", "--project", "bp", "--title", "Why?"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some("projects/bp/threads/why")), "{v}");
    let (code, v) = bp(
        root,
        &[
            "create",
            "design",
            "--project",
            "bp",
            "--title",
            "Walk",
            "--slug",
            "components/walk",
            "--under",
            "projects/bp/designs/system",
        ],
    );
    assert_eq!((code, v["status"].as_str()), (Some(0), Some("draft")), "{v}");
}

#[test]
fn create_refuses_what_breaks_the_schema() {
    let dir = vault();
    let root = dir.path();
    let issue = ["create", "issue", "--project", "bp", "--title", "X", "--under", M];
    let unnamed = body(root, "## Check\n\n~~~check shell {expect=0}\ntrue\n~~~\n");
    let system = "projects/bp/designs/system";
    for (args, want) in [
        ([&issue[..], &["--slug", "0002.b"]].concat(), "id_taken"),
        ([&issue[..], &["--slug", "old"]].concat(), "id_taken"),
        ([&issue[..], &["--body-file", &unnamed]].concat(), "check_unnamed"),
        ([&issue[..], &["--slug", "a/b"]].concat(), "schema_invalid"),
        (vec!["create", "issue", "--title", "X", "--under", M], "schema_invalid"),
        (vec!["create", "issue", "--project", "bp", "--title", "X"], "schema_invalid"),
        (vec!["create", "issue", "--project", "bp", "--title", "X", "--under", system], "schema_invalid"),
        (
            vec!["create", "issue", "--project", "bp", "--title", "X", "--under", "projects/bp/milestones/no"],
            "unresolved_link",
        ),
        (vec!["create", "issue", "--project", "zz", "--title", "X", "--under", M], "unknown_value"),
        (vec!["create", "learning", "--project", "bp", "--title", "X"], "schema_invalid"),
        (vec!["create", "design", "--project", "bp", "--title", "Walk", "--under", system], "schema_invalid"),
        (vec!["create", "inbox", "--title", "!!"], "schema_invalid"),
    ] {
        let (code, v) = bp(root, &args);
        assert_eq!((code, v["code"].as_str()), (Some(2), Some(want)), "{args:?}: {v}");
    }
    for (text, want) in [
        ("## Check\n\nProse only.\n", "check_empty"),
        ("## Check\n\n### Direct\n\n~~~check shell {name=a}\ntrue\n~~~\n\n### Indirect\n\nBy hand.\n", "check_empty"),
        ("## Check\n\n~~~check shell {name=a}\ntrue\n~~~\n~~~check shell {name=a}\ntrue\n~~~\n", "check_duplicate"),
    ] {
        let f = body(root, text);
        let (code, v) = bp(root, &[&issue[..], &["--body-file", &f]].concat());
        assert_eq!((code, v["code"].as_str()), (Some(2), Some(want)), "{text}: {v}");
    }
    let (code, v) = bp(root, &["create", "claim", "--title", "X"]);
    assert_eq!((code, v["code"].as_str()), (Some(4), Some("usage")), "a record has its own verb: {v}");
    assert_eq!(commits(root), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn a_dry_run_writes_nothing() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["create", "issue", "--project", "bp", "--title", "Dry", "--under", M, "--dry-run"]);
    assert_eq!((code, v["dry_run"].as_bool(), v["id"].as_str()), (Some(0), Some(true), Some("projects/bp/issues/dry")));
    assert!(v.get("commit").is_none());
    assert_eq!(commits(root), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}
