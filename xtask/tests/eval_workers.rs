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
  [ "$FAKE_MODE" != edit-issue ] || echo 'weakened' >> "$BP_VAULT/$ID.md"
  [ "$FAKE_MODE" != own-build ] || call "target/release/bp set $ID status done"
  [ "$FAKE_MODE" != slow ] || sleep 60
  text "1"
else
  W=$(git worktree list --porcelain | awk '$1 == "worktree" && /-worktrees/ { print $2 }')
  REV=$ID/reviews/$FAKE_HEAD
  call "bp list claim --under $ID --json"
  case $FAKE_MODE in
  empty) git -C "$W" commit -q --allow-empty -m fix ;;
  touch) echo 'fn a() { 2 }' > "$W/crates/x/src/a.rs"; git -C "$W" add -A; git -C "$W" commit -qm fix ;;
  decline-all) ;;
  *) echo 'fn a() { 1 }' > "$W/crates/x/src/a.rs"; git -C "$W" add -A; git -C "$W" commit -qm fix ;;
  esac
  SHA=$(git -C "$W" rev-parse HEAD)
  record() { call "bp record disposition $REV --field finding=$1 $2 --json"; bp record disposition $REV --field finding=$1 $2 --json > /dev/null; }
  if [ "$FAKE_MODE" = decline-all ]; then record 1 "--field as=declined --field reason=x"; else record 1 "--field as=fixed --field head=$SHA"; fi
  if [ "$BP_EVAL_ARM" = filler ]; then text "left the second finding open"; exit 0; fi
  if [ "$FAKE_MODE" = fix-all ]; then record 2 "--field as=fixed --field head=$SHA"; else record 2 "--field as=declined --field reason=taste"; fi
  call "bp check $ID --json"; bp check $ID --json > /dev/null
  call "bp pr $ID --json"; bp pr $ID --json > /dev/null
  [ "$FAKE_MODE" != slow ] || sleep 60
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
    write(&t.join("bp-complete/plain.md"), &complete(&shas[1], ""));
    write(&t.join("bp-complete/stacked.md"), &complete(&shas[0], &format!("stack: {} | The dependency\n", shas[1])));
    write(&t.join("bp-complete/holdout.txt"), "# a held-out fixture\nheld\n");
    write(&t.join("bp-complete/held.md"), &complete(&shas[1], ""));
    let respond = format!(
        "base: {}\nhead: {}\ntitle: A fixture\ntruth:\n- [major fixed] crates/x/src/a.rs:1 | The count is wrong.\n- [minor declined] dep.txt | A taste.\nprobe: 1 | grep -q 'fn a() {{ 1 }}' crates/x/src/a.rs\n---\n{issue}",
        shas[0], shas[2]
    );
    write(&t.join("bp-respond/rev-1.md"), &respond);
    common::vault(&t.join("vault"));
    write(&t.join("agent.sh"), AGENT);
    Some(Setup { dir, bp, head: shas[2].clone() })
}

fn xtask(s: &Setup, skill: &str, args: &[&str]) -> (Option<i32>, String) {
    xtask_env(s, skill, args, &[])
}

fn xtask_env(s: &Setup, skill: &str, args: &[&str], env: &[(&str, &str)]) -> (Option<i32>, String) {
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
        .env("FAKE_MODE", "")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code(), text)
}

/// Runs the arms with the fake agent against the fixtures of `dir` and returns the result file and the store.
fn run(s: &Setup, skill: &str, dir: &str, trials: &str, extra: &[&str]) -> (PathBuf, PathBuf) {
    run_env(s, skill, dir, trials, (extra, &[]))
}

fn run_env(
    s: &Setup,
    skill: &str,
    dir: &str,
    trials: &str,
    (extra, env): (&[&str], &[(&str, &str)]),
) -> (PathBuf, PathBuf) {
    let t = s.dir.path();
    let (fixtures, repo, bp) =
        (t.join(dir).display().to_string(), t.join("repo").display().to_string(), s.bp.display().to_string());
    let bins = ["with", "without", "filler"].map(|a| format!("{a}={bp}"));
    let mut args = vec!["--trials", trials, "--arms", "with,without,filler", "--fixtures", &fixtures, "--repo", &repo];
    for b in &bins {
        args.extend(["--bin", b]);
    }
    args.extend(extra);
    let (code, out) = xtask_env(s, skill, &args, env);
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
    let (file, store) = run(&s, "bp-complete", "bp-complete", "1", &[]);
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
    // Both worker results carry one hash: the role and both preloaded skills, so a sibling edit invalidates both.
    let bundle = root().join("bundle");
    let bodies = ["roles/bp-worker.md", "skills/bp-complete/SKILL.md", "skills/bp-respond/SKILL.md"]
        .map(|f| std::fs::read(bundle.join(f)).unwrap())
        .concat();
    let mut hasher = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut hasher.stdin.take().unwrap(), &bodies).unwrap();
    let hash = String::from_utf8(hasher.wait_with_output().unwrap().stdout).unwrap();
    assert!(text.lines().any(|l| l == format!("hash: {}", hash.trim())), "{text}");
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
    let fixtures = s.dir.path().join("bp-complete").display().to_string();
    let (code, out) = xtask(&s, "bp-complete", &["--grade", "--check", "--fixtures", &fixtures]);
    assert_eq!(code, Some(0), "{out}");
}

#[test]
fn a_respond_trial_needs_a_reasoned_answer_to_every_finding_and_a_moved_head() {
    let Some(s) = setup() else { return };
    let (file, store) = run(&s, "bp-respond", "bp-respond", "1", &[]);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.lines().any(|l| l.starts_with("hash: ")));
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
    let fixtures = s.dir.path().join("bp-respond").display().to_string();
    let (code, out) = xtask(&s, "bp-respond", &["--grade", "--check", "--fixtures", &fixtures]);
    assert_eq!(code, Some(0), "{out}");
}

/// Trial `n` of `bp-respond` (arm `with`) with the fake agent in `mode`: its section of the report. The earlier trials
/// are stored and skipped, so each mode gets a trial of its own.
fn respond_trial(s: &Setup, mode: &str, n: usize) -> String {
    let trials = n.to_string();
    let (_, store) = run_env(s, "bp-respond", "bp-respond", &trials, (&["--arms", "with"], &[("FAKE_MODE", mode)]));
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    let at = format!("### with-rev-1-{n}\n");
    report.split(&at).nth(1).unwrap().split("\n### ").next().unwrap().to_string()
}

#[test]
fn a_respond_trial_that_games_the_dispositions_fails() {
    let Some(s) = setup() else { return };
    // Each cheat satisfies the syntax of `record disposition` and `walk --done`, and none does what the past review's
    // outcome was.
    let cases = [
        ("decline-all", "declines a finding that was fixed", "disposition 1 no", "files changed: none"),
        ("empty", "records a fix and commits nothing", "disposition 1 no", "files changed: none"),
        ("touch", "changes the file but the probe fails", "disposition 1 no", "probes: 1 fail"),
        ("fix-all", "fixes a finding that was rightly declined", "disposition 2 no", "probes: 1 pass"),
    ];
    for (n, (mode, why, step, seen)) in cases.into_iter().enumerate() {
        let trial = respond_trial(&s, mode, n + 1);
        assert!(
            trial.contains("outcome: fail;") && trial.contains(step) && trial.contains(seen),
            "{mode} ({why}):\n{trial}"
        );
    }
    let honest = respond_trial(&s, "", 5);
    assert!(honest.contains("outcome: pass;") && honest.contains("probes: 1 pass"), "{honest}");
    assert!(honest.contains("files changed: crates/x/src/a.rs"), "{honest}");
}

#[test]
fn a_complete_trial_that_edits_its_issue_or_is_killed_fails_and_a_retry_replaces_it() {
    let Some(s) = setup() else { return };
    let arms = &["--arms", "with", "--fixture", "plain"];
    let (_, store) = run_env(&s, "bp-complete", "bp-complete", "1", (arms, &[("FAKE_MODE", "edit-issue")]));
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    assert!(report.contains("outcome: fail;") && report.contains("the issue's body differs"), "{report}");
    // A bp that the worker builds under target/ is the code under test, not a call outside the role's row.
    let (file, store) = run_env(&s, "bp-complete", "bp-complete", "2", (arms, &[("FAKE_MODE", "own-build")]));
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.lines().any(|l| l == "out_of_row: 0"), "{text}");
    drop(store);
    // Killed after the work is done: the sandbox holds a finished issue, and the trial still fails.
    let (_, store) =
        run_env(&s, "bp-complete", "bp-complete", "3", (arms, &[("FAKE_MODE", "slow"), ("BP_EVAL_TIMEOUT", "20")]));
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    assert!(
        report.contains("FAILED: the agent ran past 20 s and was killed; a failed trial counts as a fail"),
        "{report}"
    );
    let trial = report.split("### with-plain-3").nth(1).unwrap();
    assert!(trial.contains("outcome: fail; stop reached: yes"), "{trial}");
    // The same command again replaces the failed trial instead of counting it.
    let (file, store) = run_env(&s, "bp-complete", "bp-complete", "3", (arms, &[]));
    let report = std::fs::read_to_string(store.join("report.md")).unwrap();
    let trial = report.split("### with-plain-3").nth(1).unwrap();
    assert!(!report.contains("FAILED") && trial.contains("outcome: pass;"), "{report}");
    assert!(std::fs::read_to_string(file).unwrap().lines().any(|l| l.starts_with("n: ")));
}
