//! `bp risk` through the binary (37, design changes 1 and 2): the level in force is the highest of the issue's label,
//! the pull request head's diff against the base commit's `.bp/risk.yml`, each review's `risk` row, and a pending
//! delta the issue caused.

mod common;

use std::path::{Path, PathBuf};

use common::{commits, git};
use serde_json::{Value, json};

const ISSUE: &str = "projects/bp/issues/a";

/// An issue that depends on [`ISSUE`] once linked.
const B: &str = "projects/bp/issues/b";

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
    write(&code, "src/big.rs", &"fn big() {}\n".repeat(30));
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
        (&format!("{B}.md"), "---\nkind: issue\ntitle: B\nstatus: open\n---\n\nBody.\n"),
        ("projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: approved\n---\n"),
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n"),
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
        commit_in(self.worktree.as_ref().unwrap(), files)
    }

    /// Commits `files` on master in a second clone and pushes them: origin's master moves, and the first clone does
    /// not see it until a fetch.
    fn push_master(&self, files: &[(&str, &str)]) -> String {
        let other = self.t.path().join("other");
        if !other.exists() {
            let origin = self.t.path().join("origin.git").display().to_string();
            git(self.t.path(), &["clone", "-q", &origin, "other"]);
        }
        git(&other, &["pull", "-q", "origin", "master"]);
        let head = commit_in(&other, files);
        git(&other, &["push", "-q", "origin", "master"]);
        head
    }

    /// The sha of `rev` in the clone.
    fn rev(&self, rev: &str) -> String {
        git(&self.code, &["rev-parse", rev]).trim().to_string()
    }
}

impl World {
    /// `bp record review` at `head` with `round`, `lens` and the mode of the round, and one row per item of `rows`.
    fn review(&self, head: &str, round: u64, lens: &str, rows: &[&str]) -> Value {
        let table = format!("head: {head}\n\n| severity | location | finding |\n|---|---|---|\n{}\n", rows.join("\n"));
        let file = self.t.path().join("review.md");
        std::fs::write(&file, table).unwrap();
        let (head, round) = (format!("head={head}"), format!("round={round}"));
        let (lens, mode) = (format!("lens={lens}"), if round == "round=1" { "mode=full" } else { "mode=verify" });
        let fields = ["--field", &head, "--field", &round, "--field", &lens, "--field", mode];
        let (exit, v) =
            self.bp(&[&["record", "review", ISSUE][..], &fields, &["--body-file", file.to_str().unwrap()]].concat());
        assert_eq!(exit, Some(0), "{v}");
        v
    }
}

fn commit_in(dir: &Path, files: &[(&str, &str)]) -> String {
    for (rel, text) in files {
        write(dir, rel, text);
    }
    git(dir, &["add", "-A"]);
    git(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "two"]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn pick(v: &Value) -> Value {
    json!([v["level"], v["declared"], v["computed"]])
}

#[test]
fn with_no_claim_or_no_clone_the_level_is_the_label() {
    let mut w = world(None, None);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["medium", "medium", null]), "{v}");
    assert_eq!((&v["raised_by"], &v["head"], &v["base"]), (&json!([]), &json!([]), &Value::Null), "{v}");
    assert_eq!(v["reasons"], json!(["the issue declares no risk, which reads medium"]));
    w.cut();
    w.commit(&[("src/a.rs", "fn a() {}\n")]);
    let (exit, v) = common::bp(w.vault.path(), &["risk", ISSUE]);
    assert_eq!((exit, pick(&v)), (Some(0), json!(["medium", "medium", null])), "the vault is no clone: {v}");
    // A node of another kind refuses as `cut` refuses it; an absent id names no node.
    for other in ["projects/bp/project", "projects/bp/milestones/m"] {
        let (exit, v) = common::bp(w.vault.path(), &["risk", other]);
        let (_, cut) = w.bp(&["cut", other]);
        assert_eq!((exit, v["code"].as_str()), (Some(2), cut["code"].as_str()), "{v} {cut}");
        assert_eq!(v["code"], "illegal_transition", "{v}");
    }
    let (_, v) = common::bp(w.vault.path(), &["risk", "projects/bp/issues/none"]);
    assert_eq!(v["code"], "no_node", "{v}");
}

#[test]
fn an_empty_risk_label_reads_medium_and_says_so() {
    let w = world(Some(""), None);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["medium", "medium", null]), "{v}");
    assert_eq!(v["reasons"], json!(["the issue declares no risk, which reads medium"]));
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
    let head = w.commit(&[("infra/main.tf", "x\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(v["reasons"], json!(["infra/main.tf matches the high pattern infra/"]));
    assert_eq!((&v["head"], &v["base"]), (&json!([head]), &json!(w.rev("origin/master"))), "{v}");
}

#[test]
fn a_case_variant_of_a_high_path_is_high() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [src/, Infra/, .BP/, .Claude/]\n"));
    w.cut();
    w.commit(&[("INFRA/main.tf", "x\n")]);
    assert_eq!(w.risk()["reasons"], json!(["INFRA/main.tf matches the high pattern infra/"]));
    // A case-folding volume writes `.BP/risk.yml` into the `.bp/risk.yml` master holds, so the tree gets it by hand.
    let wt = w.worktree.clone().unwrap();
    let blob = w.t.path().join("blob");
    std::fs::write(&blob, "low: [infra/]\n").unwrap();
    let blob = git(&wt, &["hash-object", "-w", blob.to_str().unwrap()]).trim().to_string();
    for path in [".BP/risk.yml", ".Claude/settings.json"] {
        git(&wt, &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")]);
    }
    git(&wt, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "case"]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    let reasons = v["reasons"].as_array().unwrap();
    for want in [".BP/risk.yml is the risk file", ".Claude/settings.json is under the built-in high path .claude/"] {
        assert!(reasons.iter().any(|r| r.as_str().unwrap().starts_with(want)), "{want}: {v}");
    }
}

#[test]
fn the_gates_own_paths_are_high_with_no_risk_file() {
    let mut w = world(Some("low"), None);
    w.cut();
    w.commit(&[(".github/workflows/bp-gate.yml", "on: push\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(
        v["reasons"],
        json!([".github/workflows/bp-gate.yml is under the built-in high path .github/workflows/"])
    );
}

#[test]
fn a_link_named_as_a_high_directory_is_high() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\n"));
    w.cut();
    let wt = w.worktree.clone().unwrap();
    write(&wt, "docs/x/settings.json", "{\"hooks\":{}}\n");
    for link in [".codex", ".agents", "infra"] {
        std::os::unix::fs::symlink("docs/x", wt.join(link)).unwrap();
    }
    w.commit(&[]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(
        v["reasons"],
        json!([
            ".agents is at or above the built-in high path .agents/",
            ".codex is at or above the built-in high path .codex/",
            "infra matches the high pattern infra/"
        ])
    );
}

#[test]
fn a_non_ascii_path_is_high() {
    let mut w = world(Some("low"), Some("high: [secrets/]\nlow: [docs/]\n"));
    w.push_master(&[("secrets/prod.env", "k\n")]);
    w.cut();
    // APFS opens `\u{17f}ecrets/` as `secrets/`, so the tree gets the path by hand, as an agent's index would.
    let wt = w.worktree.clone().unwrap();
    let blob = w.t.path().join("evil");
    std::fs::write(&blob, "evil\n").unwrap();
    let blob = git(&wt, &["hash-object", "-w", blob.to_str().unwrap()]).trim().to_string();
    git(&wt, &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},\u{17f}ecrets/prod.env")]);
    git(&wt, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "fold"]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(v["reasons"], json!(["non-ASCII path: \u{17f}ecrets/prod.env"]));
}

#[test]
fn the_level_is_of_the_higher_of_the_local_and_the_pushed_head() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\n"));
    w.cut();
    let wt = w.worktree.clone().unwrap();
    // A pushed head with a high path, and the local branch moved back to a low one.
    let pushed = w.commit(&[("infra/main.tf", "x\n")]);
    git(&wt, &["push", "-q", "origin", "HEAD:bp/a"]);
    git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
    let local = w.commit(&[("docs/a.md", "One.\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(v["head"], json!([local, pushed]), "{v}");
    // The other way: an unpushed high commit on a pushed low head.
    git(&wt, &["push", "-q", "-f", "origin", "HEAD:bp/a"]);
    assert_eq!(pick(&w.risk()), json!(["low", "low", "low"]), "one head when both agree");
    let local = w.commit(&[("infra/b.tf", "y\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(v["head"].as_array().unwrap().first(), Some(&json!(local)), "{v}");
    // A tie keeps the local branch's reasons.
    git(&wt, &["push", "-q", "-f", "origin", "HEAD:bp/a"]);
    git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
    w.commit(&[("infra/c.tf", "z\n")]);
    assert_eq!(w.risk()["reasons"], json!(["infra/c.tf matches the high pattern infra/"]));
}

#[test]
fn the_rules_and_the_base_are_fetched_and_a_failed_fetch_refuses() {
    let mut w = world(Some("low"), Some("low: [docs/]\n"));
    w.cut();
    w.commit(&[("docs/a.md", "One.\n")]);
    assert_eq!(pick(&w.risk()), json!(["low", "low", "low"]));
    // Master moves after the cut: its new rules judge the branch, and its new paths are not the branch's (three dots).
    let base = w.push_master(&[(".bp/risk.yml", "high: [docs/]\nlow: [docs/]\n"), ("infra/x.tf", "x\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!((&v["base"], &v["reasons"]), (&json!(base), &json!(["docs/a.md matches the high pattern docs/"])));
    let origin = w.t.path().join("origin.git");
    std::fs::rename(&origin, w.t.path().join("moved.git")).unwrap();
    let (exit, v) = w.bp(&["risk", ISSUE]);
    assert_eq!((exit, v["code"].as_str()), (Some(1), Some("could_not_run")), "old refs are not judged: {v}");
}

/// The base and the pushed head are the shas origin's server names, so the clone's own refspec, which an agent edits
/// with no push, cannot set either.
#[test]
fn a_refspec_in_the_clone_moves_neither_the_base_nor_the_head() {
    let mut w = world(Some("low"), Some("high: [infra/, src/]\nlow: [docs/]\n"));
    w.cut();
    let wt = w.worktree.clone().unwrap();
    let base = w.rev("origin/master");
    // A lenient branch on origin, mapped onto `origin/master`; nothing maps `bp/a`, so `origin/bp/a` never moves.
    let lenient = w.push_master(&[(".bp/risk.yml", "low: [src/, infra/, docs/]\n")]);
    git(&w.t.path().join("other"), &["push", "-q", "-f", "origin", &format!("{lenient}:refs/heads/lenient")]);
    git(&w.t.path().join("other"), &["push", "-q", "-f", "origin", &format!("{base}:refs/heads/master")]);
    git(&w.code, &["config", "--unset-all", "remote.origin.fetch"]);
    git(&w.code, &["config", "--add", "remote.origin.fetch", "+refs/heads/lenient:refs/remotes/origin/master"]);
    git(&w.code, &["fetch", "-q", "origin"]);
    // The pushed head edits a high path; the local branch is moved back to a low commit.
    let pushed = w.commit(&[("src/big.rs", "fn big() {}\n")]);
    git(&wt, &["push", "-q", "origin", "HEAD:bp/a"]);
    git(&wt, &["reset", "-q", "--hard", "HEAD~1"]);
    let local = w.commit(&[("docs/a.md", "One.\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!((&v["base"], &v["head"]), (&json!(base), &json!([local, pushed])), "{v}");
    assert_eq!(v["reasons"], json!(["src/big.rs matches the high pattern src/"]));
    assert_eq!(w.rev("origin/master"), lenient, "the poisoned ref moved, and the level did not read it");
}

#[test]
fn the_diff_is_from_the_merge_base_so_master_s_own_changes_do_not_count() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\n"));
    w.cut();
    w.commit(&[("docs/a.md", "One.\n")]);
    w.push_master(&[("infra/main.tf", "x\n")]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["low", "low", "low"]), "{v}");
}

#[test]
fn a_git_rename_counts_both_paths_and_no_lines() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\nlimits: {low_lines: 10, high_lines: 20}\n"));
    w.cut();
    let wt = w.worktree.clone().unwrap();
    // A whole-file move of 30 lines: 0 changed lines, where a delete and an add would count 60.
    std::fs::create_dir(wt.join("lib")).unwrap();
    git(&wt, &["mv", "src/big.rs", "lib/big.rs"]);
    w.commit(&[]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["medium", "low", "medium"]), "{v}");
    assert_eq!(v["reasons"], json!(["lib/big.rs matches no low pattern"]));
    // Out of a high directory: the old path counts.
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\n"));
    w.push_master(&[("infra/big.txt", &"line\n".repeat(30))]);
    w.cut();
    let wt = w.worktree.clone().unwrap();
    std::fs::create_dir(wt.join("docs")).unwrap();
    git(&wt, &["mv", "infra/big.txt", "docs/big.txt"]);
    w.commit(&[]);
    assert_eq!(w.risk()["reasons"], json!(["infra/big.txt matches the high pattern infra/"]));
}

#[test]
fn a_size_git_does_not_count_is_high_outside_the_low_patterns() {
    let mut w = world(Some("low"), Some("low: [docs/]\nlimits: {low_lines: 10, high_lines: 20}\n"));
    w.cut();
    // An untracked attribute in the clone's own checkout hides the size from numstat; the diff does not show it.
    write(&w.code, ".gitattributes", "*.rs -diff\n*.md -diff\n");
    w.commit(&[("docs/a.md", "One.\n")]);
    assert_eq!(pick(&w.risk()), json!(["low", "low", "low"]), "an unknown size under a low pattern counts 0");
    w.commit(&[("src/a.rs", &"fn a() {}\n".repeat(41))]);
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "high"]), "{v}");
    assert_eq!(v["reasons"], json!(["size unknown: src/a.rs"]));
}

#[test]
fn a_pending_delta_raises_the_level_with_or_without_a_clone() {
    let mut w = world(Some("low"), Some("low: [docs/]\n"));
    w.cut();
    w.commit(&[("docs/a.md", "One.\n")]);
    assert_eq!(pick(&w.risk()), json!(["low", "low", "low"]));
    let delta = format!(
        "---\nkind: delta\nunder: projects/bp/designs/system\nmilestone: projects/bp/milestones/m\ncause: {ISSUE}\n---\n\n## One\n\nNew.\n"
    );
    let root = w.vault.path();
    write(root, "projects/bp/designs/system/deltas/d.md", &delta);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "delta"]);
    let reason = json!(["the issue caused a pending delta, and a design change is high"]);
    let v = w.risk();
    assert_eq!((pick(&v), &v["reasons"]), (json!(["high", "low", "low"]), &reason), "in the clone: {v}");
    let (exit, v) = common::bp(root, &["risk", ISSUE]);
    assert_eq!((exit, pick(&v), &v["reasons"]), (Some(0), json!(["high", "low", null]), &reason), "no clone: {v}");
}

#[test]
fn a_stacked_claim_is_judged_against_its_dependency_s_branch() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\n"));
    let path = fake_gh(w.t.path());
    w.cut();
    let head = w.commit(&[("infra/main.tf", "x\n")]);
    git(w.worktree.as_ref().unwrap(), &["push", "-q", "origin", "HEAD:bp/a"]);
    open_pr(w.t.path(), "a", &head);
    assert_eq!(common::bp(w.vault.path(), &["link", B, "depends", ISSUE]).0, Some(0));
    let vault = w.vault.path().display().to_string();
    let env = [("BP_VAULT", vault.as_str()), ("PATH", path.as_str())];
    let (exit, out) = common::bp_env(&w.code, &env, &["cut", B]);
    assert_eq!((exit, out["base_branch"].as_str()), (Some(0), Some("bp/a")), "{out}");
    let wb = PathBuf::from(out["worktree"].as_str().unwrap());
    commit_in(&wb, &[("docs/b.md", "B.\n")]);
    let (exit, v) = common::bp_env(&w.code, &env, &["risk", B]);
    assert_eq!((exit, &v["computed"], &v["base"]), (Some(0), &json!("low"), &json!(head)), "{v}");
}

#[test]
fn git_s_location_variables_do_not_send_the_level_to_another_repository() {
    let mut w = world(Some("low"), Some("high: [infra/]\nlow: [docs/]\n"));
    w.cut();
    w.commit(&[("docs/a.md", "One.\n")]);
    let want = w.risk();
    let other = w.t.path().join("other");
    w.push_master(&[("infra/x.tf", "x\n")]);
    let vault = w.vault.path().display().to_string();
    let git_dir = other.join(".git").display().to_string();
    let index = other.join(".git/index").display().to_string();
    let env = [("BP_VAULT", vault.as_str()), ("GIT_DIR", git_dir.as_str()), ("GIT_INDEX_FILE", index.as_str())];
    let (exit, v) = common::bp_env(&w.code, &env, &["risk", ISSUE]);
    assert_eq!((exit, pick(&v), &v["head"]), (Some(0), pick(&want), &want["head"]), "{v}");
}

/// A `gh` first on `PATH` whose `pr list --head <project>/<slug>` prints `prs-<slug>.json`, else no pull request.
fn fake_gh(t: &Path) -> String {
    let bin = t.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let fake = format!(
        "#!/bin/sh\n[ \"$1 $2\" = 'pr list' ] && {{ cat \"{}/prs-$(basename \"$4\").json\" 2>/dev/null || echo '[]'; }}\n",
        t.display()
    );
    std::fs::write(bin.join("gh"), fake).unwrap();
    std::fs::set_permissions(bin.join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

/// An open pull request of `<project>/<slug>` at `head` into master, as the fake prints it.
fn open_pr(t: &Path, slug: &str, head: &str) {
    let pr = json!([{ "number": 1, "state": "OPEN", "headRefName": format!("bp/{slug}"), "headRefOid": head, "baseRefName": "master" }]);
    std::fs::write(t.join(format!("prs-{slug}.json")), pr.to_string()).unwrap();
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
    let mut args = vec![
        "record",
        "review",
        ISSUE,
        "--field",
        &field,
        "--field",
        "round=1",
        "--field",
        "lens=conformance",
        "--field",
        "mode=full",
        "--body-file",
    ];
    args.push(file.to_str().unwrap());
    let (exit, review) = w.bp(&args);
    assert_eq!(exit, Some(0), "{review}");
    let id = review["id"].as_str().unwrap();
    let v = w.risk();
    assert_eq!(pick(&v), json!(["high", "low", "low"]), "{v}");
    assert_eq!(v["raised_by"], json!([id]));
    assert_eq!(v["reasons"], json!([format!("review {id} holds a risk row")]));
    // An edit by hand of the committed record cannot drop the raise: the level reads the review as first committed
    // (37, design change 2; review of #95, finding 7).
    let path = w.vault.path().join(format!("{id}.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("| risk |", "| warn |")).unwrap();
    git(w.vault.path(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "lower it by hand"]);
    let v = w.risk();
    assert_eq!((v["level"].as_str(), &v["raised_by"]), (Some("high"), &json!([id])), "{v}");
    // Git's location variables cannot turn the read off: the call clears them (review of #98, finding 6).
    let vault = w.vault.path().display().to_string();
    let (_, v) = common::bp_env(&w.code, &[("BP_VAULT", &vault), ("GIT_DIR", "/nonexistent")], &["risk", ISSUE]);
    assert_eq!(v["level"], "high", "{v}");
    // Nor can a variable that hides history or moves the objects: the call clears every `GIT_*` (row 25).
    let tip = w.t.path().join("tip");
    std::fs::write(&tip, git(w.vault.path(), &["rev-parse", "HEAD"])).unwrap();
    let tip = tip.display().to_string();
    for hide in ["GIT_SHALLOW_FILE", "GIT_GRAFT_FILE"]
        .map(|n| (n, tip.as_str()))
        .into_iter()
        .chain([("GIT_OBJECT_DIRECTORY", "/nonexistent"), ("GIT_NAMESPACE", "x")])
    {
        let (_, v) = common::bp_env(&w.code, &[("BP_VAULT", &vault), hide], &["risk", ISSUE]);
        assert_eq!(v["level"], "high", "{hide:?}: {v}");
    }
    // A first-commit `git log` that fails refuses; it never reads the present text (row 26).
    let shim = w.t.path().join("shim");
    std::fs::create_dir(&shim).unwrap();
    let path = std::env::var("PATH").unwrap();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in *' --reverse --no-renames '*) echo 'no log here' >&2; exit 128;; esac\nPATH='{path}' \
         exec git \"$@\"\n"
    );
    std::fs::write(shim.join("git"), script).unwrap();
    std::fs::set_permissions(shim.join("git"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let shimmed = format!("{}:{path}", shim.display());
    let (exit, v) = common::bp_env(&w.code, &[("BP_VAULT", &vault), ("PATH", &shimmed)], &["risk", ISSUE]);
    assert_eq!((exit, v["code"].as_str()), (Some(1), Some("could_not_run")), "{v}");
    assert!(v["subject"].as_str().unwrap().starts_with("git log"), "{v}");
    // A rename moves the record in a commit that adds it at the new path; the first add at the old path counts, so
    // the rename does not launder the edit (finding 7). The rename takes the claim's branch, so read with no clone.
    let root = w.vault.path();
    let (exit, v) = common::bp(root, &["rename", ISSUE, "a2"]);
    assert_eq!(exit, Some(0), "{v}");
    let (renamed, moved) = ("projects/bp/issues/a2", id.replace("/issues/a/", "/issues/a2/"));
    let (_, v) = common::bp(root, &["risk", renamed]);
    assert_eq!((v["level"].as_str(), &v["raised_by"]), (Some("high"), &json!([moved])), "{v}");
    // A delete and a re-add of the edited text: the first add still counts (finding 11 j).
    let path = root.join(format!("{moved}.md"));
    let edited = std::fs::read_to_string(&path).unwrap();
    git(root, &["rm", "-q", &format!("{moved}.md")]);
    git(root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "drop it"]);
    write(root, &format!("{moved}.md"), &edited);
    git(root, &["add", "-A"]);
    git(root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "add it back"]);
    let (_, v) = common::bp(root, &["risk", renamed]);
    assert_eq!(v["level"], "high", "{v}");
    // A first commit git cannot read refuses; it never falls back to the present text (finding 6).
    let first = git(root, &["log", "--diff-filter=A", "--format=%H", "--", &format!("{id}.md")]);
    let blob = git(root, &["rev-parse", &format!("{}:{id}.md", first.trim())]).trim().to_string();
    std::fs::remove_file(root.join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..]))).unwrap();
    let (exit, v) = common::bp(root, &["risk", renamed]);
    assert_eq!((exit, v["code"].as_str()), (Some(1), Some("could_not_run")), "{v}");
}

#[test]
fn a_medium_issue_with_an_error_at_its_cap_reads_high() {
    for (row, late, want) in [
        ("| error | src/a.rs:1 | Still wrong. |", json!([]), "high"),
        ("| error | src/c.rs:1 | Wrong before the fix. |", json!([1]), "medium"),
    ] {
        let mut w = world(None, Some("low: [docs/]\n"));
        w.cut();
        let one = w.commit(&[("src/a.rs", "fn a() {}\n"), ("src/c.rs", "fn c() {}\n")]);
        w.review(&one, 1, "conformance", &["| error | src/a.rs:1 | Wrong. |"]);
        w.review(&one, 1, "failure-paths", &[]);
        assert_eq!(w.risk()["level"], "medium", "round one raises nothing");
        let two = w.commit(&[("src/a.rs", "fn a() { 1; }\n")]);
        // The fix diff shows src/a.rs, so an error there is the fix's; src/c.rs is outside it, so its error is late.
        assert_eq!(w.review(&two, 2, "verify", &[row])["late"], late);
        let v = w.risk();
        assert_eq!(v["level"], want, "{v}");
        let cap = "round cap: the review of round 2 holds an error row at or past the cap of medium";
        assert_eq!(v["reasons"].as_array().unwrap().iter().any(|r| r == cap), want == "high", "{v}");
        // A hand edit that turns the error into a warn and drops `late` changes nothing: the first commit counts.
        let id = w.bp(&["list", "review", "--under", ISSUE]).1["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|r| r["id"].as_str().filter(|i| i.contains(&two)).map(str::to_string))
            .unwrap();
        let path = w.vault.path().join(format!("{id}.md"));
        let text = std::fs::read_to_string(&path).unwrap().replace("| error |", "| warn |").replace("late: [1]\n", "");
        std::fs::write(&path, text).unwrap();
        git(w.vault.path(), &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qam", "soften it by hand"]);
        assert_eq!(w.risk()["level"], want, "the edit is ignored");
    }
}

#[test]
fn a_risk_file_that_does_not_parse_refuses() {
    let mut w = world(None, Some("hihg: [infra/]\n"));
    w.cut();
    w.commit(&[("src/a.rs", "fn a() {}\n")]);
    let (exit, v) = w.bp(&["risk", ISSUE]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("risk_file_invalid")), "{v}");
    assert_eq!(v["subject"], "origin/master:.bp/risk.yml");
    assert!(v["fix"].as_str().unwrap().contains("\"hihg\" is no key"), "{v}");
}

#[test]
fn an_exact_pattern_that_names_a_directory_refuses() {
    for (rules, dir) in [("high: [src, src/b.rs]\n", "src"), ("low: [docs/, src/b.rs, src]\n", "src")] {
        let mut w = world(None, Some(rules));
        w.cut();
        w.commit(&[("src/a.rs", "fn a() {}\n")]);
        let (exit, v) = w.bp(&["risk", ISSUE]);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("risk_file_invalid")), "{v}");
        let fix = v["fix"].as_str().unwrap();
        assert!(
            fix.starts_with(&format!("the pattern {dir:?} names a directory, so it matches no path: write {dir}/"))
        );
    }
    // The check folds ASCII case, and reads each head's tree too: a directory the branch adds is one.
    for (rules, dir) in [("high: [SRC]\n", "SRC"), ("high: [newdir]\n", "newdir")] {
        let mut w = world(None, Some(rules));
        w.cut();
        w.commit(&[("newdir/a.rs", "fn a() {}\n")]);
        let (exit, v) = w.bp(&["risk", ISSUE]);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("risk_file_invalid")), "{v}");
        assert!(v["fix"].as_str().unwrap().starts_with(&format!("the pattern {dir:?} names a directory")), "{v}");
    }
    let mut w = world(None, Some("high: [src/b.rs, nothing/here]\n"));
    w.cut();
    w.commit(&[("src/b.rs", "fn b2() {}\n")]);
    assert_eq!(w.risk()["reasons"], json!(["src/b.rs matches the high pattern src/b.rs"]), "a file is a path");
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
