//! `bp risk` through the binary (37, design changes 1 and 2): the level in force is the highest of the issue's label,
//! the claim's diff against the base branch's `.bp/risk.yml`, and each review's `risk` row.

mod common;

use std::path::{Path, PathBuf};

use common::{commits, git};
use serde_json::{Value, json};

const ISSUE: &str = "projects/bp/issues/a";

struct World {
    vault: tempfile::TempDir,
    t: tempfile::TempDir,
    code: PathBuf,
    worktree: Option<PathBuf>,
}

/// A vault whose issue holds `label` as its risk, a bare origin whose master holds `rules` as the risk file when
/// given, and its clone. Nothing is cut.
fn world(label: Option<&str>, rules: Option<&str>) -> World {
    let t = tempfile::tempdir().unwrap();
    let origin = t.path().join("origin.git");
    git(t.path(), &["init", "-q", "--bare", "-b", "master", "origin.git"]);
    git(t.path(), &["init", "-q", "-b", "master", "code"]);
    let code = t.path().join("code");
    git(&code, &["config", "user.name", "t"]);
    git(&code, &["config", "user.email", "t@t"]);
    write(&code, "src/b.rs", "fn b() {}\n");
    if let Some(rules) = rules {
        write(&code, ".bp/risk.yml", rules);
    }
    git(&code, &["add", "-A"]);
    git(&code, &["commit", "-qm", "one"]);
    git(&code, &["remote", "add", "origin", &origin.display().to_string()]);
    git(&code, &["push", "-q", "-u", "origin", "master"]);
    git(&code, &["remote", "set-head", "origin", "master"]);
    let risk = label.map(|l| format!("risk: {l}\n")).unwrap_or_default();
    let issue = format!("---\nkind: issue\ntitle: A\nstatus: open\n{risk}---\n\nBody.\n");
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let vault = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{ISSUE}.md"), &issue),
    ]);
    World { vault, t, code, worktree: None }
}

fn write(root: &Path, rel: &str, text: &str) {
    std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
    std::fs::write(root.join(rel), text).unwrap();
}

impl World {
    fn env(&self) -> [(&'static str, String); 1] {
        [("BP_VAULT", self.vault.path().display().to_string())]
    }

    /// `bp <args>` from the clone.
    fn bp(&self, args: &[&str]) -> (Option<i32>, Value) {
        let env = self.env();
        common::bp_env(&self.code, &[(env[0].0, &env[0].1)], args)
    }

    /// `bp risk` on the issue, which commits nothing to the vault.
    fn risk(&self) -> Value {
        let before = commits(self.vault.path());
        let (exit, v) = self.bp(&["risk", ISSUE]);
        assert_eq!((exit, commits(self.vault.path())), (Some(0), before), "{v}");
        v
    }

    fn cut(&mut self) {
        let (exit, out) = self.bp(&["cut", ISSUE]);
        assert_eq!(exit, Some(0), "{out}");
        self.worktree = Some(PathBuf::from(out["worktree"].as_str().unwrap()));
    }

    /// Writes `files` in the worktree and commits them on the claim's branch.
    fn commit(&self, files: &[(&str, &str)]) -> String {
        let w = self.worktree.as_ref().unwrap();
        for (rel, text) in files {
            write(w, rel, text);
        }
        git(w, &["add", "-A"]);
        git(w, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "two"]);
        git(w, &["rev-parse", "HEAD"]).trim().to_string()
    }
}

fn pick(v: &Value) -> Value {
    json!([v["level"], v["declared"], v["computed"]])
}

#[test]
fn with_no_claim_or_no_clone_the_level_is_the_label() {
    let mut w = world(None, None);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["medium", "medium", null]), "{v}");
    assert_eq!(v["raised_by"], json!([]));
    assert_eq!(v["reasons"], json!(["the issue declares no risk, which reads medium"]));
    w.cut();
    w.commit(&[("src/a.rs", "fn a() {}\n")]);
    let (exit, v) = common::bp(w.vault.path(), &["risk", ISSUE]);
    assert_eq!((exit, pick(&v)), (Some(0), json!(["medium", "medium", null])), "the vault is no clone: {v}");
    let (_, v) = common::bp(w.vault.path(), &["risk", "projects/bp/project"]);
    assert_eq!(v["code"], "no_node", "{v}");
}

#[test]
fn the_claims_diff_against_the_base_rules_sets_the_computed_level() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/, '*.md']\n"));
    w.cut();
    w.commit(&[("docs/a.md", "One.\n"), ("infrastructure.md", "Not infra.\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["low", "low", "low"]), "{v}");
    assert_eq!(
        v["reasons"],
        json!([
            "the issue declares low",
            "each changed path matches a low pattern, and 2 changed lines are under low_lines 150"
        ])
    );
    w.commit(&[("src/a.rs", "fn a() {}\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["medium", "low", "medium"]), "{v}");
    assert_eq!(v["reasons"], json!(["src/a.rs matches no low pattern"]));
    w.commit(&[("infra/main.tf", "x\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(v["reasons"], json!(["infra/main.tf matches the high pattern infra/"]));
}

#[test]
fn risk_file_edit_on_the_branch_is_high_and_its_rules_do_not_count() {
    let mut w = world(Some("low"), Some("high: [src/]\n"));
    w.cut();
    w.commit(&[(".bp/risk.yml", "low: [src/, .bp/]\n"), ("src/a.rs", "fn a() {}\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(
        v["reasons"],
        json!([
            ".bp/risk.yml is the risk file, and an edit to the gate is high",
            "src/a.rs matches the high pattern src/"
        ])
    );
}

#[test]
fn a_risk_row_raises_the_level_and_needs_no_disposition() {
    let mut w = world(Some("low"), Some("low: [docs/]\n"));
    w.cut();
    let head = w.commit(&[("docs/a.md", "One.\n")]);
    assert_eq!(pick(&w.risk()), json!(["low", "low", "low"]));
    let body = format!(
        "head: {head}\n\n| severity | location | finding |\n|---|---|---|\n| risk | docs/a.md:1 | It names a secret. |\n"
    );
    let file = w.t.path().join("review.md");
    std::fs::write(&file, body).unwrap();
    let field = format!("head={head}");
    let (exit, review) = w.bp(&["record", "review", ISSUE, "--field", &field, "--body-file", file.to_str().unwrap()]);
    assert_eq!(exit, Some(0), "{review}");
    let id = review["id"].as_str().unwrap();
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "low"]), "{v}");
    assert_eq!(v["raised_by"], json!([id]));
    assert_eq!(v["reasons"], json!([format!("review {id} holds a risk row")]));
}

#[test]
fn a_risk_file_that_does_not_parse_refuses() {
    let mut w = world(None, Some("hihg: [infra/]\n"));
    w.cut();
    w.commit(&[("src/a.rs", "fn a() {}\n")]);
    let (exit, v) = w.bp(&["risk", ISSUE]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{v}");
    assert_eq!(v["subject"], "refs/remotes/origin/master:.bp/risk.yml");
    assert!(v["fix"].as_str().unwrap().contains("\"hihg\" is no key"), "{v}");
}

#[test]
fn the_risk_label_is_a_closed_set_and_complex_a_bool() {
    let w = world(Some("severe"), None);
    let (exit, v) = w.bp(&["risk", ISSUE]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("unknown_value")), "a hand-written label: {v}");
    let root = w.vault.path();
    for (field, value, code) in
        [("risk", "high", 0), ("risk", "severe", 2), ("risk", "Low", 2), ("complex", "true", 0), ("complex", "yes", 2)]
    {
        let (exit, v) = common::bp(root, &["set", ISSUE, field, value]);
        assert_eq!(exit, Some(code), "{field} {value}: {v}");
    }
    let (_, v) = common::bp(root, &["risk", ISSUE]);
    assert_eq!(pick(&v), json!(["high", "high", null]), "{v}");
}
