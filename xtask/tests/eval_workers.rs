//! `cargo xtask eval` for `bp-complete` and `bp-respond` through the binary with a fake agent (29, design changes 1 and
//! 2): no model runs. The agent is a shell script that does what a worker does at each step, so the sandbox, the
//! stop read from the vault and the clone, the `end:` lines, the grade and `--grade --check` all run. The `with` arm
//! finishes the issue, the `without` arm only reads the walk, and the `filler` arm cuts and escalates.
//! Needs a built `bp` next to the xtask binary; the test says so and passes without one.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::{git, root, write};

/// The agent. It acts on `$FAKE_ACTION`; the arm decides how far it goes. `with` and `filler` preload a body into the
/// transcript, as the harness does for a role's `skills:` line.
const AGENT: &str = r#"#!/bin/sh
set -e
ID=$(sed -n 's/^Node: //p' "$BP_EVAL_PROMPT" | head -1)
T=$BP_EVAL_TRANSCRIPT
export BP_ROLE=worker BP_SESSION=eval
call() { printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"tool_use","name":"Bash","input":{"command":"%s"}}]}}\n' "$1" >> "$T"; }
text() { printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"text","text":"%s"}]}}\n' "$1" >> "$T"; }
case $BP_EVAL_ARM in
with) LINE=$(awk 'f >= 2 && NF { print; exit } /^---$/ { f++ }' "$FAKE_SKILL" | sed 's/\\/\\\\/g; s/"/\\"/g') ;;
filler) LINE='This line holds no instruction.' ;;
*) LINE= ;;
esac
[ -z "$LINE" ] || printf '{"type":"user","isMeta":true,"message":{"content":"<skill-format>true</skill-format>\\n%s"}}\n' "$LINE" >> "$T"
git config user.name t; git config user.email t@t.invalid
call "bp walk $ID --for $FAKE_ACTION --json"
if [ "$BP_EVAL_ARM" = without ]; then text "I could not do it."; exit 0; fi
if [ $FAKE_ACTION = complete ]; then
  call "bp cut $ID --json"
  W=$(bp cut $ID --json | sed 's/.*"worktree":"\([^"]*\)".*/\1/')
  if [ "$BP_EVAL_ARM" = filler ]; then
    call "bp release $ID --reason escalated --json"; bp release $ID --reason escalated --json > /dev/null
    text "escalated: the issue is too large."; exit 0
  fi
  mkdir -p "$W/crates/x/src"; echo 'fn a() {}' > "$W/crates/x/src/a.rs"
  git -C "$W" add -A; git -C "$W" commit -qm a
  call "bp check $ID --json"; bp check $ID --json > /dev/null
  call "bp pr $ID --json"; bp pr $ID --json > /dev/null
  text "1"
else
  W=$(git worktree list --porcelain | awk '$1 == "worktree" && /-worktrees/ { print $2 }')
  REV=$ID/reviews/$FAKE_HEAD
  call "bp list claim --under $ID --json"
  echo 'fn a() { 1 }' > "$W/crates/x/src/a.rs"; git -C "$W" add -A; git -C "$W" commit -qm fix
  SHA=$(git -C "$W" rev-parse HEAD)
  call "bp record disposition $REV --field finding=1 --field as=fixed --field head=$SHA --json"
  bp record disposition $REV --field finding=1 --field as=fixed --field head=$SHA --json > /dev/null
  if [ "$BP_EVAL_ARM" = filler ]; then text "left the second finding open"; exit 0; fi
  call "bp record disposition $REV --field finding=2 --field as=declined --field reason=taste --json"
  bp record disposition $REV --field finding=2 --field as=declined --field reason=taste --json > /dev/null
  call "bp check $ID --json"; bp check $ID --json > /dev/null
  call "bp pr $ID --json"; bp pr $ID --json > /dev/null
  text "1"
fi
"#;

struct Setup {
    dir: tempfile::TempDir,
    bp: PathBuf,
    head: String,
}

/// A repository of three commits (the base, a dependency's file, the landed file), the fixtures of both skills and a
/// vault. The issue's block passes when the landed file exists.
fn setup() -> Option<Setup> {
    let bp = common::bp()?;
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path();
    let repo = t.join("repo");
    git(t, &["init", "-q", "-b", "master", "repo"]);
    let mut shas = Vec::new();
    for (file, text) in [("README.md", "# bp\n"), ("dep.txt", "dep\n"), ("crates/x/src/a.rs", "fn a() {}\n")] {
        write(&repo.join(file), text);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", file]);
        shas.push(git(&repo, &["rev-parse", "HEAD"]));
    }
    let issue =
        "## Problem\n\nx\n\n## Check\n\n~~~check shell {name=landed expect=0}\ntest -f crates/x/src/a.rs\n~~~\n";
    let complete = |base: &str, extra: &str| {
        format!("base: {base}\nhead: {}\ntitle: A fixture\n{extra}scope: crates/x\n---\n{issue}", shas[2])
    };
    write(&t.join("complete/plain.md"), &complete(&shas[1], ""));
    write(&t.join("complete/stacked.md"), &complete(&shas[0], &format!("stack: {} | The dependency\n", shas[1])));
    write(&t.join("complete/holdout.txt"), "# a held-out fixture\nheld\n");
    write(&t.join("complete/held.md"), &complete(&shas[1], ""));
    let respond = format!(
        "base: {}\nhead: {}\ntitle: A fixture\ntruth:\n- [major] crates/x/src/a.rs:1 | The count is wrong.\n- [minor] README.md | A taste.\n---\n{issue}",
        shas[0], shas[2]
    );
    write(&t.join("respond/rev-1.md"), &respond);
    common::vault(&t.join("vault"));
    write(&t.join("agent.sh"), AGENT);
    Some(Setup { dir, bp, head: shas[2].clone() })
}

fn xtask(s: &Setup, skill: &str, args: &[&str]) -> (Option<i32>, String) {
    let t = s.dir.path();
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("eval")
        .arg(skill)
        .args(args)
        .current_dir(root())
        .env("BP_REDESIGN", t.join("redesign"))
        .env("BP_VAULT", t.join("vault"))
        .env("BP_EVAL_AGENT", format!("sh {}/agent.sh", t.display()))
        .env("FAKE_SKILL", root().join("bundle/skills").join(skill).join("SKILL.md"))
        .env("FAKE_ACTION", skill.trim_start_matches("bp-"))
        .env("FAKE_HEAD", &s.head)
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code(), text)
}

/// Runs the arms with the fake agent against the fixtures of `dir` and returns the result file and the store.
fn run(s: &Setup, skill: &str, dir: &str, trials: &str, extra: &[&str]) -> (PathBuf, PathBuf) {
    let t = s.dir.path();
    let (fixtures, repo, bp) =
        (t.join(dir).display().to_string(), t.join("repo").display().to_string(), s.bp.display().to_string());
    let bins = ["with", "without", "filler"].map(|a| format!("{a}={bp}"));
    let mut args = vec!["--trials", trials, "--arms", "with,without,filler", "--fixtures", &fixtures, "--repo", &repo];
    for b in &bins {
        args.extend(["--bin", b]);
    }
    args.extend(extra);
    let (code, out) = xtask(s, skill, &args);
    assert_eq!(code, Some(0), "{out}");
    let results = t.join("redesign/analysis/evals-skills");
    (results.join(format!("{skill}.md")), results.join(skill))
}

fn lines(text: &str, wanted: &[&str]) {
    for line in wanted {
        assert!(text.lines().any(|l| l == *line), "{line} in\n{text}");
    }
}

#[test]
fn a_complete_trial_reads_the_stop_the_checks_and_the_file_scope_from_the_sandbox() {
    let Some(s) = setup() else { return };
    let (file, store) = run(&s, "bp-complete", "complete", "1", &[]);
    let text = std::fs::read_to_string(&file).unwrap();
    // Two fixtures run (the third is held out): the `with` arm passes both, `without` and the escalated `filler` fail.
    lines(
        &text,
        &[
            "arms: with, without",
            "holdout: 1",
            "skill_loaded: all",
            "model: fake-model",
            "n: 2",
            "baseline: 0.00",
            "with: 1.00",
            "adherence: 1.00",
            "out_of_row: 0",
            "false_findings: n/a",
            "walkthrough: pending",
            "status: pending",
        ],
    );
    let log = std::fs::read_to_string(store.join("with-stacked-1.vault.log")).unwrap();
    for line in ["end: done true", "end: checks pass", "end: file crates/x/src/a.rs"] {
        assert!(log.lines().any(|l| l == line), "{line} in\n{log}");
    }
    assert!(log.contains("cut projects/bp/issues/a-fixture"), "{log}");
    let stacked = std::fs::read_to_string(store.join("filler-stacked-1.vault.log")).unwrap();
    assert!(stacked.lines().any(|l| l.starts_with("release ") && l.ends_with(" escalated")), "{stacked}");
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    assert!(
        report.contains("| with | 2 | 2 of 2 | 2 | 0 | 2 | 2 |")
            && report.contains("| filler | 2 | 0 of 2 | 0 | 2 | 0 | 0 |")
            && report.contains("Stacked: the dependency \"The dependency\"")
            && report.contains("steps: walk yes, cut yes, check yes, pr yes"),
        "{report}"
    );
    let fixtures = s.dir.path().join("complete").display().to_string();
    let (code, out) = xtask(&s, "bp-complete", &["--grade", "--check", "--fixtures", &fixtures]);
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn a_respond_trial_needs_a_reasoned_answer_to_every_finding_and_a_moved_head() {
    let Some(s) = setup() else { return };
    let (file, store) = run(&s, "bp-respond", "respond", "1", &[]);
    let text = std::fs::read_to_string(&file).unwrap();
    lines(&text, &["skill_loaded: all", "n: 1", "baseline: 0.00", "with: 1.00", "adherence: 1.00", "out_of_row: 0"]);
    let log = std::fs::read_to_string(store.join("with-rev-1-1.vault.log")).unwrap();
    for line in ["end: done true", "end: moved yes", "end: disposition 1 fixed", "end: disposition 2 declined reasoned"]
    {
        assert!(log.lines().any(|l| l == line), "{line} in\n{log}");
    }
    let open = std::fs::read_to_string(store.join("filler-rev-1-1.vault.log")).unwrap();
    assert!(open.contains("end: done false") && !open.contains("disposition 2"), "{open}");
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    assert!(
        report.contains("steps: walk yes, check yes, pr yes, disposition 1 yes, disposition 2 yes, commit yes")
            && report.contains("dispositions: 1 fixed, 2 declined with a reason"),
        "{report}"
    );
    let fixtures = s.dir.path().join("respond").display().to_string();
    let (code, out) = xtask(&s, "bp-respond", &["--grade", "--check", "--fixtures", &fixtures]);
    assert_eq!(code, Some(0), "{out}");
}
