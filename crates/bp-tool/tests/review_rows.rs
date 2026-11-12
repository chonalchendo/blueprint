//! The row checks of `bp record` through the binary (15, record; 29 design change 6): a review needs its `head:` line,
//! one findings table and full rows at places in the diff of its head; a declined disposition needs a reason.

mod common;

use std::path::{Path, PathBuf};

use common::{commits, git};
use serde_json::Value;

const ISSUE: &str = "projects/bp/issues/a";

/// An open issue with no claim, so no diff.
const UNCLAIMED: &str = "projects/bp/issues/x";

struct World {
    vault: tempfile::TempDir,
    t: tempfile::TempDir,
    head: String,
    worktree: PathBuf,
}

/// A vault, a bare origin and its clone, [`ISSUE`] cut, and one commit in its worktree: `src/b.rs` line 30 changed
/// (so lines 27 to 33 show), `src/a.rs` and `sp/a b.txt` added, and `src/old.rs` renamed whole to `src/new.rs`.
fn world() -> World {
    let t = tempfile::tempdir().unwrap();
    let origin = t.path().join("origin.git");
    git(t.path(), &["init", "-q", "--bare", "-b", "master", "origin.git"]);
    git(t.path(), &["init", "-q", "-b", "master", "code"]);
    let code = t.path().join("code");
    git(&code, &["config", "user.name", "t"]);
    git(&code, &["config", "user.email", "t@t"]);
    let forty = (1..=40).map(|n| format!("fn b{n}() {{}}")).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::create_dir_all(code.join("src")).unwrap();
    std::fs::write(code.join("src/b.rs"), &forty).unwrap();
    std::fs::write(code.join("src/old.rs"), "fn old() {}\n".repeat(6)).unwrap();
    git(&code, &["add", "-A"]);
    git(&code, &["commit", "-qm", "one"]);
    git(&code, &["remote", "add", "origin", &origin.display().to_string()]);
    git(&code, &["push", "-q", "-u", "origin", "master"]);
    git(&code, &["remote", "set-head", "origin", "master"]);
    let issue = "---\nkind: issue\ntitle: A\nstatus: open\n---\n\nBody.\n";
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let vault = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{ISSUE}.md"), issue),
        (&format!("{UNCLAIMED}.md"), issue),
    ]);
    let (exit, out) = common::bp_env(&code, &[("BP_VAULT", &vault.path().display().to_string())], &["cut", ISSUE]);
    assert_eq!(exit, Some(0), "{out}");
    let w = PathBuf::from(out["worktree"].as_str().unwrap());
    std::fs::write(w.join("src/b.rs"), forty.replace("fn b30() {}", "fn b30() { todo!() }")).unwrap();
    std::fs::write(w.join("src/a.rs"), "fn a() {}\n".repeat(12)).unwrap();
    std::fs::create_dir_all(w.join("sp")).unwrap();
    std::fs::write(w.join("sp/a b.txt"), "one\n").unwrap();
    git(&w, &["mv", "src/old.rs", "src/new.rs"]);
    git(&w, &["add", "-A"]);
    git(&w, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "two"]);
    let head = git(&w, &["rev-parse", "HEAD"]).trim().to_string();
    World { vault, t, head, worktree: w }
}

impl World {
    fn v(&self) -> &Path {
        self.vault.path()
    }

    /// `bp record review <issue>` at the head, with `body`, as round one's conformance review.
    fn review(&self, issue: &str, body: &str) -> (Option<i32>, Value) {
        self.at(issue, &self.head, &["round=1", "lens=conformance", "mode=full"], body)
    }

    /// `bp record review <issue>` at `head` with `fields` and `body`.
    fn at(&self, issue: &str, head: &str, fields: &[&str], body: &str) -> (Option<i32>, Value) {
        let file = self.t.path().join("review.md");
        std::fs::write(&file, body).unwrap();
        let head = format!("head={head}");
        let mut args = vec!["record", "review", issue, "--field", &head];
        for f in fields {
            args.extend(["--field", f]);
        }
        args.extend(["--body-file", file.to_str().unwrap()]);
        common::bp(self.v(), &args)
    }

    /// A commit in the claim's worktree that changes `src/a.rs` line 1: the fix a later round reads.
    fn fix(&self) -> String {
        std::fs::write(self.worktree.join("src/a.rs"), "fn a() { fixed() }\n".to_string() + &"fn a() {}\n".repeat(11))
            .unwrap();
        git(&self.worktree, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "fix"]);
        git(&self.worktree, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// A body with the head line and the table, one row per `(location, finding)`.
    fn body(&self, rows: &[&str]) -> String {
        format!("head: {}\n\n| severity | location | finding |\n|---|---|---|\n{}\n", self.head, rows.join("\n"))
    }

    /// The refusal of `body`: exit 2, `finding_invalid`, nothing committed. Returns the fix.
    fn refused(&self, issue: &str, body: &str) -> String {
        let before = commits(self.v());
        let (exit, v) = self.review(issue, body);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("finding_invalid")), "{body}: {v}");
        assert_eq!(commits(self.v()), before, "a refusal writes nothing");
        v["fix"].as_str().unwrap().to_string()
    }
}

#[test]
fn refuses_a_review_with_no_head_line() {
    let w = world();
    let table = "| severity | location | finding |\n|---|---|---|\n";
    let fix = w.refused(ISSUE, table);
    assert!(fix.contains(&format!("head: {}", w.head)), "the fix names the head: {fix}");
    w.refused(ISSUE, &format!("head: 0123456789\n{table}"));
    w.refused(ISSUE, &format!("head: {}\n{table}", &w.head[..6]));
}

#[test]
fn refuses_a_review_with_no_findings_table() {
    let w = world();
    let fix = w.refused(ISSUE, &format!("head: {}\n\nNo finding.\n", w.head));
    assert!(fix.contains("|---|---|---|"), "{fix}");
    w.refused(ISSUE, &format!("head: {}\n| severity | location | finding |\n| error | src/a.rs:1 | x |\n", w.head));
}

#[test]
fn refuses_an_empty_finding_row() {
    let w = world();
    for row in
        ["| error | src/a.rs:1 |  |", "| | src/a.rs:1 | Wrong. |", "| error | | Wrong. |", "| error | src/a.rs:1 |"]
    {
        let fix = w.refused(ISSUE, &w.body(&["| warn | src/a.rs:2 | Fine. |", row]));
        assert!(fix.starts_with("fill row 2"), "{row}: {fix}");
    }
}

#[test]
fn refuses_a_misspelt_risk_row() {
    let w = world();
    let near = ["Risk", "RISK", "**risk**", "risk:", "risk/error", "risk (raise)", "risk row", "ri\u{200b}sk", "Brisk"];
    for severity in near {
        let fix =
            w.refused(ISSUE, &w.body(&["| risk | src/a.rs:2 | Auth. |", &format!("| {severity} | src/a.rs:1 | X. |")]));
        assert_eq!(fix, format!("row 2's severity is `{severity}`: write `risk`, in lower case, to raise the level"));
    }
    let rows = ["| `risk` | src/a.rs:1 | It reads a secret. |", "| major | src/a.rs:2 | A past review's word. |"];
    let (exit, v) = w.review(ISSUE, &w.body(&rows));
    assert_eq!(exit, Some(0), "{v}");
}

#[test]
fn refuses_a_second_table() {
    let w = world();
    let rows = ["| warn | src/a.rs:2 | Fine. |", "|---|---|---|", "| warn | src/a.rs:3 | Fine. |"];
    let fix = w.refused(ISSUE, &w.body(&rows));
    assert!(fix.contains("second table"), "{fix}");
    let headed = ["| warn | src/a.rs:2 | Fine. |", "", "| x | y | z |", "|---|---|---|", "| a | b | c |"];
    let fix = w.refused(ISSUE, &w.body(&headed));
    assert_eq!(fix, "the body holds a second table; keep one findings table", "not a location fix for its header");
}

#[test]
fn refuses_a_location_outside_the_diff() {
    let w = world();
    for at in ["src/c.rs:1", "src/b.rs:5", "src/b.rs:20-24", "src/b.rs:30, 40", "README.md", "src/a.rs:13"] {
        let fix = w.refused(ISSUE, &w.body(&[&format!("| error | `{at}` | Wrong. |")]));
        assert!(fix.contains(at) && fix.contains("git diff"), "{at}: {fix}");
    }
}

#[test]
fn refuses_a_finding_on_an_issue_with_no_claim() {
    let w = world();
    let fix = w.refused(UNCLAIMED, &w.body(&["| error | src/a.rs:1 | Wrong. |"]));
    assert!(fix.contains("no claim"), "{fix}");
}

#[test]
fn refuses_a_declined_disposition_with_no_reason() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | src/a.rs:1 | Wrong. |", "| warn | src/b.rs:30 | Slow. |"]));
    assert_eq!(exit, Some(0), "{v}");
    let review = v["id"].as_str().unwrap();
    let before = commits(w.v());
    for extra in [&[][..], &["--field", "reason= "][..]] {
        let args = [&["record", "disposition", review, "--field", "finding=1", "--field", "as=declined"][..], extra];
        let (exit, v) = common::bp(w.v(), &args.concat());
        assert_eq!(
            (exit, v["code"].as_str(), v["subject"].as_str()),
            (Some(2), Some("schema_invalid"), Some("disposition reason")),
            "{v}"
        );
    }
    assert_eq!(commits(w.v()), before);
    let reason = "reason=the issue asks for no test of the skip";
    let (exit, v) = common::bp(
        w.v(),
        &["record", "disposition", review, "--field", "finding=1", "--field", "as=declined", "--field", reason],
    );
    assert_eq!(exit, Some(0), "a reason records: {v}");
    let fixed = format!("head={}", w.head);
    let (exit, v) = common::bp(
        w.v(),
        &["record", "disposition", review, "--field", "finding=2", "--field", "as=fixed", "--field", &fixed],
    );
    assert_eq!(exit, Some(0), "a fix needs no reason: {v}");
}

#[test]
fn a_review_whose_rows_sit_in_the_diff_records() {
    let w = world();
    let quoted = format!("Here it is.\nhead: `{}`\n| severity | location | finding |\n|---|---|---|\n", &w.head[..7]);
    let (exit, v) = w.review(ISSUE, &quoted);
    assert_eq!(exit, Some(0), "a short quoted head and a table with no row: {v}");
    let rows = [
        "| error | `src/a.rs:12` | Wrong. |",
        "| warn | src/b.rs:27 | Wrong. |",
        "| warn | src/b.rs:20-27 | Wrong. |",
        "| warn | src/b.rs:30, 33 | Wrong. |",
        "| warn | src/b.rs | Wrong. |",
        "| warn | projects/bp/project | Wrong. |",
        "| warn | src/a.rs:1 | Wrong. | extra |",
    ];
    let (exit, v) = w.review(ISSUE, &w.body(&rows));
    assert_eq!(exit, Some(0), "{v}");
    let (_, shown) = common::bp(w.v(), &["show", v["id"].as_str().unwrap()]);
    assert!(shown["body"].as_str().unwrap().contains("projects/bp/project"), "{shown}");
}

#[test]
fn refuses_nothing_for_a_path_with_a_space() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | sp/a b.txt:1 | Wrong. |", "| warn | sp/a b.txt | Wrong. |"]));
    assert_eq!(exit, Some(0), "git ends the name of a path with a space with a tab: {v}");
}

#[test]
fn a_renamed_file_with_no_hunk_is_cited_by_its_bare_path() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | src/new.rs | Wrong. |"]));
    assert_eq!(exit, Some(0), "a 100% rename shows no hunk: {v}");
    w.refused(ISSUE, &w.body(&["| error | src/new.rs:1 | Wrong. |"]));
}

#[test]
fn record_review_returns_rows() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | src/a.rs:12 | Wrong. |", "| warn | src/b.rs:30 | Slow. |"]));
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!((v["rows"].as_u64(), v["findings"].as_array().map(Vec::len)), (Some(2), Some(0)), "{v}");
    let (exit, v) = w.review(ISSUE, &w.body(&[]));
    assert_eq!((exit, v["rows"].as_u64()), (Some(0), Some(0)), "an empty table is no finding: {v}");
}

#[test]
fn a_review_names_the_round_lens_and_mode_of_its_head() {
    let w = world();
    let body = w.body(&["| error | src/a.rs:1 | Wrong. |"]);
    let refused = |head: &str, fields: &[&str], code: &str| {
        let before = commits(w.v());
        let (exit, v) = w.at(ISSUE, head, fields, &body.replace(&w.head, head));
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some(code)), "{fields:?}: {v}");
        assert_eq!(commits(w.v()), before, "a refusal writes nothing");
        v["fix"].as_str().unwrap().to_string()
    };
    let round_one = format!("{} is round 1 of {ISSUE}: --field round=1 --field mode=full", w.head);
    for fields in [&["round=2", "lens=conformance", "mode=full"][..], &["round=1", "lens=conformance", "mode=verify"]] {
        assert_eq!(refused(&w.head, fields, "schema_invalid"), round_one);
    }
    let fix = refused(&w.head, &["round=1", "lens=verify", "mode=full"], "schema_invalid");
    assert_eq!(
        fix,
        "round one reads one lens per review: --field lens=<conformance|failure-paths|tests-and-checks|security>"
    );
    refused(&w.head, &["round=1", "lens=style", "mode=full"], "unknown_value");
    refused(&w.head, &["round=1", "mode=full"], "schema_invalid");
    refused(&w.head, &["round=1", "lens=conformance", "mode=full", "late=1"], "schema_invalid");
    for lens in ["conformance", "failure-paths", "conformance"] {
        let (exit, v) = w.at(ISSUE, &w.head, &["round=1", &format!("lens={lens}"), "mode=full"], &body);
        assert_eq!(
            (exit, &v["late"]),
            (Some(0), &serde_json::json!([])),
            "one record per lens, and a second reading: {v}"
        );
    }
    // The next head with a review is round 2: one `verify` review of the fix.
    let two = w.fix();
    let fix = refused(&two, &["round=1", "lens=conformance", "mode=full"], "schema_invalid");
    assert_eq!(fix, format!("{two} is round 2 of {ISSUE}: --field round=2 --field mode=verify"));
    let fix = refused(&two, &["round=2", "lens=conformance", "mode=verify"], "schema_invalid");
    assert_eq!(fix, "round 2 verifies the fix in one review: --field lens=verify");
}

#[test]
fn a_verify_review_marks_its_error_rows_outside_the_fix_late() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | src/a.rs:1 | Wrong. |"]));
    assert_eq!(exit, Some(0), "{v}");
    let two = w.fix();
    let rows = [
        "| error | src/a.rs:1 | The fix is wrong. |",
        "| error | src/b.rs:30 | Wrong before the fix. |",
        "| warn | src/b.rs:30 | Slow. |",
        "| error | projects/bp/issues/x | A design the head contradicts. |",
        "| risk | src/b.rs:30 | It reads a secret. |",
    ];
    let body = w.body(&rows).replace(&w.head, &two);
    let (exit, v) = w.at(ISSUE, &two, &["round=2", "lens=verify", "mode=verify"], &body);
    assert_eq!((exit, &v["late"]), (Some(0), &serde_json::json!([2, 4])), "{v}");
    let review = v["id"].as_str().unwrap().to_string();
    let (_, shown) = common::bp(w.v(), &["show", &review, "--no-body"]);
    assert_eq!((&shown["round"], &shown["lens"], &shown["mode"]), (&2.into(), &"verify".into(), &"verify".into()));
    // A late row is declined only with a follow-up issue; the reviewed issue is none.
    let decline = |n: &str, reason: &str| {
        let (n, reason) = (format!("finding={n}"), format!("reason={reason}"));
        common::bp(
            w.v(),
            &["record", "disposition", &review, "--field", &n, "--field", "as=declined", "--field", &reason],
        )
    };
    for reason in ["out of scope", &format!("see {ISSUE}"), "follow-up projects/bp/issues/none"] {
        let (exit, v) = decline("2", reason);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{reason}: {v}");
        assert!(v["fix"].as_str().unwrap().starts_with("finding 2 is a late error outside the fix"), "{v}");
    }
    let (exit, v) = decline("2", &format!("follow-up {UNCLAIMED}."));
    assert_eq!(exit, Some(0), "{v}");
    let (exit, v) = decline("1", "the reviewer misread the fix");
    assert_eq!(exit, Some(0), "a row the fix shows declines with any reason, and still blocks: {v}");
}
