//! The kill grade through the binary with a fake agent (30, design change 2): no model runs. A planted set of one
//! mutant and one control runs two arms. `with` writes a row in the mutant's span with a command in backticks, and a
//! control row next to the `known:` entry; `old` writes the same mutant row with no evidence, and a control row
//! elsewhere. Needs a built `bp` next to the xtask binary; the test says so and passes without one.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::{git, root, write};

/// The agent: the planted head from the walk, a preload of the shipped body's first line, and one row chosen by the
/// arm and the fixture.
const AGENT: &str = r#"#!/bin/sh
set -e
ID=$(sed -n 's/^Node: //p' "$BP_EVAL_PROMPT" | head -1)
H=$(bp walk "$ID" --for review --json | tr ',{' '\n\n' | sed -n 's/.*"head":"\([0-9a-f]*\)".*/\1/p' | head -1)
T=$BP_EVAL_TRANSCRIPT
LINE=$(awk 'f >= 2 && NF { print; exit } /^---$/ { f++ }' "$FAKE_SKILL" | sed 's/\\/\\\\/g; s/"/\\"/g')
printf '{"type":"user","isMeta":true,"message":{"content":"<skill-format>true</skill-format>\\n%s"}}\n' "$LINE" >> "$T"
case "$BP_EVAL_ARM-$BP_EVAL_FIXTURE" in
with-mutant-*) ROW='| error | src/a.rs:3 | `cargo test -p a | grep width` fails: the width is 3, not 2 |' ;;
*-mutant-*) ROW='| error | src/a.rs:3 | The width divides where it should subtract. |' ;;
with-control-*) ROW='| warn | src/a.rs:3 | The width is fixed at 3. |' ;;
*) ROW='| warn | src/a.rs:9 | A taste. |' ;;
esac
printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"text","text":"head: %s\\n| severity | location | finding |\\n|---|---|---|\\n%s"}]}}\n' "$H" "$ROW" >> "$T"
"#;

const CHECK: &str = "## Problem\n\nx\n\n## Check\n\n~~~check shell {name=sandbox expect=0}\ntest -f README.md\n~~~\n";

struct Setup {
    dir: tempfile::TempDir,
    bp: PathBuf,
}

fn setup() -> Option<Setup> {
    let bp = common::bp()?;
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path();
    let repo = t.join("repo");
    git(t, &["init", "-q", "-b", "master", "repo"]);
    write(&repo.join("README.md"), "# bp\n");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    let code =
        "fn width() -> u32 {\n    let w = 3;\n    w - 1\n}\n\nfn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";
    write(&repo.join("src/a.rs"), code);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-qm", "head"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    write(&repo.join("src/a.rs"), &code.replace("w - 1", "w / 1"));
    let diff = git(&repo, &["diff"]);
    git(&repo, &["checkout", "--", "src/a.rs"]);

    let set = t.join("bp-review-mutants");
    let shared = format!("base: {base}\nhead: {head}\ntitle: A fixture\n");
    let indented: String = diff.lines().flat_map(|l| ["    ", l, "\n"]).collect();
    write(
        &set.join("mutant-1-1.md"),
        &format!(
            "kind: mutant\n{shared}genre: BinaryOperator\nspan: src/a.rs:3-3\nseed: 7\nmutant_diff: |\n{indented}truth:\n- src/a.rs:3-3 | replace - with / in width\n---\n{CHECK}"
        ),
    );
    write(
        &set.join("control-1.md"),
        &format!("kind: control\n{shared}known: src/a.rs:2 | The width is fixed.\ntruth:\n---\n{CHECK}"),
    );
    common::vault(&t.join("vault"));
    write(&t.join("agent.sh"), AGENT);
    Some(Setup { dir, bp })
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
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code(), text)
}

fn lines(text: &str, wanted: &[&str]) {
    for line in wanted {
        assert!(text.lines().any(|l| l == *line), "{line} in\n{text}");
    }
}

#[test]
fn a_kill_needs_the_span_and_evidence_and_a_known_control_row_is_not_false() {
    let Some(s) = setup() else { return };
    let t = s.dir.path();
    let (set, repo, bp) = (t.join("bp-review-mutants"), t.join("repo"), s.bp.display().to_string());
    let (set, repo) = (set.display().to_string(), repo.display().to_string());
    let (with, old) = (format!("with={bp}"), format!("old={bp}"));
    let args = ["bp-review", "--trials", "1", "--arms", "with,old", "--fixtures", &set, "--repo", &repo];
    let (code, out) = xtask(&s, &[&args[..], &["--bin", &with, "--bin", &old]].concat());
    assert_eq!(code, Some(0), "{out}");

    let results = t.join("redesign/analysis/evals-skills");
    let (file, store) = (results.join("bp-review-dev.md"), results.join("bp-review-dev"));
    let text = std::fs::read_to_string(&file).unwrap();
    lines(
        &text,
        &[
            "arms: with, old",
            "skill_loaded: all",
            "mutants: 1",
            "kills: 1/1",
            "kills_BinaryOperator: 1/1",
            "control_false: 0.00",
            "duplicates: 0",
            "holdout_gain: 1.00 [-0.12, 1.00]",
            "status: pending",
        ],
    );
    assert!(!text.contains("uplift") && !text.contains("false_findings"), "{text}");
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    for want in [
        "| with | 1 of 1 | 0.00 | 0 |",
        "| old | 0 of 1 | 1.00 | 0 |",
        "match, truth item 1 `src/a.rs:3`: `cargo test -p a | grep width` fails",
        "FALSE `src/a.rs:3`: The width divides",
        "known, entry 1 `src/a.rs:3`",
        "FALSE `src/a.rs:9`",
    ] {
        assert!(report.contains(want), "{want} in\n{report}");
    }

    // `capability` needs the lower bound above 0: the grader names the bound and writes nothing.
    std::fs::write(&file, text.replace("status: pending", "status: capability")).unwrap();
    let grade = |extra: &[&str]| xtask(&s, &[&["--grade", "bp-review", "--fixtures", &set][..], extra].concat());
    for extra in [&[][..], &["--check"]] {
        let (code, out) = grade(extra);
        assert!(code == Some(2) && out.contains("lower bound of holdout_gain is -0.12, not above 0"), "{out}");
    }
    std::fs::write(&file, &text).unwrap();

    // An arm built from a git ref names its commit in each trial's hash file, and grades against that commit's files.
    let sha = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(root()).output().unwrap();
    let sha = String::from_utf8(sha.stdout).unwrap().trim().to_string();
    let hashed = |trial: &str| store.join(format!("{trial}.hash"));
    for trial in ["old-control-1-1", "old-mutant-1-1-1"] {
        let had = std::fs::read_to_string(hashed(trial)).unwrap();
        std::fs::write(hashed(trial), format!("{had}ref: {sha}\n")).unwrap();
    }
    assert_eq!(grade(&[]).0, Some(0));
    let text = std::fs::read_to_string(&file).unwrap();
    let hash = text.lines().find_map(|l| l.strip_prefix("hash: ")).unwrap();
    lines(&text, &[&format!("old_ref: {sha}"), &format!("old_hash: {hash}")]);
    assert_eq!(grade(&["--check"]).0, Some(0));
    let had = std::fs::read_to_string(hashed("old-mutant-1-1-1")).unwrap();
    std::fs::write(hashed("old-mutant-1-1-1"), had.replacen(hash, &"0".repeat(40), 1)).unwrap();
    let (code, out) = grade(&[]);
    assert!(code == Some(2) && out.contains("old-mutant-1-1-1: the trial's hash is not the hash of"), "{out}");
}
