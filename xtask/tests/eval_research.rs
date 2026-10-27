//! `cargo xtask eval bp-research` through the binary with a fake agent (31, design change 3): no model runs. The agent
//! checks the sandbox it starts in, writes a session directory as Claude Code does, and prints the result JSON that
//! names the session. Needs a built `bp` next to the xtask binary; the test says so and passes without one.

mod common;

use std::path::Path;
use std::process::Command;

use common::{git, root, write};

/// The agent: the seed is in the vault, the clone hides the research sets and is clean, and the session holds a lead,
/// its child and a decoy of another session's call.
const AGENT: &str = r#"#!/bin/sh
set -e
cp "$BP_EVAL_PROMPT" "$FAKE_OUT/prompt.txt"
bp show "$FAKE_NODE" --json > /dev/null
test ! -e xtask/evals/bp-research-plants && test -f xtask/evals/bp-review/pr-1.md
git status --porcelain > "$FAKE_OUT/status.txt"
git -C "$BP_VAULT" log --format='%an <%ae>|%s%n%b' > "$FAKE_OUT/vault.txt"
S=sess-$BP_EVAL_ARM-$BP_EVAL_FIXTURE
D=$CLAUDE_CONFIG_DIR/projects/$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')/$S/subagents
mkdir -p "$D"
printf '{"agentType":"bp-researcher","spawnDepth":1,"toolUseId":"toolu_s"}' > "$D/agent-lead.meta.json"
printf '{"type":"assistant","message":{"model":"fake-model","content":[{"type":"tool_use","id":"toolu_c","name":"Agent","input":{"subagent_type":"bp-researcher"}}]}}\n' > "$D/agent-lead.jsonl"
printf '{"agentType":"bp-researcher","spawnDepth":2,"toolUseId":"toolu_c"}' > "$D/agent-child.meta.json"
echo '{}' > "$D/agent-child.jsonl"
printf '{"agentType":"bp-researcher","spawnDepth":2,"toolUseId":"toolu_other"}' > "$D/agent-decoy.meta.json"
echo '{}' > "$D/agent-decoy.jsonl"
echo "a warning on stderr" >&2
printf '{"type":"result","session_id":"%s","total_cost_usd":0}\n' "$S"
"#;

const NODE: &str = "shared/learnings/the-scale-module-weighs-a-hop-at-nine";

#[test]
fn a_research_trial_seeds_hides_the_sets_and_stores_the_session() {
    let Some(bp) = common::bp() else { return };
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
    write(
        &t.join("bp-research-plants/plant-1.md"),
        &format!(
            "kind: plant\nseed: 3\nbase: {base}\nquestion: {question}\nplant: {NODE}\ngenre: Number\nclass: design\n\
             settled_by: grep -n HOP README.md\nmatch: 3\nunless: 9\nnouns: scale hop\n---\nkind: learning\n\
             title: \"The weight is nine\"\nstatus: verified\ncites: [projects/bp/designs/system]\n---\n\
             The scale module weighs a hop at 9.\n"
        ),
    );
    common::vault(&t.join("vault"));
    write(&t.join("agent.sh"), AGENT);
    std::fs::create_dir_all(t.join("out")).unwrap();
    let xtask = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("eval")
            .args(args)
            .current_dir(root())
            .env("BP_REDESIGN", t.join("redesign"))
            .env("BP_VAULT", t.join("vault"))
            .env("BP_EVAL_AGENT", format!("sh {}/agent.sh", t.display()))
            .env("CLAUDE_CONFIG_DIR", t.join("claude"))
            .env("FAKE_OUT", t.join("out"))
            .env("FAKE_NODE", NODE)
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code(), text)
    };
    let fixtures = t.join("bp-research-plants").display().to_string();
    let with = format!("with={}", bp.display());
    let run = ["bp-research", "--trials", "1", "--fixtures", &fixtures, "--repo", &repo.display().to_string()];
    let (code, out) = xtask(&[&run[..], &["--bin", &with]].concat());
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("plants: 1 in 1 files\ncontrols: 0\ntwo_part: 0\nskill: none"), "{out}");

    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    let prompt = read(&t.join("out/prompt.txt"));
    let dispatch = prompt.split_once("\n\n").unwrap().1;
    let lines: Vec<&str> = dispatch.lines().collect();
    assert_eq!((lines[0], lines[5], lines[6]), ("Node: projects/bp/project", "", question.as_str()), "{prompt}");
    assert_eq!(read(&t.join("out/status.txt")), "", "the hidden sets leave git status clean");
    let vault = read(&t.join("out/vault.txt"));
    assert!(vault.starts_with(&format!("bp scratch <scratch@bp.invalid>|create learning {NODE}\n")), "{vault}");
    assert!(!["seed", "plant", "fixture", "bp-eval"].iter().any(|w| vault.to_lowercase().contains(w)), "{vault}");

    let store = t.join("redesign/analysis/evals-skills/bp-research-plants");
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
    assert!(hash.ends_with("skill: none\n") && hash.contains(" arms-3\n"), "{hash}");

    let (code, out) = xtask(&["--grade", "--check", "bp-research", "--fixtures", &fixtures]);
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("plants: 1 in 1 files"), "{out}");
    let (code, out) = xtask(&[&run[..], &["--arms", "with,without", "--bin", &with]].concat());
    assert_eq!(code, Some(4), "no without arm for research: {out}");
}
