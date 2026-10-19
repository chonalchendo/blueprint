//! One file that does not parse, through the binary (25, issue 8): a finding, not a refusal of every other verb.

mod common;

use common::{bp, commits, git};

const BROKEN: &str = "projects/bp/issues/0001.a";
const OTHER: &str = "projects/bp/issues/0002.b";
const GOOD: &str = "---\nkind: issue\ntitle: A\nstatus: open\nseverity: low\n---\n\nLead.\n";

#[test]
fn a_file_that_does_not_parse_is_a_finding() {
    let dir = common::vault(&[
        ("projects/bp/issues/0001.a.md", GOOD),
        ("projects/bp/issues/0002.b.md", "---\nkind: issue\ntitle: B\nstatus: open\nseverity: low\n---\n"),
        (
            "projects/bp/issues/0003.c.md",
            &format!("---\nkind: issue\ntitle: C\nstatus: open\ncites: [\"{BROKEN}\"]\n---\n"),
        ),
        ("shared/.keep", ""),
    ]);
    let root = dir.path();
    let path = root.join(format!("{BROKEN}.md"));
    let census = || bp(root, &["list", "--count-only"]).1;
    assert_eq!(
        (census()["findings"]["schema_invalid"].as_u64(), census()["findings"]["unresolved_link"].as_u64()),
        (Some(0), Some(0))
    );

    std::fs::write(&path, GOOD.replacen("title: A\n", "title: A\ntitle: again\n", 1)).unwrap();
    git(root, &["commit", "-qam", "broken by hand"]);
    let (code, v) = bp(root, &["show", BROKEN]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{v}");
    let fix = v["fix"].as_str().unwrap();
    assert!(fix.contains("duplicated key") && fix.contains("by hand"), "{fix}");

    let (code, v) = bp(root, &["set", OTHER, "severity", "high"]);
    assert_eq!(code, Some(0), "a write to another file proceeds: {v}");
    let (code, v) = bp(root, &["link", OTHER, "cites", BROKEN]);
    assert_eq!(v["code"].as_str(), Some("unresolved_link"), "a new link to the file names nothing: {code:?}");
    let (_, v) = bp(root, &["set", BROKEN, "severity", "high"]);
    assert_eq!(v["code"].as_str(), Some("schema_invalid"), "a write to the file parses it");
    std::fs::write(&path, "---\nkind: issue\ntitle: [\n---\n").unwrap();
    let (_, v) = bp(root, &["set", BROKEN, "severity", "high"]);
    assert_eq!(v["code"].as_str(), Some("path_dirty"), "a dirty target refuses before the parse");
    git(root, &["checkout", "--", &format!("{BROKEN}.md")]);
    assert_eq!(commits(root), "3");

    let refreshed = census();
    assert_eq!(refreshed["findings"]["schema_invalid"], 1, "{refreshed}");
    assert_eq!(refreshed["findings"]["unresolved_link"], 1, "C's cite of the file: {refreshed}");
    assert_eq!((refreshed["files"].as_u64(), refreshed["nodes"]["issue"].as_u64()), (Some(3), Some(2)));
    let (_, v) = bp(root, &["list", "issue"]);
    assert!(v["rows"].as_array().unwrap().iter().all(|r| r["id"] != BROKEN), "{v}");
    assert_eq!(bp(root, &["search", "again"]).1["rows"], serde_json::json!([]));
    std::fs::remove_file(root.join(".bp/index.db")).unwrap();
    assert_eq!(census(), refreshed, "a rebuild from nothing gives the same census");

    std::fs::write(&path, GOOD).unwrap();
    git(root, &["commit", "-qam", "repaired by hand"]);
    let (code, v) = bp(root, &["show", BROKEN]);
    assert_eq!((code, v["title"].as_str()), (Some(0), Some("A")), "{v}");
    let repaired = census();
    assert_eq!(
        (repaired["findings"]["schema_invalid"].as_u64(), repaired["nodes"]["issue"].as_u64()),
        (Some(0), Some(3))
    );
    std::fs::remove_file(root.join(".bp/index.db")).unwrap();
    assert_eq!(census(), repaired);
}
