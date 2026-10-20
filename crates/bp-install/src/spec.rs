//! The surface spec of each target (17, Interfaces 1): where each part of the bundle goes, and the limits the
//! validator holds it to. A change here is a delta on 17. The hook scripts read the harness's hook input with `jq`:
//! `session_id`, and `tool_input.file_path` for a pre-tool hook.

use crate::Target;

/// One hook: the harness event, the tool matcher, and the script under `bundle/hooks/`.
pub(crate) struct Hook {
    pub(crate) event: &'static str,
    pub(crate) matcher: Option<&'static str>,
    pub(crate) script: &'static str,
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
    /// The always-on file, which holds Blueprint's section between the markers.
    pub(crate) always_on: &'static str,
    /// The file whose `hooks` key holds the hook entries, beside the user's own.
    pub(crate) settings: &'static str,
    /// How a hook command names the clone's top.
    pub(crate) top: &'static str,
    pub(crate) events: &'static [Hook],
    pub(crate) description_max: usize,
    pub(crate) body_lines: usize,
    /// The listing budget in description characters: Claude Code's 1% of the context window is 8,000 and Codex's 2%
    /// is 16,000 (`analysis/bundle-install-spike.md`).
    pub(crate) listing: usize,
}

const START: Hook = Hook { event: "SessionStart", matcher: None, script: "bp-start.sh" };

pub(crate) const CLAUDE: Spec = Spec {
    target: Target::Claude,
    skills: ".claude/skills",
    agents: ".claude/agents",
    dispatch: ".claude/bp/dispatch",
    hooks: ".claude/hooks",
    always_on: "CLAUDE.md",
    settings: ".claude/settings.local.json",
    top: "\"$CLAUDE_PROJECT_DIR\"",
    events: &[
        START,
        Hook { event: "PreToolUse", matcher: Some("Edit|Write"), script: "bp-guard.sh" },
        Hook { event: "SessionEnd", matcher: None, script: "bp-end.sh" },
    ],
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
    settings: ".codex/hooks.json",
    // The main worktree's top, where `install` writes the hooks: a linked worktree holds none of them, as they are
    // excluded, not tracked.
    top: "\"$(git rev-parse --path-format=absolute --git-common-dir)/..\"",
    events: &[START],
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

    /// The manifest entries of the two edits inside shared files: `CLAUDE.md#bp`, `settings.local.json#hooks`.
    pub(crate) fn section_entry(&self) -> String {
        format!("{}#bp", file_name(self.always_on))
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
