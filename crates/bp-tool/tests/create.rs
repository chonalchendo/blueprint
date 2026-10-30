//! `bp create` through the binary, over a small vault under git: one node, one commit, the slug, the repeat, the
//! refusals and the dry run.

use std::path::Path;

mod common;

use common::{bp, commits, git};

const M: &str = "projects/bp/milestones/m";

fn vault() -> tempfile::TempDir {
    common::vault(&[
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        (
            "projects/bp/designs/system.md",
            "---\nkind: design\ntitle: S\nstatus: active\nunder: projects/bp/project\naliases: [\"projects/bp/designs/old-system\"]\n---\n\nThe system.\n",
        ),
        (
            "projects/bp/milestones/m.md",
            "---\nkind: milestone\ntitle: M\nstatus: planned\nunder: projects/bp/designs/system\n---\n",
        ),
        (
            "projects/bp/issues/0002.b.md",
            "---\nkind: issue\ntitle: B\nstatus: open\nunder: projects/bp/milestones/m\naliases: [\"projects/bp/issues/old\"]\n---\n",
        ),
        (
            "projects/bp/issues/0003.Case.md",
            "---\nkind: issue\ntitle: Case\nstatus: open\nunder: projects/bp/milestones/m\n---\n",
        ),
        ("projects/zz/project.md", "---\nkind: project\ntitle: zz\nstatus: active\n---\n"),
        (
            "projects/zz/designs/system.md",
            "---\nkind: design\ntitle: Z\nstatus: active\nunder: projects/zz/project\n---\n",
        ),
        (
            "projects/zz/milestones/m.md",
            "---\nkind: milestone\ntitle: ZM\nstatus: planned\nunder: projects/zz/designs/system\n---\n",
        ),
        ("shared/.keep", ""),
    ])
}

fn body(root: &Path, text: &str) -> String {
    let path = root.join(".bp/body.md");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
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
    let badname = body(root, "## Check\n\n~~~check shell {name=../../x}\ntrue\n~~~\n");
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
        (vec!["create", "issue", "--project", "qq", "--title", "X", "--under", M], "unknown_value"),
        (vec!["create", "issue", "--project", "BP", "--title", "X", "--under", M], "unknown_value"),
        ([&issue[..], &["--slug", "Upper-Case"]].concat(), "schema_invalid"),
        ([&issue[..], &["--slug", "nl\nBp-Role: worker"]].concat(), "schema_invalid"),
        ([&issue[..], &["--slug", "foo#bar"]].concat(), "schema_invalid"),
        ([&issue[..], &["--slug", "notes.md"]].concat(), "schema_invalid"),
        ([&issue[..], &["--slug", ".hidden"]].concat(), "schema_invalid"),
        ([&issue[..], &["--slug", &"a".repeat(81)]].concat(), "schema_invalid"),
        ([&issue[..], &["--slug", "0003.case"]].concat(), "id_taken"),
        ([&issue[..], &["--body-file", &badname]].concat(), "check_unnamed"),
        (vec!["create", "issue", "--project", "bp", "--title", "", "--slug", "empty", "--under", M], "schema_invalid"),
        (
            vec!["create", "issue", "--project", "bp", "--title", "   ", "--slug", "blank", "--under", M],
            "schema_invalid",
        ),
        (vec!["create", "issue", "--project", "bp", "--title", "   ", "--under", M], "schema_invalid"),
        (vec!["create", "issue", "--project", "bp", "--title", "Café résumé", "--under", M], "schema_invalid"),
        (
            vec!["create", "issue", "--project", "bp", "--title", "X", "--under", "projects/zz/milestones/m"],
            "schema_invalid",
        ),
        (
            vec!["create", "milestone", "--project", "bp", "--title", "X", "--under", "projects/zz/designs/system"],
            "schema_invalid",
        ),
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
    let fix = |args: &[&str]| bp(root, args).1["fix"].as_str().unwrap().to_string();
    let upper = fix(&[&issue[..], &["--slug", "Upper-Case"]].concat());
    assert!(upper.contains("--slug x"), "the fix shows the minted slug: {upper}");
    let cafe = fix(&["create", "issue", "--project", "bp", "--title", "Café", "--under", M]);
    assert!(cafe.contains("--slug"), "{cafe}");
    let badname = body(root, "## Check\n\n~~~check shell {name=A}\ntrue\n~~~\n");
    let name = fix(&[&issue[..], &["--body-file", &badname]].concat());
    assert!(name.contains("name=A") && name.contains("a name=<slug>"), "{name}");
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

#[test]
fn a_case_variant_alias_and_inbox_slug_resolve_to_one_id() {
    let dir = vault();
    let root = dir.path();
    let case = ["create", "issue", "--project", "bp", "--title", "Case", "--slug", "0003.case", "--under", M];
    let (code, v) = bp(root, &case);
    assert_eq!((code, v["id"].as_str()), (Some(3), Some("projects/bp/issues/0003.Case")), "one id up to case: {v}");
    let (code, v) = bp(root, &["create", "inbox", "--title", "x", "--slug", "no-date"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some("shared/inbox/2026-10-19-no-date")), "{v}");
    let (code, v) = bp(root, &["create", "inbox", "--title", "x", "--slug", "2026-10-18-dated"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some("shared/inbox/2026-10-18-dated")), "{v}");
    let alias =
        ["create", "milestone", "--project", "bp", "--title", "Next", "--under", "projects/bp/designs/old-system"];
    let (code, v) = bp(root, &alias);
    assert_eq!(code, Some(0), "{v}");
    let text = std::fs::read_to_string(root.join("projects/bp/milestones/next.md")).unwrap();
    assert!(text.contains("under: projects/bp/designs/system\n"), "the real id, not the alias: {text}");
    assert_eq!(bp(root, &alias).0, Some(3), "a retry through the alias is the same node");
}

#[test]
fn a_body_keeps_its_first_indent_and_drops_a_bom_and_crlf() {
    let dir = vault();
    let root = dir.path();
    let f = body(root, "\u{feff}\n\n    indented code\r\nLine two.\r\n\n");
    let (code, v) =
        bp(root, &["create", "issue", "--project", "bp", "--title", "Indent", "--under", M, "--body-file", &f]);
    assert_eq!(code, Some(0), "{v}");
    let text = std::fs::read_to_string(root.join("projects/bp/issues/indent.md")).unwrap();
    assert!(text.ends_with("---\n\n    indented code\nLine two.\n"), "{text:?}");
}

#[test]
fn a_dirty_target_refuses_path_dirty_before_the_comparison() {
    let dir = vault();
    let root = dir.path();
    let args = ["create", "issue", "--project", "bp", "--title", "Dirty", "--under", M];
    let (_, dry) = bp(root, &[&args[..], &["--dry-run"]].concat());
    assert_eq!(dry["id"], "projects/bp/issues/dirty", "{dry}");
    let path = root.join("projects/bp/issues/dirty.md");
    std::fs::write(&path, format!("---\nkind: issue\ntitle: Dirty\nstatus: open\nunder: {M}\n---\n")).unwrap();
    let (code, v) = bp(root, &args);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "an untracked twin is not a retry: {v}");
    assert!(path.exists());
    let tracked = root.join("projects/bp/issues/0002.b.md");
    let edited = std::fs::read_to_string(&tracked).unwrap() + "HUMAN WORK\n";
    std::fs::write(&tracked, &edited).unwrap();
    let (code, v) = bp(root, &["create", "issue", "--project", "bp", "--title", "B", "--slug", "0002.b", "--under", M]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "{v}");
    assert_eq!(std::fs::read_to_string(&tracked).unwrap(), edited);
    assert_eq!(commits(root), "1");
}

#[test]
fn a_body_file_that_holds_a_header_refuses() {
    let dir = vault();
    let root = dir.path();
    let args = ["create", "issue", "--project", "bp", "--title", "Nested", "--under", M, "--body-file"];
    let own = root.join("projects/bp/issues/0002.b.md").display().to_string();
    for dry in [&[][..], &["--dry-run"][..]] {
        let (code, v) = bp(root, &[&args[..], &[own.as_str()], dry].concat());
        assert_eq!((code, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{dry:?}: {v}");
    }
    let f = body(root, "---\nA paragraph between rules.\n---\n");
    let (code, v) = bp(root, &[&args[..], &[f.as_str()]].concat());
    assert_eq!(code, Some(0), "a thematic break holds no kind: {v}");
    assert_eq!(commits(root), "2");
}

#[test]
fn a_dry_run_refuses_a_dirty_target() {
    let dir = vault();
    let root = dir.path();
    let path = root.join("projects/bp/issues/dirty.md");
    std::fs::write(&path, "HUMAN WORK\n").unwrap();
    let args = ["create", "issue", "--project", "bp", "--title", "Dirty", "--under", M, "--dry-run"];
    let (code, v) = bp(root, &args);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "{v}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "HUMAN WORK\n");
}

#[test]
fn a_template_writes_nothing_and_each_unfilled_slot_is_a_finding() {
    let dir = vault();
    let root = dir.path();
    let template = |kind: &str| {
        let (code, v) = bp(root, &["create", kind, "--template"]);
        assert_eq!((code, v["kind"].as_str()), (Some(0), Some(kind)), "{v}");
        v["template"].as_str().unwrap().to_string()
    };
    let (issue, milestone) = (template("issue"), template("milestone"));
    assert_eq!(commits(root), "1");
    let s = "projects/bp/designs/system";
    let codes = |v: &serde_json::Value| -> Vec<String> {
        v["findings"].as_array().unwrap().iter().map(|f| f["code"].as_str().unwrap().to_string()).collect()
    };
    let path = |name: &str, text: &str| {
        let path = root.join(".bp").join(name);
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    };
    let (m, i) = (path("m.md", &milestone), path("i.md", &issue));
    let (code, v) =
        bp(root, &["create", "milestone", "--project", "bp", "--title", "Tm", "--under", s, "--body-file", &m]);
    assert_eq!((code, codes(&v)), (Some(0), vec!["placeholder".to_string(); 4]), "a verbatim template: {v}");
    let (code, v) = bp(root, &["create", "issue", "--project", "bp", "--title", "Ti", "--under", M, "--body-file", &i]);
    assert_eq!((code, codes(&v)), (Some(0), vec!["placeholder".to_string(); 3]), "{v}");
    let filled = path("f.md", &issue.replace("TODO(bp): ", "Filled: "));
    let (code, v) =
        bp(root, &["create", "issue", "--project", "bp", "--title", "Tf", "--under", M, "--body-file", &filled]);
    assert_eq!((code, codes(&v)), (Some(0), vec![]), "a filled template: {v}");
    let (code, v) = bp(root, &["create", "run", "--template"]);
    assert_eq!((code, v["code"].as_str()), (Some(4), Some("usage")), "a record has no template: {v}");
}

#[test]
fn a_dry_run_on_a_vault_with_no_commit_refuses_as_the_write_does() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("projects")).unwrap();
    std::fs::create_dir_all(root.join("shared")).unwrap();
    git(root, &["init", "-q"]);
    let args = ["create", "project", "--title", "Recap", "--slug", "recap"];
    let (code, write) = bp(root, &args);
    assert_eq!((code, write["code"].as_str()), (Some(1), Some("could_not_run")), "{write}");
    let (code, dry) = bp(root, &[&args[..], &["--dry-run"]].concat());
    assert_eq!((code, &dry), (Some(1), &write), "the dry run refuses with the write's refusal");
    assert!(!root.join(".bp").exists(), "nothing touched the disk");
}
