//! The surface spec of each target (17, Interfaces 1): where each part of the bundle goes, and the limits the
//! validator holds it to. A change here is a delta on 17. The hook scripts read the harness's hook input with `jq`:
//! `session_id`, `tool_input.file_path` or `tool_input.command` for a pre-tool hook, and `agent_id` and `agent_type`
//! inside a subagent.

use crate::Target;

/// One hook: the harness event, the tool matcher, and the script under `bundle/hooks/`.
pub(crate) struct Hook {
    pub(crate) event: &'static str,
    pub(crate) matcher: Option<&'static str>,
    pub(crate) script: &'static str,
    /// Seconds the harness waits for the script. `None` takes the harness's default.
    pub(crate) timeout: Option<u32>,
}

/// Where a target reads each part, as paths from the clone's top, and the limits it holds.
pub(crate) struct Spec {
    pub(crate) target: Target,
    /// The skill directory; the manifest sits at its root.
    pub(crate) skills: &'static str,
    /// The subagent directory. A Codex role is TOML, written from the role's markdown.
    pub(crate) agents: &'static str,
    /// The dispatch templates, each named `<role>-<template>`: a harness reads none of them.
    pub(crate) dispatch: &'static str,
    pub(crate) hooks: &'static str,
    /// The always-on file, which holds Blueprint's section between the markers. It is `AGENTS.md` for both targets,
    /// so one copy serves both harnesses (27, design change 1).
    pub(crate) always_on: &'static str,
    /// The files that make the harness skip the always-on file, in the harness's order. The first that exists gets a
    /// section of one line that imports it, unless it imports it already. Claude Code reads `AGENTS.md` only when none
    /// of its own files exists.
    pub(crate) imports: &'static [&'static str],
    /// The file whose `hooks` key holds the hook entries, beside the user's own.
    pub(crate) settings: &'static str,
    /// Claude Code's `"onFailure": "block"` on each hook entry: without it, a hook script that cannot start is a
    /// non-blocking error, and a `bp` call runs with no role, as `human`.
    pub(crate) block_on_failure: bool,
    /// The rules the hooks need in the settings' `permissions.allow`, beside the user's own. The role hook's `export`
    /// prefix would prompt under a `Bash(bp *)` rule, and a dispatch cannot answer a prompt.
    pub(crate) allow: &'static [&'static str],
    /// How a hook command names the clone's top.
    pub(crate) top: &'static str,
    pub(crate) events: &'static [Hook],
    /// Keys of the settings' `env` object that the bundle owns, with their values.
    pub(crate) env: &'static [(&'static str, &'static str)],
    pub(crate) description_max: usize,
    pub(crate) body_lines: usize,
    /// The listing budget in description characters: Claude Code's 1% of the context window is 8,000 and Codex's 2%
    /// is 16,000 (`analysis/bundle-install-spike.md`).
    pub(crate) listing: usize,
}

const START: Hook = Hook { event: "SessionStart", matcher: None, script: "bp-start.sh", timeout: None };

pub(crate) const CLAUDE: Spec = Spec {
    target: Target::Claude,
    skills: ".claude/skills",
    agents: ".claude/agents",
    dispatch: ".claude/bp/dispatch",
    hooks: ".claude/hooks",
    always_on: "AGENTS.md",
    imports: &["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"],
    settings: ".claude/settings.local.json",
    block_on_failure: true,
    allow: &["Bash(export BP_ROLE=*)"],
    top: "\"$CLAUDE_PROJECT_DIR\"",
    events: &[
        START,
        Hook { event: "PreToolUse", matcher: Some("Edit|Write"), script: "bp-guard.sh", timeout: None },
        Hook { event: "PreToolUse", matcher: Some("Bash"), script: "bp-role.sh", timeout: None },
        // A researcher dispatches sub-researchers only (31, design change 6). `Task` is the `Agent` tool's older name.
        Hook { event: "PreToolUse", matcher: Some("Agent|Task"), script: "bp-agent.sh", timeout: None },
        // Claude Code kills a SessionEnd hook after 1.5 s unless `timeout` is set, with no message; the commit of
        // `bp record` can outlast that.
        Hook { event: "SessionEnd", matcher: None, script: "bp-end.sh", timeout: Some(10) },
    ],
    // Two layers below the session: the researcher spawns a sub-researcher, and the sub-researcher spawns none. The key
    // is project-wide, so every agent loses the third layer, which no role uses (31, Decided 8).
    env: &[("CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH", "2")],
    description_max: 1024,
    body_lines: 500,
    listing: 8000,
};

/// Codex is documented, not verified (27, Goal). Its edit tool takes a patch, not a path, and it has no session-end
/// event, so only the start hook applies; the 1,024 description rule is agentskills.io's.
pub(crate) const CODEX: Spec = Spec {
    target: Target::Codex,
    skills: ".agents/skills",
    agents: ".codex/agents",
    dispatch: ".agents/bp/dispatch",
    hooks: ".codex/hooks",
    always_on: "AGENTS.md",
    imports: &[],
    settings: ".codex/hooks.json",
    block_on_failure: false,
    allow: &[],
    // The main worktree's top, where `install` writes the hooks: a linked worktree holds none of them, as they are
    // excluded, not tracked.
    top: "\"$(git rev-parse --path-format=absolute --git-common-dir)/..\"",
    events: &[START],
    // Codex gets no `Agent` tool (31, Out of scope), so no depth key.
    env: &[],
    description_max: 1024,
    body_lines: 500,
    listing: 16000,
};

impl Spec {
    pub(crate) fn of(target: Target) -> &'static Self {
        match target {
            Target::Claude => &CLAUDE,
            Target::Codex => &CODEX,
        }
    }

    /// The manifest's path.
    pub(crate) fn manifest(&self) -> String {
        format!("{}/bundle.json", self.skills)
    }

    /// The manifest entries of the edits inside shared files: `AGENTS.md#bp`, an import's `<file>#bp`, and
    /// `settings.local.json#hooks`, which holds the hook entries, the allow rules and the bundle's `env` keys.
    pub(crate) fn section_entry(&self) -> String {
        format!("{}#bp", self.always_on)
    }

    pub(crate) fn hooks_entry(&self) -> String {
        format!("{}#hooks", file_name(self.settings))
    }

    /// A hook command, so the harness runs the script from any directory of the session.
    pub(crate) fn command(&self, script: &str) -> String {
        format!("{}/{}/{script}", self.top, self.hooks)
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
