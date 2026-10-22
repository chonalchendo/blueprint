//! `cargo xtask eval` through the binary with a fake agent (29, design change 1 and 2): no model runs. The agent is a
//! shell script that checks the sandbox it starts in and writes a synthetic transcript, so the whole pipeline runs:
//! the sandbox, the prompt, the transcript store, the vault log, the grade, the result file and `--grade --check`.
//! Needs a built `bp` next to the xtask binary; the test says so and passes without one.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t.invalid"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The agent. `with` and `filler` preload a body into the transcript, as the harness does for a role's `skills:`
/// line; `without` loads none and makes one call. It fails when the sandbox is not at `needs-review`.
const AGENT: &str = r#"#!/bin/sh
set -e
ID=$(sed -n 's/^Node: //p' "$BP_EVAL_PROMPT" | head -1)
bp walk "$ID" --for review --done --json | grep -q '"done":false'
S=$(printf %.7s "$FAKE_HEAD"); B=$(printf %.7s "$FAKE_BASE")
T=$BP_EVAL_TRANSCRIPT
call() { printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"tool_use","name":"Bash","input":{"command":"%s"}}]}}\n' "$1" >> "$T"; }
case $BP_EVAL_ARM in
with) LINE=$(awk 'f >= 2 && NF { print; exit } /^---$/ { f++ }' "$FAKE_SKILL" | sed 's/\\/\\\\/g; s/"/\\"/g') ;;
filler) LINE='This line holds no instruction.' ;;
*) LINE= ;;
esac
[ -z "$LINE" ] || printf '{"type":"user","isMeta":true,"message":{"content":"<skill-format>true</skill-format>\\n%s"}}\n' "$LINE" >> "$T"
call "bp walk $ID --for review --json"
if [ "$BP_EVAL_ARM" = without ]; then
  printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"text","text":"I could not review it."}]}}\n' >> "$T"
  exit 0
fi
call "bp list claim --under $ID --json"
call "git -C /w diff $B $S"
call "bp list delta --under projects/bp/designs/system --json"
printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"text","text":"head: %s\\n| severity | location | finding |\\n|---|---|---|\\n| error | src/a.rs:1 | The count is wrong. |\\n| warn | other.rs:9 | A taste. |"}]}}\n' "$S" >> "$T"
"#;

struct Setup {
    dir: tempfile::TempDir,
    bp: PathBuf,
    head: String,
    base: String,
}

fn setup() -> Option<Setup> {
    let bp = Path::new(env!("CARGO_BIN_EXE_xtask")).with_file_name("bp");
    if !bp.is_file() {
        eprintln!("skipped: {} is not built (cargo build -p bp-tool)", bp.display());
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path();
    let repo = t.join("repo");
    git(t, &["init", "-q", "-b", "master", "repo"]);
    write(&repo.join("README.md"), "# bp\n");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    write(&repo.join("src/a.rs"), "fn a() {}\n");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "head"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);

    let fixture = format!(
        "base: {base}\nhead: {head}\ntitle: A fixture\ntruth:\n- src/a.rs:1 | The count is wrong.\n---\n## Problem\n\nx\n\n## Check\n\n~~~check shell {{name=sandbox expect=0}}\ntest -f README.md\n~~~\n"
    );
    write(&t.join("fixtures/pr-1.md"), &fixture);
    write(&t.join("fixtures/pr-2.md"), &fixture);
    write(&t.join("fixtures/holdout.txt"), "pr-2\n");

    let project = "---\nkind: project\ntitle: Bp\nstatus: active\nrepository: https://github.invalid/o/r\n---\n";
    write(&t.join("vault/projects/bp/project.md"), project);
    write(
        &t.join("vault/projects/bp/designs/system.md"),
        "---\nkind: design\ntitle: S\nstatus: active\n---\n\nBody.\n",
    );
    write(&t.join("vault/shared/.gitkeep"), "");
    write(&t.join("agent.sh"), AGENT);
    Some(Setup { dir, bp, head, base })
}

fn xtask(s: &Setup, args: &[&str]) -> (Option<i32>, String) {
    let t = s.dir.path();
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("eval")
        .args(args)
        .current_dir(root())
        .env("BP_REDESIGN", t.join("redesign"))
        .env("BP_VAULT", t.join("vault"))
        .env("BP_EVAL_AGENT", format!("sh {}/agent.sh", t.display()))
        .env("FAKE_SKILL", root().join("bundle/skills/bp-review/SKILL.md"))
        .env("FAKE_HEAD", &s.head)
        .env("FAKE_BASE", &s.base)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code(), text)
}

struct Ran {
    s: Setup,
    fixtures: String,
    results: PathBuf,
}

impl Ran {
    /// Runs `trials` trials of the with and without arms and one of filler, with the fake agent.
    fn new(trials: &str) -> Option<Self> {
        let s = setup()?;
        let t = s.dir.path();
        let (fixtures, repo, bp) = (
            t.join("fixtures").display().to_string(),
            t.join("repo").display().to_string(),
            s.bp.display().to_string(),
        );
        let bins = ["with", "without", "filler"].map(|a| format!("{a}={bp}"));
        let mut run =
            vec!["bp-review", "--trials", trials, "--arms", "with,without", "--fixtures", &fixtures, "--repo", &repo];
        for b in &bins[..2] {
            run.extend(["--bin", b]);
        }
        let (code, out) = xtask(&s, &run);
        assert_eq!(code, Some(0), "{out}");
        // One filler trial, as the walkthrough's filler run.
        let filler = [
            "bp-review",
            "--trials",
            "1",
            "--arms",
            "filler",
            "--fixtures",
            &fixtures,
            "--repo",
            &repo,
            "--bin",
            &bins[2],
        ];
        let (code, out) = xtask(&s, &filler);
        assert_eq!(code, Some(0), "{out}");
        let results = t.join("redesign/analysis/evals-skills");
        Some(Self { s, fixtures, results })
    }

    fn repo(&self) -> String {
        self.s.dir.path().join("repo").display().to_string()
    }

    fn with(&self) -> String {
        format!("with={}", self.s.bp.display())
    }

    fn file(&self) -> PathBuf {
        self.results.join("bp-review.md")
    }

    fn grade(&self, extra: &[&str]) -> (Option<i32>, String) {
        let mut args = vec!["--grade", "bp-review", "--fixtures", &self.fixtures];
        args.extend(extra);
        xtask(&self.s, &args)
    }
}

#[test]
fn a_fake_agent_runs_the_whole_pipeline_and_stores_what_the_grader_reads() {
    let Some(r) = Ran::new("2") else { return };
    let text = std::fs::read_to_string(r.file()).unwrap();
    let role = std::fs::read_to_string(root().join("bundle/roles/bp-reviewer.md")).unwrap();
    let skill = std::fs::read_to_string(root().join("bundle/skills/bp-review/SKILL.md")).unwrap();
    let hash = {
        let mut c = Command::new("git")
            .args(["hash-object", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut c.stdin.take().unwrap(), format!("{role}{skill}").as_bytes()).unwrap();
        String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap().trim().to_string()
    };
    for line in [
        "arms: with, without",
        "holdout: 1",
        "skill_loaded: all",
        &format!("hash: {hash}"),
        "model: fake-model",
        "n: 2",
        "baseline: 0.00",
        "with: 1.00",
        "uplift: 1.00",
        "adherence: 1.00",
        "out_of_row: 0",
        "false_findings: 0.50",
        "walkthrough: pending",
        "status: pending",
    ] {
        assert!(text.lines().any(|l| l == line), "{line} in\n{text}");
    }
    let ci = text.lines().find_map(|l| l.strip_prefix("uplift_ci: [")).unwrap();
    assert!(!ci.starts_with("0.00") && ci.ends_with("1.00]"), "an interval over 0 would mark a fold: {ci}");
    assert_eq!(
        text.lines().filter(|l| ["| with-", "| without-", "| filler-"].iter().any(|p| l.starts_with(p))).count(),
        5
    );

    let store = r.results.join("bp-review");
    for name in ["with-pr-1-1", "with-pr-1-2", "without-pr-1-1", "filler-pr-1-1"] {
        for ext in ["jsonl", "vault.log", "hash"] {
            assert!(store.join(format!("{name}.{ext}")).is_file(), "{name}.{ext}");
        }
    }
    assert_eq!(std::fs::read_to_string(store.join("with-pr-1-1.hash")).unwrap().trim(), hash);
    let log = std::fs::read_to_string(store.join("with-pr-1-1.vault.log")).unwrap();
    assert!(log.lines().any(|l| l.starts_with("record review ")), "{log}");
    assert!(!std::fs::read_to_string(store.join("without-pr-1-1.vault.log")).unwrap().contains("record review"));
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    assert!(
        report.contains("| with | 2 | 2 of 2 |")
            && report.contains("| without | 2 | 0 of 2 |")
            && report.contains("with-pr-1-1.jsonl"),
        "{report}"
    );
    assert!(report.contains("FALSE `other.rs:9`") && report.contains("match, truth item 1 `src/a.rs:1`"), "{report}");
    assert!(report.contains("prose recall"), "{report}");

    // `--trials N` fills to N: a rerun at 2 runs nothing, a run at 3 adds one trial per arm and fixture.
    let again = |n: &str| {
        xtask(
            &r.s,
            &[
                "bp-review",
                "--trials",
                n,
                "--arms",
                "with",
                "--fixtures",
                &r.fixtures,
                "--repo",
                &r.repo(),
                "--bin",
                &r.with(),
            ],
        )
    };
    let (code, out) = again("2");
    assert_eq!((code, out.contains("eval: trial")), (Some(0), false), "{out}");
    let (code, out) = again("3");
    assert!(code == Some(0) && out.contains("eval: trial with-pr-1-3"), "{out}");

    // A hole in the numbering and a bundle that changed since the run are refused, not graded.
    std::fs::remove_file(store.join("with-pr-1-2.jsonl")).unwrap();
    let (code, out) = r.grade(&[]);
    assert!(code == Some(2) && out.contains("with-pr-1: trial numbers [1, 3]"), "{out}");
    std::fs::write(store.join("with-pr-1-1.hash"), "0000000000000000000000000000000000000000\n").unwrap();
    let (code, out) = r.grade(&[]);
    assert!(code == Some(2) && out.contains("bundles"), "{out}");
}

#[test]
fn a_held_out_fixture_a_forged_file_and_a_swapped_trace_fail() {
    let Some(r) = Ran::new("1") else { return };
    assert_eq!(r.grade(&["--check"]).0, Some(0));
    let (code, out) = xtask(&r.s, &["bp-review", "--trials", "1", "--fixture", "pr-2", "--fixtures", &r.fixtures]);
    assert_eq!((code, out.contains("held out")), (Some(1), true), "{out}");

    let text = std::fs::read_to_string(r.file()).unwrap();
    std::fs::write(r.file(), text.replace("with: 1.00", "with: 0.90")).unwrap();
    let (code, out) = r.grade(&["--check"]);
    assert_eq!(code, Some(2), "{out}");
    assert!(out.contains("`with: 1.00`") && out.contains("`with: 0.90`"), "{out}");
    std::fs::write(r.file(), &text).unwrap();

    let trace = r.results.join("bp-review/without-pr-1-1.jsonl");
    let original = std::fs::read_to_string(&trace).unwrap();
    std::fs::write(&trace, format!("{original}{{\"type\":\"user\",\"message\":{{\"content\":\"edited\"}}}}\n"))
        .unwrap();
    assert_eq!(r.grade(&["--check"]).0, Some(2), "a swapped transcript changes its sha256");
    std::fs::write(&trace, original).unwrap();
    assert_eq!(r.grade(&["--check"]).0, Some(0));

    let hash = r.results.join("bp-review/with-pr-1-1.hash");
    std::fs::remove_file(&hash).unwrap();
    let (code, out) = r.grade(&[]);
    assert!(code == Some(2) && out.contains("no with-pr-1-1.hash"), "{out}");

    let (code, out) = xtask(&r.s, &["bp-review", "--fixtures", &r.fixtures]);
    assert_eq!(code, Some(4), "a usage error exits 4: {out}");
}

#[test]
fn a_regrade_keeps_the_owner_lines() {
    let Some(r) = Ran::new("1") else { return };
    let text = std::fs::read_to_string(r.file()).unwrap();
    std::fs::write(
        r.file(),
        text.replace("walkthrough: pending", "walkthrough: approved").replace("status: pending", "status: regression"),
    )
    .unwrap();
    assert_eq!(r.grade(&[]).0, Some(0));
    let kept = std::fs::read_to_string(r.file()).unwrap();
    assert!(kept.contains("walkthrough: approved\n") && kept.contains("status: regression\n"), "{kept}");
    assert_eq!(r.grade(&["--check"]).0, Some(0));
    std::fs::write(r.file(), kept.replace("status: regression", "status: keep")).unwrap();
    assert_eq!(r.grade(&["--check"]).0, Some(1), "a status outside the set cannot be graded");
    // One trial per arm leaves the interval over 0, so `capability` contradicts it.
    std::fs::write(r.file(), kept.replace("status: regression", "status: capability")).unwrap();
    let (code, out) = r.grade(&["--check"]);
    assert!(code == Some(2) && out.contains("does not clear 0"), "{out}");
}
