//! The row checks of `bp record` through the binary (15, record; 29 design change 6): a review needs its `head:` line,
//! one findings table and full rows at places in the diff of its head; a declined disposition needs a reason.

mod common;

use std::path::{Path, PathBuf};

use common::{commits, git};
use serde_json::Value;

const ISSUE: &str = "projects/bp/issues/a";

/// An open issue with no claim, so no diff, under [`ISSUE`]'s milestone: a live follow-up.
const UNCLAIMED: &str = "projects/bp/issues/x";

/// The milestone of [`ISSUE`] and [`UNCLAIMED`].
const MILESTONE: &str = "projects/bp/milestones/m";

/// An issue under [`MILESTONE`] that is resolved, and one open under another milestone: neither is a follow-up.
const RESOLVED: &str = "projects/bp/issues/done";
const ELSEWHERE: &str = "projects/bp/issues/elsewhere";

/// An open thread under [`MILESTONE`]: live and under the milestone, but not an issue, so not a follow-up.
const THREAD: &str = "projects/bp/threads/t";

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
    let issue = |status: &str, under: &str| {
        format!("---\nkind: issue\ntitle: A\nstatus: {status}\nunder: {under}\n---\n\nBody.\n")
    };
    let milestone = "---\nkind: milestone\ntitle: M\nstatus: approved\n---\n\nGoal.\n";
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let vault = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{MILESTONE}.md"), milestone),
        ("projects/bp/milestones/n.md", milestone),
        (&format!("{ISSUE}.md"), &issue("open", MILESTONE)),
        (&format!("{UNCLAIMED}.md"), &issue("open", MILESTONE)),
        (&format!("{RESOLVED}.md"), &issue("resolved", MILESTONE)),
        (&format!("{ELSEWHERE}.md"), &issue("open", "projects/bp/milestones/n")),
        (&format!("{THREAD}.md"), &issue("open", MILESTONE).replace("kind: issue", "kind: thread")),
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

    /// A second fix after [`Self::fix`]: `src/a.rs` line 2.
    fn fix_again(&self) -> String {
        let text = "fn a() { fixed() }\nfn a() { again() }\n".to_string() + &"fn a() {}\n".repeat(10);
        std::fs::write(self.worktree.join("src/a.rs"), text).unwrap();
        git(&self.worktree, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "fix again"]);
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
    let (exit, v) = w.review(ISSUE, &w.body(&["| `risk` | src/a.rs:1 | It reads a secret. |"]));
    assert_eq!(exit, Some(0), "{v}");
}

#[test]
fn refuses_a_severity_outside_error_warn_and_risk() {
    let w = world();
    // A word the tool does not know reads as an error, so a meant warn would block the land in silence.
    for severity in ["warning", "Warn", "**warn**", "major", "Error"] {
        let fix =
            w.refused(ISSUE, &w.body(&["| warn | src/a.rs:2 | Fine. |", &format!("| {severity} | src/a.rs:1 | X. |")]));
        assert_eq!(fix, format!("row 2's severity is `{severity}`: write one of error, warn, risk"));
    }
}

#[test]
fn refuses_a_head_that_is_no_commit_of_the_clone() {
    let w = world();
    let bad = "1111111111111111111111111111111111111111";
    let before = commits(w.v());
    let (exit, v) = w.at(
        ISSUE,
        bad,
        &["round=1", "lens=conformance", "mode=full"],
        &format!("head: {bad}\n| severity | location | finding |\n|---|---|---|\n"),
    );
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("finding_invalid")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("after a fetch of the claim's branch"), "{v}");
    assert_eq!(commits(w.v()), before, "a refusal writes nothing");
    // A head that only origin holds yet is fetched, then known.
    let other = w.t.path().join("other");
    git(w.t.path(), &["clone", "-q", &w.t.path().join("origin.git").display().to_string(), "other"]);
    std::fs::write(other.join("src/c.rs"), "fn c() {}\n").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "pushed"]);
    git(&other, &["push", "-q", "origin", "HEAD:refs/heads/bp/a"]);
    let pushed = git(&other, &["rev-parse", "HEAD"]).trim().to_string();
    let body = format!("head: {pushed}\n| severity | location | finding |\n|---|---|---|\n");
    let (exit, v) = w.at(ISSUE, &pushed, &["round=1", "lens=conformance", "mode=full"], &body);
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
    assert_eq!(refused(&w.head, &["round=2", "lens=conformance", "mode=full"], "schema_invalid"), round_one);
    let open = |head: &str, lenses: &str| {
        format!(
            "round one of {ISSUE} is open at {head}: missing lenses: {lenses}; record each with --field round=1 \
             --field mode=full --field lens=<lens>"
        )
    };
    for fields in [&["round=1", "lens=conformance", "mode=verify"][..], &["round=1", "lens=verify", "mode=full"]] {
        assert_eq!(refused(&w.head, fields, "schema_invalid"), open(&w.head, "conformance, failure-paths"));
    }
    refused(&w.head, &["round=1", "lens=style", "mode=full"], "unknown_value");
    refused(&w.head, &["round=1", "mode=full"], "schema_invalid");
    refused(&w.head, &["round=1", "lens=conformance", "mode=full", "late=1"], "schema_invalid");
    let (exit, v) = w.at(ISSUE, &w.head, &["round=1", "lens=conformance", "mode=full"], &body);
    assert_eq!((exit, &v["late"]), (Some(0), &serde_json::json!([])), "{v}");
    // A new head before each lens of the level has a review at one head is still round one, and needs every lens
    // again (review of #98, finding 1).
    let two = w.fix();
    assert_eq!(
        refused(&two, &["round=2", "lens=verify", "mode=verify"], "schema_invalid"),
        open(&two, "conformance, failure-paths")
    );
    for lens in ["conformance", "failure-paths", "conformance"] {
        let (exit, v) =
            w.at(ISSUE, &two, &["round=1", &format!("lens={lens}"), "mode=full"], &body.replace(&w.head, &two));
        assert_eq!(exit, Some(0), "one record per lens, and a second reading: {v}");
    }
    // Each lens at one head closes round one: the next head is round 2, one `verify` review of the fix.
    let three = w.fix_again();
    let fix = refused(&three, &["round=1", "lens=conformance", "mode=full"], "schema_invalid");
    assert_eq!(fix, format!("{three} is round 2 of {ISSUE}: --field round=2 --field mode=verify"));
    let fix = refused(&three, &["round=2", "lens=conformance", "mode=verify"], "schema_invalid");
    assert_eq!(fix, "round 2 verifies the fix in one review: --field lens=verify");
}

#[test]
fn the_round_reads_each_review_as_its_first_commit_wrote_it() {
    let w = world();
    let empty = |head: &str| format!("head: {head}\n| severity | location | finding |\n|---|---|---|\n");
    for lens in ["conformance", "failure-paths"] {
        let (exit, v) = w.at(ISSUE, &w.head, &["round=1", &format!("lens={lens}"), "mode=full"], &empty(&w.head));
        assert_eq!(exit, Some(0), "{v}");
    }
    // A hand edit of a committed record's round changes nothing (review of #98, finding 11 a).
    let path = w.v().join(format!("{ISSUE}/reviews/{}.md", w.head));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("round: 1\n", "round: 7\n")).unwrap();
    git(w.v(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "round 7 by hand"]);
    let two = w.fix();
    let (exit, v) = w.at(ISSUE, &two, &["round=2", "lens=verify", "mode=verify"], &empty(&two));
    assert_eq!(exit, Some(0), "{v}");
}

#[test]
fn a_verify_review_marks_its_new_error_rows_outside_the_fix_late() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | src/b.rs:29 | Wrong. |"]));
    assert_eq!(exit, Some(0), "{v}");
    let first = v["id"].as_str().unwrap().to_string();
    let (exit, v) = w.at(ISSUE, &w.head, &["round=1", "lens=failure-paths", "mode=full"], &w.body(&[]));
    assert_eq!(exit, Some(0), "{v}");
    let two = w.fix();
    let rows = [
        "| error | src/a.rs:1 | The fix is wrong. | |",
        "| error | src/b.rs:30 | Wrong before the fix. | |",
        "| warn | src/b.rs:30 | Slow. | |",
        "| error | projects/bp/issues/x | A caller the fix breaks. | |",
        "| risk | src/b.rs:30 | It reads a secret. | |",
        &format!("| error | src/b.rs:30 | Still wrong. | `{first}#1` |"),
        "| error | src/b.rs:29 | Found again, with no `of` cell. | |",
    ];
    let body =
        format!("head: {two}\n\n| severity | location | finding | of |\n|---|---|---|---|\n{}\n", rows.join("\n"));
    // An `of` cell that names no earlier row is refused.
    for bad in [format!("{first}#2"), first.clone(), "projects/bp/issues/a/reviews/none#1".into()] {
        let body = body.replace(&format!("`{first}#1`"), &bad);
        let (exit, v) = w.at(ISSUE, &two, &["round=2", "lens=verify", "mode=verify"], &body);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("finding_invalid")), "{bad}: {v}");
        assert!(v["fix"].as_str().unwrap().starts_with("row 6's `of` cell"), "{v}");
    }
    // A new error at a path outside the fix is late; a node row and a re-report never are, and a row at an earlier
    // error row's location is a re-report (review of #98, findings 3 and 5, row 24).
    let (exit, v) = w.at(ISSUE, &two, &["round=2", "lens=verify", "mode=verify"], &body);
    assert_eq!((exit, &v["late"]), (Some(0), &serde_json::json!([2])), "{v}");
    let review = v["id"].as_str().unwrap().to_string();
    let (_, shown) = common::bp(w.v(), &["show", &review, "--no-body"]);
    assert_eq!((&shown["round"], &shown["lens"], &shown["mode"]), (&2.into(), &"verify".into(), &"verify".into()));
    // A late row is declined only with a live follow-up issue under the same milestone (review of #98, finding 4).
    let decline = |n: &str, reason: &str| {
        let (n, reason) = (format!("finding={n}"), format!("reason={reason}"));
        common::bp(
            w.v(),
            &["record", "disposition", &review, "--field", &n, "--field", "as=declined", "--field", &reason],
        )
    };
    for reason in [
        "out of scope".to_string(),
        format!("see {ISSUE}"),
        "follow-up projects/bp/issues/none".into(),
        format!("follow-up {MILESTONE}"),
        format!("follow-up {RESOLVED}"),
        format!("follow-up {ELSEWHERE}"),
        format!("follow-up {THREAD}"),
    ] {
        let (exit, v) = decline("2", &reason);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{reason}: {v}");
        let fix = v["fix"].as_str().unwrap();
        assert!(fix.starts_with("finding 2 is a late error outside the fix") && fix.contains(MILESTONE), "{v}");
    }
    let (exit, v) = decline("2", &format!("follow-up {UNCLAIMED}."));
    assert_eq!(exit, Some(0), "{v}");
    let (exit, v) = decline("1", "the reviewer misread the fix");
    assert_eq!(exit, Some(0), "a row the fix shows declines with any reason, and still blocks: {v}");
    // The late check reads the review as its first commit wrote it: a hand edit that marks row 6 late does not let
    // its decline pass with no follow-up.
    let path = w.v().join(format!("{review}.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("late:\n- 2\n", "late:\n- 2\n- 6\n").replace("late: [2]", "late: [2, 6]"))
        .unwrap();
    git(w.v(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "late by hand"]);
    let (exit, v) = decline("6", "out of scope");
    assert_eq!(exit, Some(0), "row 6 is no late row, so any reason declines it: {v}");
}

#[test]
fn an_of_cell_re_reports_only_from_an_error_row_of_an_earlier_head() {
    let w = world();
    let (exit, v) = w.review(ISSUE, &w.body(&["| error | src/a.rs:1 | Wrong. |"]));
    assert_eq!(exit, Some(0), "{v}");
    let first = v["id"].as_str().unwrap().to_string();
    let table = |head: &str, row: &str| {
        format!("head: {head}\n\n| severity | location | finding | of |\n|---|---|---|---|\n{row}\n")
    };
    let refused = |head: &str, fields: &[&str], row: &str| {
        let before = commits(w.v());
        let (exit, v) = w.at(ISSUE, head, fields, &table(head, row));
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("finding_invalid")), "{row}: {v}");
        assert_eq!(commits(w.v()), before, "a refusal writes nothing");
        assert!(v["fix"].as_str().unwrap().contains("only an `error` row re-reports"), "{v}");
    };
    // A second lens at the same head, as a warn or as an error, re-reports nothing (review of #98, row 20).
    let paths = ["round=1", "lens=failure-paths", "mode=full"];
    refused(&w.head, &paths, &format!("| warn | src/a.rs:1 | The same. | {first}#1 |"));
    refused(&w.head, &paths, &format!("| error | src/a.rs:1 | The same. | {first}#1 |"));
    let (exit, v) = w.at(ISSUE, &w.head, &paths, &table(&w.head, ""));
    assert_eq!(exit, Some(0), "{v}");
    // At a later head, a warn row still re-reports nothing; an error row does.
    let two = w.fix();
    let verify = ["round=2", "lens=verify", "mode=verify"];
    refused(&two, &verify, &format!("| warn | src/a.rs:1 | Fixed enough. | {first}#1 |"));
    let (exit, v) = w.at(ISSUE, &two, &verify, &table(&two, &format!("| error | src/a.rs:1 | Still. | {first}#1 |")));
    assert_eq!((exit, &v["late"]), (Some(0), &serde_json::json!([])), "{v}");
}

#[test]
fn a_cell_holds_an_escaped_pipe_in_each_table_shape() {
    let w = world();
    let finding = r"Wrong: `bp list issue --json \| jq .rows` prints 0.";
    let (exit, v) = w.review(ISSUE, &w.body(&[&format!("| error | src/a.rs:1 | {finding} |")]));
    assert_eq!((exit, v["rows"].as_u64()), (Some(0), Some(1)), "three columns: {v}");
    let first = v["id"].as_str().unwrap().to_string();
    let four = |head: &str, of: &str| {
        format!(
            "head: {head}\n\n| severity | location | finding | of |\n|---|---|---|---|\n| error | src/a.rs:1 | {finding} | {of} |\n"
        )
    };
    let (exit, v) = w.at(ISSUE, &w.head, &["round=1", "lens=failure-paths", "mode=full"], &four(&w.head, ""));
    assert_eq!((exit, v["rows"].as_u64()), (Some(0), Some(1)), "four columns, an empty `of`: {v}");
    let two = w.fix();
    let (exit, v) = w.at(ISSUE, &two, &["round=2", "lens=verify", "mode=verify"], &four(&two, &format!("{first}#1")));
    assert_eq!((exit, &v["late"]), (Some(0), &serde_json::json!([])), "the `of` cell is the fourth cell: {v}");
}

#[test]
fn refuses_a_head_at_or_behind_the_claims_base() {
    let w = world();
    let base = git(&w.worktree, &["rev-parse", "HEAD~1"]).trim().to_string();
    let orphan = git(&w.worktree, &["commit-tree", &format!("{}^{{tree}}", w.head), "-m", "orphan"]).trim().to_string();
    for head in [&base, &orphan] {
        let before = commits(w.v());
        let body = format!("head: {head}\n| severity | location | finding |\n|---|---|---|\n");
        let (exit, v) = w.at(ISSUE, head, &["round=1", "lens=conformance", "mode=full"], &body);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("finding_invalid")), "{head}: {v}");
        assert!(v["fix"].as_str().unwrap().contains("or does not descend from it"), "{v}");
        assert_eq!(commits(w.v()), before, "a refusal writes nothing");
    }
}

#[test]
fn a_review_at_a_head_whose_round_closed_refuses() {
    let w = world();
    let empty = |head: &str| format!("head: {head}\n| severity | location | finding |\n|---|---|---|\n");
    let lens =
        |head: &str, lens: &str| w.at(ISSUE, head, &["round=1", &format!("lens={lens}"), "mode=full"], &empty(head));
    assert_eq!(lens(&w.head, "conformance").0, Some(0));
    let two = w.fix();
    for l in ["conformance", "failure-paths"] {
        assert_eq!(lens(&two, l).0, Some(0), "round one again at the new head");
    }
    // The first head's round closed when the second head was reviewed (review of #98, row 27).
    let (exit, v) = lens(&w.head, "failure-paths");
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{v}");
    let want = format!("{} is round 1 of {ISSUE}, which closed when {two} was reviewed", w.head);
    assert!(v["fix"].as_str().unwrap().starts_with(&want), "{v}");
    // A `verify` review at the head that completed round one names the next head, not an empty list of lenses.
    let (exit, v) = w.at(ISSUE, &two, &["round=1", "lens=verify", "mode=verify"], &empty(&two));
    assert_eq!(exit, Some(2), "{v}");
    let want = format!(
        "{two} is round 1 of {ISSUE}, and each of its lenses is recorded: a `verify` review reads the next head"
    );
    assert_eq!(v["fix"], want.as_str());
}

#[test]
fn a_review_at_a_head_before_a_later_round_refuses_after_a_rebase() {
    let w = world();
    let empty = |head: &str| format!("head: {head}\n| severity | location | finding |\n|---|---|---|\n");
    for lens in ["conformance", "failure-paths"] {
        let (exit, v) = w.at(ISSUE, &w.head, &["round=1", &format!("lens={lens}"), "mode=full"], &empty(&w.head));
        assert_eq!(exit, Some(0), "{v}");
    }
    // A rebased head: the same tree on the claim's base, so the first head is not its ancestor.
    let tree = format!("{}^{{tree}}", w.head);
    let rebased = git(&w.worktree, &["commit-tree", &tree, "-p", "HEAD~1", "-m", "rebased"]).trim().to_string();
    let (exit, v) = w.at(ISSUE, &rebased, &["round=2", "lens=verify", "mode=verify"], &empty(&rebased));
    assert_eq!(exit, Some(0), "{v}");
    // Round 2 is recorded, so round one, at the first head, has closed (review of #98, row 27).
    let (exit, v) = w.at(ISSUE, &w.head, &["round=1", "lens=conformance", "mode=full"], &empty(&w.head));
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{v}");
    let want = format!("{} is round 1 of {ISSUE}, which closed when {rebased} was reviewed", w.head);
    assert!(v["fix"].as_str().unwrap().starts_with(&want), "{v}");
}

#[test]
fn a_stacked_claim_takes_a_head_rebased_off_its_base() {
    let w = world();
    // A claim stacked on the layer below starts at that layer's head; its land rebases this branch off it.
    let claim = format!(
        "---\nkind: claim\nunder: {ISSUE}\nfrom: 2026-10-19T09:00:00Z\nowner: worker\nworktree: {}\nbase: {}\n\
         base_branch: bp/x\nbase_issue: {UNCLAIMED}\n---\n",
        w.worktree.display(),
        w.head
    );
    std::fs::write(w.v().join(format!("{ISSUE}/claims/2026-10-19T09-00-00Z.md")), claim).unwrap();
    git(w.v(), &["add", "-A"]);
    git(w.v(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "stacked claim"]);
    let tree = format!("{}^{{tree}}", w.head);
    let rebased = git(&w.worktree, &["commit-tree", &tree, "-p", "HEAD~1", "-m", "rebased"]).trim().to_string();
    let empty = |head: &str| format!("head: {head}\n| severity | location | finding |\n|---|---|---|\n");
    let one = ["round=1", "lens=conformance", "mode=full"];
    let (exit, v) = w.at(ISSUE, &rebased, &one, &empty(&rebased));
    assert_eq!(exit, Some(0), "a head off the base, not behind it: {v}");
    let (exit, v) = w.at(ISSUE, &w.head, &one, &empty(&w.head));
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("finding_invalid")), "the base itself: {v}");
}
