//! `bp show` and `bp list --count-only` through the binary, over a small vault and over the converted vault when
//! `BP_TEST_VAULT` names one.

use std::path::Path;

use assert_cmd::Command;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn vault() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "projects/bp/issues/0001.a.md",
        "---\nkind: issue\ntitle: A\nstatus: resolved\nunder: projects/bp/milestones/m\ncites: [\"projects/bp/designs/system\"]\nseverity: low\n---\n\nLead.\n\n## Problem\n\nThe problem, in full.\n\n## Direction\n\nThe direction.\n",
    );
    write(
        dir.path(),
        "projects/bp/issues/0001.a/claims/s1.md",
        "---\nkind: claim\nunder: projects/bp/issues/0001.a\nowner: me\n---\n",
    );
    write(dir.path(), "shared/assets/glossary.md", "# Glossary\n");
    dir
}

fn bp(root: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let out = Command::cargo_bin("bp").unwrap().current_dir(root).env_remove("BP_VAULT").args(args).output().unwrap();
    let value = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    (out.status.code(), value)
}

#[test]
fn show_flattens_the_header_and_lists_the_sections() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0001.a", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(v["kind"], "issue");
    assert_eq!(v["status"], "resolved");
    assert_eq!(v["under"], "projects/bp/milestones/m");
    assert_eq!(v["cites"][0], "projects/bp/designs/system");
    assert_eq!(v["severity"], "low");
    assert_eq!(v["record"], false);
    assert_eq!(v["lead"], "Lead.");
    assert_eq!(v["sections"], serde_json::json!(["Problem", "Direction"]));
    assert!(v["body"].as_str().unwrap().contains("## Direction"));
    assert_eq!(v["hash"].as_str().unwrap().len(), 64);
}

#[test]
fn show_one_section_or_refuse_with_the_headings() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0001.a", "--section", "Problem", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(v["section"], "The problem, in full.");
    assert!(v.get("body").is_none());
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0001.a", "--section", "Nope", "--json"]);
    assert_eq!(code, Some(2));
    assert_eq!(v["code"], "no_section");
    assert_eq!(v["fix"], "bp show projects/bp/issues/0001.a --no-body; the headings are: Problem, Direction");
}

#[test]
fn show_a_record_and_refuse_a_missing_id() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0001.a/claims/s1", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(v["record"], true);
    assert_eq!(v["owner"], "me");
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/none", "--json"]);
    assert_eq!(code, Some(2));
    assert_eq!(v["code"], "no_node");
}

#[test]
fn list_counts_files_nodes_and_records() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["list", "--count-only", "--json"]);
    assert_eq!(code, Some(0));
    assert_eq!(v["files"], 3);
    assert_eq!(v["no_header"], 1);
    assert_eq!(v["nodes"]["issue"], 1);
    assert_eq!(v["records"]["claim"], 1);
    let (code, _) = bp(dir.path(), &["list", "--json"]);
    assert_eq!(code, Some(4));
}

#[test]
fn list_rows_carry_total_truncated_and_the_contract_version() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["list", "issue", "--project", "bp", "--limit", "1", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["rows"][0]["id"], "projects/bp/issues/0001.a");
    assert_eq!(v["rows"][0]["status"], "resolved");
    assert_eq!((v["total"].as_u64(), v["truncated"].as_bool()), (Some(1), Some(false)));
    assert_eq!(v["contract_version"], 1);
    let (code, _) = bp(dir.path(), &["list", "issue", "--count-only", "--json"]);
    assert_eq!(code, Some(4));
}

#[test]
fn search_returns_id_kind_rank_and_snippet() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["search", "problem", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    let row = &v["rows"][0];
    assert_eq!(row["id"], "projects/bp/issues/0001.a");
    assert_eq!(row["kind"], "issue");
    assert!(row["rank"].as_f64().unwrap() > 0.0);
    assert!(row["snippet"].as_str().unwrap().contains("«problem»"));
    let (code, _) = bp(dir.path(), &["search", "--json"]);
    assert_eq!(code, Some(4));
}

/// The converted vault: every file reads, and the census matches the converter's report when it sits beside it.
#[test]
fn the_converted_vault_reads_whole() {
    let Some(root) = std::env::var_os("BP_TEST_VAULT") else { return };
    let root = Path::new(&root);
    let (code, v) = bp(root, &["list", "--count-only", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    let (code, s) = bp(root, &["search", "walk budget", "--json"]);
    assert_eq!(code, Some(0), "{s}");
    assert!(s["rows"][0]["snippet"].is_string());
    let nodes: u64 = v["nodes"].as_object().unwrap().values().map(|n| n.as_u64().unwrap()).sum();
    let records: u64 = v["records"].as_object().unwrap().values().map(|n| n.as_u64().unwrap()).sum();
    assert_eq!(nodes + records + v["no_header"].as_u64().unwrap(), v["files"].as_u64().unwrap());
    let (code, v) = bp(root, &["show", "projects/bp/designs/system", "--section", "Solution strategy", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    assert!(v["section"].as_str().unwrap().len() > 100);
}

#[test]
fn refusals_carry_closed_codes() {
    let dir = vault();
    let cases: [(&[&str], i32, &str); 6] = [
        (&["show", "/etc/passwd"], 2, "no_node"),
        (&["show", "projects/../../etc/passwd"], 2, "no_node"),
        (&["show", "shared/assets/glossary"], 2, "no_node"),
        (&["list", "issues"], 2, "unknown_value"),
        (&["list", "milestone", "--status", "in-progress"], 2, "unknown_value"),
        (&["list", "--limit", "x"], 4, "usage"),
    ];
    for (args, exit, code) in cases {
        let mut all = args.to_vec();
        all.push("--json");
        let (got, v) = bp(dir.path(), &all);
        assert_eq!((got, v["code"].as_str()), (Some(exit), Some(code)), "{args:?}: {v}");
        assert!(v.get("contract_version").is_none(), "a refusal carries the five envelope fields only");
    }
}

#[test]
fn the_census_reports_findings() {
    let dir = vault();
    write(
        dir.path(),
        "projects/bp/issues/0002.b.md",
        "---\nkind: issue\ntitle: B\nstatus: escalated\nunder: '[[milestone.anvil.x]]'\n---\n",
    );
    let (code, v) = bp(dir.path(), &["list", "--count-only", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["findings"]["status_outside_set"], 1);
    // The legacy `under` text, plus 0001.a's milestone and design, which no file holds.
    assert_eq!(v["findings"]["unresolved_link"], 3);
}

#[test]
fn a_duplicate_heading_is_a_finding_and_its_section_refuses() {
    let dir = vault();
    write(
        dir.path(),
        "projects/bp/issues/0003.c.md",
        "---\nkind: issue\ntitle: C\nstatus: open\n---\n\n## Check\n\n```check shell {name=one expect=0}\ntrue\n```\n\n## Check\n\n```bash\nnever closed\n",
    );
    let (code, v) = bp(dir.path(), &["list", "--count-only", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["check_blocks"], 1);
    assert_eq!(v["findings"]["duplicate_heading"], 1);
    assert_eq!(v["findings"]["unclosed_fence"], 1);
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0003.c", "--section", "Check", "--json"]);
    assert_eq!(code, Some(2), "{v}");
    assert_eq!(v["code"], "no_section");
    assert_eq!(v["fix"], "bp show projects/bp/issues/0003.c --json");
    let (_, v) = bp(dir.path(), &["show", "projects/bp/issues/0003.c", "--json"]);
    let kinds: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["duplicate_heading", "unclosed_fence"]);
}

#[test]
fn show_without_the_body_keeps_the_header_lead_and_headings() {
    let dir = vault();
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0001.a", "--no-body", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    assert!(v.get("body").is_none());
    assert_eq!(v["lead"], "Lead.");
    assert_eq!(v["sections"], serde_json::json!(["Problem", "Direction"]));
}

#[test]
fn show_at_a_commit_reads_the_file_as_it_was() {
    let dir = vault();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").current_dir(dir.path()).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "add", "."]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"]);
    let id = "projects/bp/issues/0001.a";
    let path = dir.path().join(format!("{id}.md"));
    let now = std::fs::read_to_string(&path).unwrap().replace("status: resolved", "status: open");
    std::fs::write(&path, now).unwrap();
    let (code, v) = bp(dir.path(), &["show", id, "--at", "HEAD", "--json"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["status"], "resolved");
    let (code, v) = bp(dir.path(), &["show", id, "--at", "no-such-ref", "--json"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("unknown_value")));
    let (code, v) = bp(dir.path(), &["show", "projects/bp/issues/0009.none", "--at", "HEAD", "--json"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("no_node")));
}
