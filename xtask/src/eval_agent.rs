//! Finding the transcript that Claude Code wrote for a headless run, as the owner's replay did.

use std::path::{Path, PathBuf};

/// The newest subagent transcript of `role` that Claude Code wrote for a session started in a directory whose name
/// holds `name`: `<config>/projects/<encoded cwd>/<session>/subagents/agent-*.jsonl`. The sibling `agent-*.meta.json`
/// names the agent type, so a second subagent of the session (an Explore, a retry of another role) is not graded.
pub fn claude_transcript(config: &Path, name: &str, role: &str) -> Option<PathBuf> {
    let dirs = |p: &Path| std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    let is_role = |transcript: &Path| {
        let meta = std::fs::read_to_string(transcript.with_extension("meta.json")).unwrap_or_default();
        serde_json::from_str::<serde_json::Value>(&meta).is_ok_and(|m| m["agentType"] == role)
    };
    let mut found = Vec::new();
    for project in dirs(&config.join("projects")).into_iter().filter(|p| p.to_string_lossy().contains(name)) {
        for session in dirs(&project) {
            for file in dirs(&session.join("subagents")) {
                let named = file.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("agent-"));
                if let (true, true, Ok(m)) = (
                    named && file.extension().is_some_and(|e| e == "jsonl"),
                    is_role(&file),
                    file.metadata().and_then(|m| m.modified()),
                ) {
                    found.push((m, file));
                }
            }
        }
    }
    found.into_iter().max().map(|(_, f)| f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_newest_transcript_of_the_role_in_the_session() {
        let cfg = tempfile::tempdir().unwrap();
        let sub = cfg.path().join("projects/-tmp-bp-forge-AbCd1234-code/sess/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        let agent = |name: &str, kind: &str, ahead: u64| {
            let file = sub.join(format!("{name}.jsonl"));
            std::fs::write(&file, "{}").unwrap();
            std::fs::write(sub.join(format!("{name}.meta.json")), format!(r#"{{"agentType":"{kind}"}}"#)).unwrap();
            let when = std::time::SystemTime::now() + std::time::Duration::from_secs(ahead);
            std::fs::File::open(&file).unwrap().set_modified(when).unwrap();
            file
        };
        agent("agent-1", "bp-reviewer", 0);
        let newer = agent("agent-2", "bp-reviewer", 30);
        agent("agent-3", "Explore", 60);
        std::fs::write(sub.join("agent-4.jsonl"), "{}").unwrap();
        std::fs::create_dir_all(cfg.path().join("projects/-tmp-bp-forge-Other999-code/s/subagents")).unwrap();
        assert_eq!(claude_transcript(cfg.path(), "AbCd1234", "bp-reviewer"), Some(newer));
        assert_eq!(claude_transcript(cfg.path(), "AbCd1234", "bp-worker"), None, "no agent of that role");
        assert_eq!(claude_transcript(cfg.path(), "Nothing", "bp-reviewer"), None);
    }
}
