//! `bp set` through the binary, over a small vault under git: one line, one commit, the refusals, the no-op and
//! the dry run; and `--body-file`, which replaces the body and keeps the header byte for byte.

mod common;

use common::{bp, bp_env, commits, git};

const ISSUE: &str = "projects/bp/issues/0001.a";

fn vault() -> tempfile::TempDir {
    common::vault(&[
        (
            "projects/bp/issues/0001.a.md",
            "---\nkind: issue\ntitle: A\nstatus: open\nseverity: low\ntags: [\"x\"]\n---\n\nLead.\n",
        ),
        (
            "projects/bp/issues/0001.a/claims/s1.md",
            "---\nkind: claim\nunder: projects/bp/issues/0001.a\nowner: me\n---\n",
        ),
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\nActive.\n"),
        ("projects/bp/designs/product.md", "---\nkind: design\ntitle: P\nstatus: draft\n---\n\nDraft.\n"),
        ("shared/.keep", ""),
    ])
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
    assert_eq!(commits(root), "3");
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
    assert_eq!(commits(root), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn bp_role_is_a_closed_set_and_empty_reads_as_human() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp_env(root, &[("BP_ROLE", "bogus")], &["set", ISSUE, "severity", "high"]);
    assert_eq!((code, v["code"].as_str(), v["subject"].as_str()), (Some(4), Some("usage"), Some("BP_ROLE=bogus")));
    let (code, _) = bp_env(root, &[("BP_ROLE", "")], &["set", ISSUE, "severity", "high"]);
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
    assert_eq!(commits(root), "1");
}

#[test]
fn a_dry_run_writes_nothing() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["set", ISSUE, "severity", "high", "--dry-run"]);
    assert_eq!((code, v["dry_run"].as_bool(), v["value"].as_str()), (Some(0), Some(true), Some("high")));
    assert!(v.get("commit").is_none());
    assert_eq!(commits(root), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

/// A body file in the ignored `.bp/`, so the vault's working tree stays clean.
fn body(root: &std::path::Path, text: &str) -> String {
    let path = root.join(".bp/body.md");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

#[test]
fn a_body_file_replaces_the_body_and_keeps_the_header_bytes() {
    // An odd header: a quoted key, a padded value, a comment and no blank line before the body.
    let header = "---\nkind: issue\ntitle:   A\nstatus: open\n\"severity\": low   # kept\n---\n";
    let dir = common::vault(&[("projects/bp/issues/0001.a.md", &format!("{header}Old lead.\n")), ("shared/.keep", "")]);
    let root = dir.path();
    let f = body(root, "\u{feff}\n  \nNew lead.\r\n\n## Check\n\n```check shell {name=a expect=0}\ntrue\n```\n  \n");
    let (code, v) = bp(root, &["set", ISSUE, "--body-file", &f]);
    assert_eq!((code, v["field"].as_str()), (Some(0), Some("body")), "{v}");
    assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim());
    let text = std::fs::read_to_string(root.join(format!("{ISSUE}.md"))).unwrap();
    let want = "\nNew lead.\n\n## Check\n\n```check shell {name=a expect=0}\ntrue\n```\n";
    assert_eq!(text, format!("{header}{want}"));
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: set"));
    assert_eq!(bp(root, &["show", ISSUE]).1["lead"], "New lead.", "the index took the write");
    let f = body(root, "New lead.\n\n## Check\n\n```check shell {name=a expect=0}\ntrue\n```\n");
    let (code, v) = bp(root, &["set", ISSUE, "--body-file", &f]);
    assert_eq!((code, v["id"].as_str()), (Some(3), Some(ISSUE)), "the same body is a no-op: {v}");
    let f = body(root, "\n \n");
    assert_eq!(bp(root, &["set", ISSUE, "--body-file", &f]).0, Some(0));
    assert_eq!(std::fs::read_to_string(root.join(format!("{ISSUE}.md"))).unwrap(), header, "an empty body");
    assert_eq!(commits(root), "3");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn a_design_past_draft_refuses_design_not_delta() {
    let dir = vault();
    let root = dir.path();
    let f = body(root, "One change.\n");
    let (code, v) = bp(root, &["set", "projects/bp/designs/system", "--body-file", &f]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("design_not_delta")), "{v}");
    assert!(v["fix"].as_str().unwrap().starts_with("bp record delta projects/bp/designs/system "), "{v}");
    let (code, v) = bp(root, &["set", "projects/bp/designs/product", "--body-file", &f]);
    assert_eq!(code, Some(0), "a draft design takes a new body: {v}");
    let (code, v) = bp(root, &["set", "projects/bp/issues/0001.a/claims/s1", "--body-file", &f]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("not_a_label")), "a record is append-only: {v}");
    assert_eq!(commits(root), "2");
}

#[test]
fn a_body_file_takes_no_field_or_value() {
    let dir = vault();
    let root = dir.path();
    let f = body(root, "One change.\n");
    for args in [
        vec!["set", ISSUE, "severity", "high", "--body-file", &f],
        vec!["set", ISSUE, "severity", "--body-file", &f],
        vec!["set", ISSUE, "severity"],
        vec!["set", ISSUE],
    ] {
        let (code, v) = bp(root, &args);
        assert_eq!((code, v["code"].as_str()), (Some(4), Some("usage")), "{args:?}: {v}");
    }
    assert_eq!(commits(root), "1");
}

#[test]
fn a_body_passes_the_check_rules_as_create_does() {
    let dir = vault();
    let root = dir.path();
    for (text, code) in [
        ("## Check\n\n~~~check shell {expect=0}\ntrue\n~~~\n", "check_unnamed"),
        ("## Check\n\nProse only.\n", "check_empty"),
        ("## Check\n\n~~~check shell {name=a}\ntrue\n~~~\n~~~check shell {name=a}\ntrue\n~~~\n", "check_duplicate"),
    ] {
        let f = body(root, text);
        for dry in [&[][..], &["--dry-run"][..]] {
            let (exit, v) = bp(root, &[&["set", ISSUE, "--body-file", &f][..], dry].concat());
            assert_eq!((exit, v["code"].as_str()), (Some(2), Some(code)), "{dry:?}: {v}");
        }
    }
    assert_eq!(commits(root), "1");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn a_body_file_dry_run_shows_the_body_and_a_dirty_target_refuses() {
    let dir = vault();
    let root = dir.path();
    let f = body(root, "\nOne change.  \r\n\n");
    let (code, v) = bp(root, &["set", ISSUE, "--body-file", &f, "--dry-run"]);
    assert_eq!((code, v["dry_run"].as_bool(), v["field"].as_str()), (Some(0), Some(true), Some("body")), "{v}");
    assert_eq!(v["body"], "One change.", "the body as it would be stored: {v}");
    assert!(v.get("commit").is_none());
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    let path = root.join(format!("{ISSUE}.md"));
    let edited = std::fs::read_to_string(&path).unwrap() + "HUMAN WORK\n";
    std::fs::write(&path, &edited).unwrap();
    // A dry run refuses what the write refuses (contract.json, write_results.dry_run).
    for args in [
        vec!["set", ISSUE, "--body-file", &f],
        vec!["set", ISSUE, "--body-file", &f, "--dry-run"],
        vec!["set", ISSUE, "severity", "high", "--dry-run"],
    ] {
        let (code, v) = bp(root, &args);
        assert_eq!((code, v["code"].as_str()), (Some(2), Some("path_dirty")), "{args:?}: {v}");
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
    assert_eq!(commits(root), "1");
}

#[test]
fn a_body_file_that_holds_a_header_refuses() {
    let dir = vault();
    let root = dir.path();
    let path = root.join(format!("{ISSUE}.md"));
    let own = std::fs::read_to_string(&path).unwrap();
    let f = path.display().to_string();
    for args in [vec!["set", ISSUE, "--body-file", &f], vec!["set", ISSUE, "--body-file", &f, "--dry-run"]] {
        let (code, v) = bp(root, &args);
        assert_eq!((code, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{args:?}: {v}");
        assert!(v["fix"].as_str().unwrap().starts_with("pass the body only"), "{v}");
    }
    let f = body(root, &format!("\u{feff}\r\n{}", own.replace('\n', "\r\n")));
    let (code, v) = bp(root, &["set", ISSUE, "--body-file", &f]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("schema_invalid")), "a BOM and CRLF hide no header: {v}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), own);
    // A body that opens with a thematic break holds no `kind`, so it is a body.
    let f = body(root, "---\nA paragraph between rules.\n---\n");
    let (code, v) = bp(root, &["set", ISSUE, "--body-file", &f]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(bp(root, &["show", ISSUE]).1["body"], "---\nA paragraph between rules.\n---");
    assert_eq!(commits(root), "2");
}

#[test]
fn an_odd_header_keeps_its_bytes_and_takes_the_new_body() {
    // A BOM, CRLF, a lone CR (the parser's line end too) and a header closed at end of file. The new body is LF.
    for (header, old, sep) in [
        ("\u{feff}---\nkind: issue\ntitle: A\nstatus: open\n---\n", "\nOld lead.\n", ""),
        ("---\r\nkind: issue\r\ntitle: A\r\nstatus: open\r\n---\r\n", "\r\nOld lead.\r\n", ""),
        ("---\rkind: issue\rtitle: A\rstatus: open\r---\r", "\rOld lead.\r", "\n"),
        ("---\nkind: issue\ntitle: A\nstatus: open\n---", "", "\n"),
    ] {
        let dir = common::vault(&[("projects/bp/issues/0001.a.md", &format!("{header}{old}")), ("shared/.keep", "")]);
        let root = dir.path();
        let f = body(root, "New lead.\n");
        let (code, v) = bp(root, &["set", ISSUE, "--body-file", &f]);
        assert_eq!(code, Some(0), "{header:?}: {v}");
        let text = std::fs::read_to_string(root.join(format!("{ISSUE}.md"))).unwrap();
        assert_eq!(text, format!("{header}{sep}\nNew lead.\n"), "{header:?}");
        assert_eq!(bp(root, &["show", ISSUE]).1["lead"], "New lead.", "{header:?}");
        assert_eq!(bp(root, &["set", ISSUE, "--body-file", &f]).0, Some(3), "{header:?}: the same body");
        assert_eq!(commits(root), "2");
    }
}

#[test]
fn an_active_convention_takes_a_new_body() {
    // A convention revises with set --body-file; only a design past draft needs a delta (13).
    let dir = common::vault(&[
        ("shared/conventions/convention.c.md", "---\nkind: convention\ntitle: C\nstatus: active\n---\n\nOld rule.\n"),
        ("projects/.keep", ""),
    ]);
    let root = dir.path();
    let f = body(root, "New rule.\n");
    let (code, v) = bp(root, &["set", "shared/conventions/convention.c", "--body-file", &f]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim());
    assert_eq!(bp(root, &["show", "shared/conventions/convention.c"]).1["lead"], "New rule.");
}

#[test]
fn an_agent_role_refuses_the_humans_acts_before_any_other_test() {
    let dir = vault();
    let root = dir.path();
    // Ids that name nothing: the role refuses first, so no other refusal shows.
    let acts: [&[&str]; 4] = [
        &["stamp", "projects/bp/milestones/none"],
        &["land", "projects/bp/issues/none"],
        &["transition", "projects/bp/milestones/none", "approved"],
        &["transition", "projects/bp/designs/none", "active", "--dry-run"],
    ];
    for role in ["session", "worker", "reviewer", "researcher"] {
        for act in acts {
            let (code, v) = bp_env(root, &[("BP_ROLE", role)], act);
            assert_eq!((code, v["code"].as_str()), (Some(2), Some("role_forbidden")), "{role} {act:?}: {v}");
        }
    }
    let (_, v) = bp_env(root, &[("BP_ROLE", "session")], &["transition", "projects/bp/designs/none", "draft"]);
    assert_eq!(v["code"], "no_node", "another move is not the human's: {v}");
    let (_, v) = bp_env(root, &[("BP_ROLE", "")], &["transition", "projects/bp/designs/product", "active"]);
    assert!(v["commit"].is_string(), "the human activates: {v}");
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Role: human"));
}
