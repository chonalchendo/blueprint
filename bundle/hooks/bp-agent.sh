#!/usr/bin/env bash
# PreToolUse on Agent and Task (`Task` is the tool's older name): a researcher dispatches sub-researchers and no other
# type (31, design change 6). The harness ignores a type list in a subagent's `Agent(...)` tool, and the role hook would
# give a worker child the worker's write verbs, so this hook bounds the child's type. The depth key that `install`
# writes, `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`, bounds the depth. Any other caller passes: the session dispatches
# every role. Exit 2 blocks the call. The hook fails closed, as every guard does (17, Hooks): when it cannot read its
# input, it denies, and the message names the cause.
deny() {
    echo "bp-agent: $*" >&2
    exit 2
}
command -v jq > /dev/null || deny "jq is not installed, so every Agent call is denied; install jq"
input=$(< /dev/stdin)
[ -n "$input" ] || deny "the hook input is empty, so the call is denied"
# The harness sends one JSON object. Spaces, `null`, another value, or two documents are no such input: `--slurp` reads
# every document, so each of them fails the test.
jq -se 'length == 1 and (.[0] | type == "object")' <<< "$input" > /dev/null 2>&1 ||
    deny "the hook input is not one JSON object, so the call is denied"
type=$(jq -r '.agent_type // empty' <<< "$input")
child=$(jq -r '.tool_input.subagent_type // empty' <<< "$input")
if [ "$type" = bp-researcher ] && [ "$child" != bp-researcher ]; then
    deny "a researcher may dispatch bp-researcher only, not ${child:-an agent with no type}"
fi
exit 0
