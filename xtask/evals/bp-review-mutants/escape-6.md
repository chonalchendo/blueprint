kind: escape
base: e5b1cb71ffca335a74d047ff818d91c5c22d9005
head: 45a7f4a58b738af2a8bf6b6bcb93aae1edb691f5
title: The role hook
truth:
- crates/bp-operation/src/lib.rs:107 | The session role's verbs lack `check`, which 21's session row lists, so the role hook denies the `bp check` that `bp-design`'s revision step tells the session to run (escape 6; fixed in #50).
---
## Problem

`bp-operation` holds each role's verbs, from 21's rows; the hook and the templates read them.

## Shape

3. **Each commit names its role** (21, rules 6 and 7; 26, design change 7). On `claude`, a `PreToolUse` hook on `Bash`, `"$CLAUDE_PROJECT_DIR"/.claude/hooks/bp-role.sh`, acts on each command that calls `bp`. With an `agent_id`, it maps `agent_type` (`bp-worker`, `bp-reviewer`, `bp-researcher`) to the role; with none, the role is `session`. Another subagent runs only the read verbs (21, rule 6). It denies a verb outside the role's verbs in `bp-operation` with exit 2. Else it returns the whole `tool_input` through `updatedInput`, prefixed with `export BP_ROLE=<role> BP_SESSION=<agent_id, else session_id>;`, and decides no permission. On `codex`, each role's TOML sets `BP_ROLE` through `shell_environment_policy`, which Codex does not document for a subagent. A human shell sets neither, so its commits stay `human`. The role is a label, not a gate; the forge stays the gate. Under an agent role, `stamp`, `land` and a `transition` to `approved` or `active` refuse a new code, `role_forbidden` (exit 2), before any other test. So a milestone's approval is its latest `transition … approved` commit with `Bp-Role: human` (13). Additive: one code.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p bp-operation -p bp-tool -p bp-vault
~~~
