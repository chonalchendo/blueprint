kind: escape
base: e5b1cb71ffca335a74d047ff818d91c5c22d9005
head: 45a7f4a58b738af2a8bf6b6bcb93aae1edb691f5
title: The role hook
truth:
- crates/bp-operation/src/lib.rs:107 | The session role's verbs lack `check`, which 21's session row lists, so the role hook denies the `bp check` that `bp-design`'s revision step tells the session to run (escape 6; fixed in #50).
---
## Problem

A vault commit cannot name the agent role that made it. Today every commit from a Claude Code session or dispatch carries `Bp-Role: human`, because nothing sets `BP_ROLE`. An agent can also run a human act: `BP_ROLE=worker bp stamp <m>` and `bp land <i>` do not refuse on master `9575da5`. So 25's interim approver (the latest `transition … approved` commit with `Bp-Role: human`) cannot be measured.

## Shape

| Before | After |
|---|---|
| `install --target claude` registers three hooks. | It also registers `"$CLAUDE_PROJECT_DIR"/.claude/hooks/bp-role.sh` on `PreToolUse` with matcher `Bash`. Codex gets no role hook (17). |
| A Claude hook script that cannot start lets the call run. | Each Claude hook entry has `"onFailure": "block"`. |
| A `bp` call from Claude Code commits `Bp-Role: human`. | The hook returns the whole `tool_input` through `updatedInput`, with `export BP_ROLE=<role> BP_SESSION=<agent_id, else session_id>;` before the command. It does this for every command that names `bp` as a word. It decides no permission. |
| — | Install writes the allow rule `Bash(export BP_ROLE=*)` into `settings.local.json` `permissions.allow`, beside the user's rules, so the prefix does not prompt. |
| A dispatch can run any verb. | The hook denies a write verb outside the role's verbs with exit 2. The reason names the verb, the role and the next step. The read verbs (`where`, `show`, `list`, `search`, `walk`) and a call with no verb pass for every role and every subagent. |
| `bp-start.sh` prints the walk as one JSON object, which the harness reads as hook output. | It prints `{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"<walk JSON>"}}`. |
| — | New code `role_forbidden` (exit 2). Under an agent role, `stamp`, `land` and `transition` to `approved` or `active` refuse it before any other test. Additive. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p bp-operation -p bp-tool -p bp-vault
~~~
