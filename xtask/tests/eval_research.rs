//! `cargo xtask eval bp-research` through the binary with a fake agent (31, design changes 3 and 4): no model runs.
//! The agent checks the sandbox it starts in, writes a session directory as Claude Code does, with a lead whose
//! report the grader reads, and prints the result JSON that names the session. Needs a built `bp` next to the xtask
//! binary; the test says so and passes without one.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{git, root, write};

/// The agent: the seed is in the vault, the clone hides the research sets and is clean, and the session holds a lead,
/// its child and a decoy of another session's call. The lead's calls and report are the fixture's file under
/// `$FAKE_LEADS`, with the clone's path for `@CLONE@`.
const AGENT: &str = r#"#!/bin/sh
set -e
cp "$BP_EVAL_PROMPT" "$FAKE_OUT/prompt.txt"
bp show "$(cat "$FAKE_LEADS/$BP_EVAL_FIXTURE.node")" --json > /dev/null
test ! -e xtask/evals/bp-research-plants && test -f xtask/evals/bp-review/pr-1.md
git status --porcelain > "$FAKE_OUT/status.txt"
git -C "$BP_VAULT" log --format='%an <%ae>|%s%n%b' > "$FAKE_OUT/vault.txt"
S=sess-$BP_EVAL_ARM-$BP_EVAL_FIXTURE
D=$CLAUDE_CONFIG_DIR/projects/$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')/$S/subagents
mkdir -p "$D"
printf '{"agentType":"bp-researcher","spawnDepth":1,"toolUseId":"toolu_s"}' > "$D/agent-lead.meta.json"
printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"tool_use","id":"toolu_c","name":"Agent","input":{"subagent_type":"bp-researcher"}}]}}\n' > "$D/agent-lead.jsonl"
sed "s#@CLONE@#$(pwd -P)#g" "$FAKE_LEADS/$BP_EVAL_FIXTURE.jsonl" >> "$D/agent-lead.jsonl"
printf '{"agentType":"bp-researcher","spawnDepth":2,"toolUseId":"toolu_c"}' > "$D/agent-child.meta.json"
echo '{}' > "$D/agent-child.jsonl"
printf '{"agentType":"bp-researcher","spawnDepth":2,"toolUseId":"toolu_other"}' > "$D/agent-decoy.meta.json"
echo '{}' > "$D/agent-decoy.jsonl"
echo "a warning on stderr" >&2
if [ -n "$FAKE_KILLED" ]; then
  # A killed run: no result line, and a stderr line that names a session elsewhere.
  E=$CLAUDE_CONFIG_DIR/projects/-elsewhere/stray/subagents; mkdir -p "$E"
  printf '{"agentType":"bp-researcher","spawnDepth":1}' > "$E/agent-stray.meta.json"; echo '{}' > "$E/agent-stray.jsonl"
  echo '{"type":"system","subtype":"init","session_id":"stray"}' >&2
  exit 0
fi
printf '{"type":"result","session_id":"%s","total_cost_usd":0}\n' "$S"
"#;

const NODE: &str = "shared/learnings/the-scale-module-weighs-a-hop-at-nine";
const TRUE_NODE: &str = "shared/learnings/the-scale-module-weighs-a-hop-at-three";

/// A lead's transcript: its Bash calls, each also under `cd <clone> &&` as the harness writes it, then its report.
fn transcript(commands: &[&str], report: &str) -> String {
    let mut lines = Vec::new();
    for (i, c) in commands.iter().enumerate() {
        for (n, command) in [(*c).to_string(), format!("cd @CLONE@ && {c}")].into_iter().enumerate() {
            let call = serde_json::json!({"type": "assistant", "cwd": "@CLONE@", "message": {"model": "fake-model",
                "content": [{"type": "tool_use", "id": format!("toolu_{i}_{n}"), "name": "Bash", "input": {"command": command}}]}});
            lines.push(call.to_string());
        }
    }
    let last = serde_json::json!({"type": "assistant", "cwd": "@CLONE@", "message": {"model": "fake-model",
        "content": [{"type": "text", "text": report}]}});
    lines.push(last.to_string());
    lines.join("\n") + "\n"
}

/// A scratch repository, a fixture set of one plant and its control, the leads' transcripts, a vault and the agent.
struct Fake {
    dir: tempfile::TempDir,
    repo: String,
    fixtures: String,
    question: String,
}

impl Fake {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path();
        let repo = t.join("repo");
        git(t, &["init", "-q", "-b", "master", "repo"]);
        write(&repo.join("README.md"), "# bp\n");
        write(&repo.join("xtask/evals/bp-research-plants/plant-1.md"), "Measured on the repository's master branch.\n");
        write(&repo.join("xtask/evals/bp-review/pr-1.md"), "a review fixture stays\n");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        let shape = "Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.";
        let question = format!("What weight does the scale module give a hop? {shape}");
        let seed = |title: &str, n: &str| {
            format!(
                "---\nkind: learning\ntitle: \"{title}\"\nstatus: verified\ncites: [projects/bp/designs/system]\n---\n\
                 The scale module weighs a hop at {n}.\n"
            )
        };
        let common = format!("base: {base}\nquestion: {question}\n");
        let settle = "settled_by: grep -n HOP README.md\nmatch: 3\n";
        write(
            &t.join("bp-research-plants/plant-1.md"),
            &format!(
                "kind: plant\nseed: 3\n{common}plant: {NODE}\ngenre: Number\nclass: design\n{settle}unless: 9\nnouns: scale hop\n{}",
                seed("The weight is nine", "9")
            ),
        );
        write(
            &t.join("bp-research-plants/control-1.md"),
            &format!(
                "kind: control\n{common}plant: {TRUE_NODE}\n{settle}nouns: scale hop\n{}",
                seed("The weight is three", "3")
            ),
        );
        // The plant's lead kills it and cites a URL it never fetched; the control's lead falsifies the true seed.
        let table = "## Answer\n\nThree.\n\n## Findings\n\nat: abc1234\n| claim | verdict | source | mark |\n|---|---|---|---|\n";
        let plant = format!(
            "{table}| The scale module weighs a hop at 3 | holds | `@CLONE@/README.md:1` | measured |\n\
             | The learning's weight of 9 is stale | falsified | `{NODE}` | documented |\n\
             | The weight is documented upstream | holds | https://example.com/weights | documented |\n"
        );
        let control = format!(
            "{table}| The scale module weighs a hop at 3 | holds | `README.md:1` | measured |\n\
             | The learning's weight of 3 is wrong | falsified | `{TRUE_NODE}` | inferred |\n"
        );
        for (id, node, report) in [("plant-1", NODE, plant), ("control-1", TRUE_NODE, control)] {
            let calls = [format!("bp show {node} --json"), "grep -n HOP README.md".into()];
            write(&t.join(format!("leads/{id}.jsonl")), &transcript(&[&calls[0], &calls[1]], &report));
            write(&t.join(format!("leads/{id}.node")), node);
        }
        common::vault(&t.join("vault"));
        write(&t.join("agent.sh"), AGENT);
        std::fs::create_dir_all(t.join("out")).unwrap();
        let fixtures = t.join("bp-research-plants").display().to_string();
        Self { repo: repo.display().to_string(), fixtures, question, dir }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn store(&self) -> PathBuf {
        self.path("redesign/analysis/evals-skills/bp-research-plants")
    }

    /// `cargo xtask eval <args>` with the fake agent; `killed` makes the agent end with no result line.
    fn xtask(&self, args: &[&str], killed: &str) -> (Option<i32>, String) {
        let t = self.dir.path();
        let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("eval")
            .args(args)
            .current_dir(root())
            .env("BP_REDESIGN", t.join("redesign"))
            .env("BP_VAULT", t.join("vault"))
            .env("BP_EVAL_AGENT", format!("sh {}/agent.sh", t.display()))
            .env("CLAUDE_CONFIG_DIR", t.join("claude"))
            .env("FAKE_OUT", t.join("out"))
            .env("FAKE_LEADS", t.join("leads"))
            .env("FAKE_KILLED", killed)
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code(), text)
    }

    /// One trial of each fixture on the `with` arm, run with `bp`.
    fn run(&self, bp: &Path, trials: &str, killed: &str) -> (Option<i32>, String) {
        let with = format!("with={}", bp.display());
        let args =
            ["bp-research", "--trials", trials, "--fixtures", &self.fixtures, "--repo", &self.repo, "--bin", &with];
        self.xtask(&args, killed)
    }

    fn grade(&self, more: &[&str]) -> (Option<i32>, String) {
        self.xtask(&[&["--grade", "--check", "bp-research", "--fixtures", &self.fixtures][..], more].concat(), "")
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn a_research_trial_seeds_hides_the_sets_and_stores_the_session() {
    let Some(bp) = common::bp() else { return };
    let fake = Fake::new();
    let (code, out) = fake.run(&bp, "1", "");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("plants: 1 in 1 files\ncontrols: 1\ntwo_part: 0\nskill: bp-research"), "{out}");

    let prompt = read(&fake.path("out/prompt.txt"));
    let dispatch = prompt.split_once("\n\n").unwrap().1;
    let lines: Vec<&str> = dispatch.lines().collect();
    assert_eq!((lines[0], lines[5], lines[6]), ("Node: projects/bp/project", "", fake.question.as_str()), "{prompt}");
    assert_eq!(read(&fake.path("out/status.txt")), "", "the hidden sets leave git status clean");
    let vault = read(&fake.path("out/vault.txt"));
    assert!(vault.starts_with(&format!("bp scratch <scratch@bp.invalid>|create learning {NODE}\n")), "{vault}");
    assert!(!["seed", "plant", "fixture", "bp-eval"].iter().any(|w| vault.to_lowercase().contains(w)), "{vault}");

    let store = fake.store();
    let lead = read(&store.join("with-plant-1-1.jsonl"));
    assert!(lead.contains("toolu_c"), "the lead is the graded trace: {lead}");
    let listed = read(&store.join("with-plant-1-1/run.txt"));
    assert_eq!(
        listed,
        "session: sess-with-plant-1\nchild: agent-child.jsonl bp-researcher depth 2\n\
         other: agent-decoy.jsonl bp-researcher depth 2\nlead: agent-lead.jsonl bp-researcher depth 1\n"
    );
    for name in ["lead", "child", "decoy"] {
        assert!(store.join(format!("with-plant-1-1/agent-{name}.meta.json")).is_file(), "{name}");
    }
    let hash = read(&store.join("with-plant-1-1.hash"));
    assert!(!hash.contains("skill: none") && hash.contains(" arms-3\n"), "the bundle holds the body: {hash}");
    let (code, out) = fake.grade(&[]);
    assert!(code == Some(0) && out.contains("plants: 1 in 1 files"), "{out}");
    let review = root().join("xtask/evals/bp-review").display().to_string();
    let (code, out) = fake.xtask(&["--grade", "--check", "bp-research", "--fixtures", &review], "");
    assert!(code == Some(1) && out.contains("no research fixture"), "a wrong set fails: {out}");

    // A run with no result line keeps its own clone's session, not the one a stderr line names.
    let (code, out) = fake.run(&bp, "2", "1");
    assert_eq!(code, Some(0), "{out}");
    let kept = store.join("with-plant-1-2");
    assert!(kept.join("agent-lead.jsonl").is_file() && !kept.join("agent-stray.jsonl").exists());
    let listed = read(&kept.join("run.txt"));
    assert!(listed.starts_with("session: sess-with-plant-1\n"), "{listed}");
    assert!(listed.contains("warning: the agent's output holds no result line"), "{listed}");
    assert!(read(&store.join("with-plant-1-2.jsonl")).contains("toolu_c"), "the trace is its own lead");
    let with = format!("with={}", bp.display());
    let (code, out) = fake.xtask(&["bp-research", "--trials", "1", "--arms", "with,without", "--bin", &with], "");
    assert_eq!(code, Some(4), "no without arm for research: {out}");
}

/// The grader's lines on a fake store (31, design change 4), the agreement file's round trip (`--labels`, then
/// `--relabel`), and the refusal of an arm that ran part of the set.
#[test]
fn a_research_store_grades_labels_and_refuses_an_uneven_arm() {
    let Some(bp) = common::bp() else { return };
    let fake = Fake::new();
    let (code, out) = fake.run(&bp, "1", "");
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = fake.grade(&[]);
    assert_eq!(code, Some(0), "{out}");
    let result = read(&fake.path("redesign/analysis/evals-skills/bp-research-plants.md"));
    let lines = "plants: 1\nkills: 1/1\nkills_Number: 1/1\nkills_design: 1/1\nrelayed: 0\ncontrol_false: 1.00\n\
                 unread_cites: 0.20\ndocumented_unread: 1\nno_report: 0\nrows: 2.50\nfan_out: 2\nout_of_row: 0\n\
                 holdout_gain: n/a\nwalkthrough: pending\nstatus: pending\n";
    assert!(result.contains(lines), "{result}");
    let report = read(&fake.store().join("report.md"));
    assert!(
        report.contains("  - row 2: KILL plant 1: The learning's weight of 9")
            && report.contains("FALSE contradiction")
    );

    // `--labels` writes the file; the labeller fills a blind cell; `--relabel` rewrites the code cells from the store
    // and keeps every other byte.
    let labels = fake.path("labels.tsv");
    let file = labels.display().to_string();
    let (code, out) = fake.xtask(&["--grade", "bp-research", "--fixtures", &fake.fixtures, "--labels", &file], "");
    assert_eq!(code, Some(0), "{out}");
    let written = read(&labels);
    assert_eq!(
        written,
        "trial\trow\tcode_kill\tblind_kill\tcode_unread\tblind_unread\n\
         with-control-1-1\t1\tno\t\tno\t\nwith-control-1-1\t2\tno\t\tno\t\n\
         with-plant-1-1\t1\tno\t\tno\t\nwith-plant-1-1\t2\tyes\t\tno\t\nwith-plant-1-1\t3\tno\t\tyes\t\n"
    );
    let labelled = format!("labeller: {}\n{written}", "a".repeat(40)).replace("1-1\t2\tyes\t\t", "1-1\t2\tyes\tyes\t");
    write(&labels, &labelled.replace("1-1\t3\tno\t\tyes\t", "1-1\t3\tno\t\tno\t"));
    let (code, out) = fake.grade(&["--relabel", &file]);
    assert_eq!(code, Some(0), "{out}");
    assert_eq!(read(&labels), labelled, "the stale code cell is rewritten; the blind cell and the header stay");
    let edited: Vec<(&str, &str)> = written.lines().zip(labelled.lines().skip(1)).filter(|(a, b)| a != b).collect();
    assert_eq!(edited, [("with-plant-1-1\t2\tyes\t\tno\t", "with-plant-1-1\t2\tyes\tyes\tno\t")], "one cell");
    write(&labels, &format!("{labelled}with-plant-1-1\t9\tno\t\tno\t\n"));
    let (code, out) = fake.grade(&["--relabel", &file]);
    assert!(code == Some(1) && out.contains("the store holds no such trial and row"), "{out}");
    // The file must hold each row of the store once: a row twice, or a row short, is an error (review of PR 81).
    write(&labels, &format!("{labelled}with-plant-1-1\t2\tyes\tyes\tno\t\n"));
    let (code, out) = fake.grade(&["--relabel", &file]);
    assert!(code == Some(1) && out.contains("`with-plant-1-1\t2`: the label file holds this row twice"), "{out}");
    write(&labels, &labelled.replace("with-plant-1-1\t3\tno\t\tyes\t\n", ""));
    let (code, out) = fake.grade(&["--relabel", &file]);
    assert!(
        code == Some(1) && out.contains("`with-plant-1-1\t3`: the label file lacks this row of the store"),
        "{out}"
    );
    let (code, out) = fake.grade(&["--labels", &file, "--relabel", &file]);
    assert!(code == Some(4) && out.contains("--labels and --relabel are two runs"), "{out}");

    // An arm that ran part of the set: the grader refuses the store.
    let store = fake.store();
    for ext in ["jsonl", "hash", "vault.log", "head"] {
        std::fs::copy(store.join(format!("with-plant-1-1.{ext}")), store.join(format!("old-plant-1-1.{ext}"))).unwrap();
    }
    let (code, out) = fake.grade(&[]);
    assert!(code == Some(2) && out.contains("the old arm ran 1 of 2 fixtures; rerun it"), "{out}");
}
