//! The transcripts that Claude Code wrote for one headless run (31, design change 3). The run's result JSON names its
//! session, and the session's directory holds one transcript per subagent, with a `.meta.json` beside it: the agent
//! type, the spawn depth and the id of the `Agent` call that started it. The lead is the dispatched role at depth 1. A
//! child is a transcript whose `toolUseId` is an `Agent` (or `Task`) call in the lead's transcript. The newest
//! transcript of a type on disk is a race: five pilot trials copied an agent of another session (`m8-pilot/result.md`,
//! finding 2).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

/// One subagent transcript and its meta (`Null` when the meta is missing or is no JSON).
pub struct Agent {
    pub file: PathBuf,
    meta: Value,
}

impl Agent {
    fn name(&self) -> String {
        self.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn depth(&self) -> String {
        self.meta["spawnDepth"].as_u64().map_or_else(|| "none".into(), |d| d.to_string())
    }
}

/// A session's transcripts, joined to the lead.
pub struct Session {
    pub id: String,
    /// `None` when no transcript is the lead: the store keeps the session's transcripts all the same.
    pub lead: Option<PathBuf>,
    pub children: Vec<PathBuf>,
    pub agents: Vec<Agent>,
    /// A child whose spawn depth is not 2 (the `toolUseId` join holds, and the cross-check does not), or a session
    /// found with no result line.
    pub warnings: Vec<String>,
}

/// The `session_id` of the result line of `output`: `claude -p --output-format json` prints its result as one JSON
/// line with `type: result` when it ends. The agent's log also holds its stderr, where another JSON line may name
/// another session, so no other line counts.
pub fn session_id(output: &str) -> Option<String> {
    let mut values = output.lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok());
    values.find(|v| v["type"] == "result").and_then(|v| v["session_id"].as_str().map(str::to_string))
}

/// Where Claude Code keeps the sessions started in `cwd`: `<config>/projects/<cwd>/`, with each character of `cwd`
/// that is not a letter or a digit written as `-`.
pub fn project_dir(config: &Path, cwd: &Path) -> PathBuf {
    let encoded: String =
        cwd.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    config.join("projects").join(encoded)
}

impl Session {
    /// The session of a run of the agent in `cwd`, whose stdout and stderr are `output`. The result line names it. A
    /// run with no result line (killed at the timeout, or one that died) names none: its session is then the newest
    /// directory of the clone's own project directory, which the trial alone uses, that holds `subagents/`, and one with
    /// a lead of `role` before one without. So `memory/` and another directory the harness writes beside a session are
    /// never the session (review of PR #80). A warning says so. `None` when no directory holds `subagents/`.
    pub fn find(config: &Path, cwd: &Path, output: &str, role: &str) -> Option<Self> {
        let project = project_dir(config, cwd);
        if let Some(id) = session_id(output) {
            return Some(Self::join(&project.join(id), role));
        }
        let sessions = std::fs::read_dir(&project).into_iter().flatten().flatten().map(|e| e.path());
        let joined = sessions.filter(|p| p.join("subagents").is_dir()).map(|p| (Self::join(&p, role), p));
        let modified = |p: &Path| p.metadata().and_then(|m| m.modified()).ok();
        let (mut s, _) = joined.max_by_key(|(s, p)| (s.lead.is_some(), modified(p)))?;
        s.warnings.insert(
            0,
            format!("the agent's output holds no result line; this is the newest session of {}", project.display()),
        );
        Some(s)
    }

    /// The transcripts of the session directory `dir`, joined: the lead is the newest transcript of `role` at depth 1.
    pub fn join(dir: &Path, role: &str) -> Self {
        let files = std::fs::read_dir(dir.join("subagents")).into_iter().flatten().flatten().map(|e| e.path());
        let mut agents: Vec<Agent> = files
            .filter(|f| f.extension().is_some_and(|e| e == "jsonl"))
            .filter(|f| f.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("agent-")))
            .map(|file| {
                let meta = std::fs::read_to_string(file.with_extension("meta.json")).unwrap_or_default();
                Agent { meta: serde_json::from_str(&meta).unwrap_or_default(), file }
            })
            .collect();
        agents.sort_by(|a, b| a.file.cmp(&b.file));
        let lead = agents
            .iter()
            .filter(|a| a.meta["agentType"] == role && a.meta["spawnDepth"] == 1)
            .max_by_key(|a| a.file.metadata().and_then(|m| m.modified()).ok())
            .map(|a| a.file.clone());
        let calls =
            lead.as_ref().map(|l| agent_calls(&std::fs::read_to_string(l).unwrap_or_default())).unwrap_or_default();
        let (mut children, mut warnings) = (Vec::new(), Vec::new());
        for a in agents.iter().filter(|a| a.meta["toolUseId"].as_str().is_some_and(|id| calls.contains(id))) {
            if a.meta["spawnDepth"] != 2 {
                warnings.push(format!(
                    "{}: its toolUseId joins it to the lead, and its spawnDepth is {}, not 2",
                    a.name(),
                    a.depth()
                ));
            }
            children.push(a.file.clone());
        }
        let id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { id, lead, children, agents, warnings }
    }

    /// Copies every transcript of the session and its meta into `dir`, and writes `dir/run.txt`: the session, then the
    /// lead, each child and each other transcript by file name, type and depth, then each warning. A retry of a failed
    /// trial replaces what the earlier attempt kept.
    pub fn store(&self, dir: &Path) -> Result<()> {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        std::fs::create_dir_all(dir)?;
        let mut run = vec![format!("session: {}", self.id)];
        for a in &self.agents {
            let name = a.name();
            std::fs::copy(&a.file, dir.join(&name)).with_context(|| format!("copy {}", a.file.display()))?;
            let meta = a.file.with_extension("meta.json");
            if meta.is_file() {
                std::fs::copy(&meta, dir.join(name.replace(".jsonl", ".meta.json")))?;
            }
            let role = if self.lead.as_ref() == Some(&a.file) {
                "lead"
            } else if self.children.contains(&a.file) {
                "child"
            } else {
                "other"
            };
            let kind = a.meta["agentType"].as_str().unwrap_or("none");
            run.push(format!("{role}: {name} {kind} depth {}", a.depth()));
        }
        run.extend(self.warnings.iter().map(|w| format!("warning: {w}")));
        std::fs::write(dir.join("run.txt"), run.join("\n") + "\n")?;
        Ok(())
    }
}

/// The ids of the `Agent` and `Task` calls in a transcript: `Task` is the tool's older name.
fn agent_calls(transcript: &str) -> BTreeSet<String> {
    let values = transcript.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok());
    let blocks: Vec<Value> = values
        .filter(|v| v["type"] == "assistant")
        .flat_map(|v| v["message"]["content"].as_array().cloned().unwrap_or_default())
        .collect();
    blocks
        .iter()
        .filter(|b| b["type"] == "tool_use" && (b["name"] == "Agent" || b["name"] == "Task"))
        .filter_map(|b| b["id"].as_str().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_a_child_to_the_lead_by_tool_use_id() {
        let cfg = tempfile::tempdir().unwrap();
        let clone = Path::new("/private/var/folders/1r/n_ttp_c1/T/tmp.RdXDapR7wz/clone");
        let dir = project_dir(cfg.path(), clone).join("2e5fc50e-50da");
        assert!(dir.ends_with("projects/-private-var-folders-1r-n-ttp-c1-T-tmp-RdXDapR7wz-clone/2e5fc50e-50da"));
        let sub = dir.join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        let agent = |name: &str, meta: &str, text: &str| {
            std::fs::write(sub.join(format!("{name}.jsonl")), text).unwrap();
            std::fs::write(sub.join(format!("{name}.meta.json")), meta).unwrap();
            sub.join(format!("{name}.jsonl"))
        };
        let call = |tool: &str, id: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{tool}","input":{{"subagent_type":"bp-researcher"}}}}]}}}}"#
            )
        };
        let lead = agent(
            "agent-a1",
            r#"{"agentType":"bp-researcher","spawnDepth":1,"toolUseId":"toolu_session"}"#,
            &format!("{}\n{}\n", call("Agent", "toolu_one"), call("Task", "toolu_two")),
        );
        let child = agent("agent-b2", r#"{"agentType":"bp-researcher","spawnDepth":2,"toolUseId":"toolu_one"}"#, "{}");
        let deep = agent("agent-c3", r#"{"agentType":"bp-researcher","spawnDepth":3,"toolUseId":"toolu_two"}"#, "{}");
        agent("agent-d4", r#"{"agentType":"general-purpose","spawnDepth":1,"toolUseId":"toolu_x"}"#, "{}");
        agent("agent-e5", r#"{"agentType":"bp-researcher","spawnDepth":2,"toolUseId":"toolu_nobody"}"#, "{}");
        std::fs::write(sub.join("notes.jsonl"), "{}").unwrap();

        let s = Session::join(&dir, "bp-researcher");
        assert_eq!((s.id.as_str(), s.lead.as_ref()), ("2e5fc50e-50da", Some(&lead)));
        assert_eq!(s.children, [child, deep], "the Task call joins too; the decoys do not");
        assert_eq!(s.warnings, ["agent-c3.jsonl: its toolUseId joins it to the lead, and its spawnDepth is 3, not 2"]);
        assert!(Session::join(&dir, "bp-reviewer").lead.is_none(), "no lead of that role");
        assert!(Session::join(&cfg.path().join("none"), "bp-researcher").agents.is_empty());

        let out = tempfile::tempdir().unwrap();
        s.store(out.path()).unwrap();
        let run = std::fs::read_to_string(out.path().join("run.txt")).unwrap();
        assert_eq!(
            run,
            "session: 2e5fc50e-50da\nlead: agent-a1.jsonl bp-researcher depth 1\nchild: agent-b2.jsonl bp-researcher depth 2\n\
             child: agent-c3.jsonl bp-researcher depth 3\nother: agent-d4.jsonl general-purpose depth 1\n\
             other: agent-e5.jsonl bp-researcher depth 2\nwarning: agent-c3.jsonl: its toolUseId joins it to the lead, \
             and its spawnDepth is 3, not 2\n"
        );
        for name in ["a1", "b2", "c3", "d4", "e5"] {
            assert!(out.path().join(format!("agent-{name}.jsonl")).is_file(), "{name}");
            assert!(out.path().join(format!("agent-{name}.meta.json")).is_file(), "{name}");
        }

        let log = "warning: a line\n{\"type\":\"result\",\"session_id\":\"2e5fc50e-50da\",\"result\":\"x\"}\nbye\n";
        assert_eq!(session_id(log).as_deref(), Some("2e5fc50e-50da"));
        assert_eq!(session_id("no JSON here"), None);
        assert_eq!(session_id("{\"session_id\":\"other\"}\n"), None, "only the result line names the session");
    }

    /// A run killed before its result line, whose stderr names another session, keeps its own clone's session.
    #[test]
    fn a_run_with_no_result_line_keeps_its_own_clones_session() {
        let cfg = tempfile::tempdir().unwrap();
        let (clone, elsewhere) = (Path::new("/tmp/bp-forge/Ab12/code"), Path::new("/tmp/elsewhere"));
        for (cwd, id, kind) in [(clone, "own", "bp-researcher"), (elsewhere, "other", "bp-researcher")] {
            let sub = project_dir(cfg.path(), cwd).join(id).join("subagents");
            std::fs::create_dir_all(&sub).unwrap();
            std::fs::write(sub.join(format!("agent-{id}.jsonl")), "{}").unwrap();
            std::fs::write(
                sub.join(format!("agent-{id}.meta.json")),
                format!(r#"{{"agentType":"{kind}","spawnDepth":1}}"#),
            )
            .unwrap();
        }
        let log = "{\"type\":\"system\",\"session_id\":\"other\"}\nKilled\n";
        let s = Session::find(cfg.path(), clone, log, "bp-researcher").unwrap();
        assert_eq!(s.id, "own");
        assert!(s.lead.as_ref().is_some_and(|l| l.ends_with("agent-own.jsonl")), "{:?}", s.lead);
        assert!(s.warnings[0].starts_with("the agent's output holds no result line"), "{:?}", s.warnings);
        let out = tempfile::tempdir().unwrap();
        s.store(out.path()).unwrap();
        assert!(out.path().join("agent-own.jsonl").is_file() && !out.path().join("agent-other.jsonl").exists());
        let run = std::fs::read_to_string(out.path().join("run.txt")).unwrap();
        assert!(
            run.contains("lead: agent-own.jsonl") && run.contains("warning: the agent's output holds no result"),
            "{run}"
        );
        assert!(Session::find(cfg.path(), Path::new("/tmp/none"), log, "bp-researcher").is_none(), "no session at all");
    }

    /// The harness writes `memory/` beside a session, and a session without the role's lead may be newer: the fallback
    /// takes neither (review of PR #80, fold review).
    #[test]
    fn the_fallback_skips_a_memory_directory_and_prefers_a_lead() {
        let cfg = tempfile::tempdir().unwrap();
        let clone = Path::new("/tmp/bp-forge/Cd34/code");
        let project = project_dir(cfg.path(), clone);
        let session = |id: &str, kind: &str| {
            let sub = project.join(id).join("subagents");
            std::fs::create_dir_all(&sub).unwrap();
            std::fs::write(sub.join(format!("agent-{id}.jsonl")), "{}").unwrap();
            let meta = format!(r#"{{"agentType":"{kind}","spawnDepth":1}}"#);
            std::fs::write(sub.join(format!("agent-{id}.meta.json")), meta).unwrap();
        };
        let later = || std::thread::sleep(std::time::Duration::from_millis(20));
        session("own", "bp-researcher");
        later();
        session("helper", "general-purpose");
        later();
        std::fs::create_dir_all(project.join("memory")).unwrap();
        std::fs::write(project.join("memory/MEMORY.md"), "notes").unwrap();
        let s = Session::find(cfg.path(), clone, "Killed\n", "bp-researcher").unwrap();
        assert_eq!(s.id, "own", "memory/ is newer and holds no subagents/; helper is newer and holds no lead");
        assert!(s.lead.is_some_and(|l| l.ends_with("agent-own.jsonl")));
        let s = Session::find(cfg.path(), clone, "Killed\n", "bp-reviewer").unwrap();
        assert_eq!(s.id, "helper", "with no lead of the role anywhere, the newest session");
        std::fs::remove_dir_all(project.join("own")).unwrap();
        std::fs::remove_dir_all(project.join("helper")).unwrap();
        assert!(Session::find(cfg.path(), clone, "Killed\n", "bp-researcher").is_none(), "memory/ alone is no session");
    }

    /// The review, complete and respond trials join the same way: their lead is the role at depth 1.
    #[test]
    fn a_review_lead_is_the_reviewer_at_depth_one() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        for (name, meta) in [
            ("agent-1", r#"{"agentType":"bp-reviewer","spawnDepth":1}"#),
            ("agent-2", r#"{"agentType":"Explore","spawnDepth":1}"#),
        ] {
            std::fs::write(sub.join(format!("{name}.jsonl")), "{}").unwrap();
            std::fs::write(sub.join(format!("{name}.meta.json")), meta).unwrap();
        }
        let s = Session::join(dir.path(), "bp-reviewer");
        assert_eq!((s.lead, s.children.len()), (Some(sub.join("agent-1.jsonl")), 0));
        assert!(Session::join(dir.path(), "bp-worker").lead.is_none());
    }
}
