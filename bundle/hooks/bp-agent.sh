#!/usr/bin/env bash
# PreToolUse on Agent and Task (`Task` is the tool's older name): a researcher dispatches sub-researchers and no other
# type (31, design change 6). The harness ignores a type list in a subagent's `Agent(...)` tool, and the role hook would
# give a worker child the worker's write verbs, so this hook bounds the child's type. The depth key that `install`
# writes, `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`, bounds the depth. Any other caller passes: the session dispatches
# every role. A dispatch of a worker, reviewer or researcher names its `model` and its `effort`: the role files fix
# none, so a call that omits them runs on the session's model, which is the dearest (36, section 3). Exit 2 blocks the
# call. The hook fails closed, as every guard does (17, Hooks): when it cannot read its
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
# A tool input that carries a non-empty string for each key passes; anything else is a missing one.
named() { jq -e --arg k "$1" '.tool_input[$k] | type == "string" and length > 0' <<< "$input" > /dev/null 2>&1; }
case "$child" in
    bp-worker) route="read the route of its intent (ready or needs-response) from bp list or bp walk" ;;
    bp-reviewer) route="read the route of its intent (needs-review) from bp list or bp walk" ;;
    bp-researcher) route="research runs on opus at high effort, for each researcher and sub-researcher" ;;
    *) route="" ;;
esac
if [ -n "$route" ]; then
    for key in model effort; do
        named "$key" || deny "a $child dispatch must name its $key; $route"
    done
fi
exit 0
