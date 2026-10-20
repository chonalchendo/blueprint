#!/usr/bin/env bash
# PreToolUse on Bash: names the role of each command that names bp, and denies a verb outside the role's verbs (21,
# rule 6; 27, design change 3). A dispatch carries an `agent_id`, and its `agent_type` names the role; else the role is
# the session's. The role is a label an agent can forge, not a gate: the forge stays the gate. The hook fails closed:
# when it cannot read its input or decide the role, it exits 2, which blocks the call, and names the cause.
deny() {
    echo "bp-role: $1" >&2
    exit 2
}
command -v jq > /dev/null || deny "jq is not installed, so every Bash call is denied; install jq"
input=$(< /dev/stdin)
[ -n "$input" ] || deny "the hook input is empty, so the call is denied"
command=$(jq -er '.tool_input.command | strings' <<< "$input" 2> /dev/null) ||
    deny "the hook input is not the JSON of a Bash call, so the call is denied"
# The label goes on any command that names bp as a word, as a call the parse misses would commit as `human`, the one
# label the approval measure trusts. The prefix is an `export`, so a nested `bash -c`, `eval` or loop body inherits it.
bps=$(tr -c '[:alnum:]_.-' '\n' <<< "$command" | grep -cx bp)
[ "$bps" -gt 0 ] || exit 0
# The deny alone rests on the parse. Each simple command is tested: the shell's separators split the text, then leading
# assignments, the shell's keywords and the harness's wrappers with their options go. A separator inside quotes splits
# too, so the test errs toward a deny.
verbs=()
while read -ra words; do
    i=0
    while [[ ${words[i]:-} =~ ^([A-Za-z_][A-Za-z0-9_]*=.*|-.*|[0-9.]+[smhd]?|!|builtin|command|do|elif|else|env|exec|if|nice|nohup|noglob|stdbuf|sudo|then|time|timeout|until|while|xargs)$ ]]; do
        i=$((i + 1))
    done
    w=${words[i]:-}
    w=${w//[\"\']/}
    [ "${w##*/}" = bp ] || continue
    # The verb is the first word after bp that is not a flag, as `--json` is global.
    verb=
    for w in "${words[@]:i+1}"; do
        case $w in -*) ;; *) verb=${w//[\"\']/} && break ;; esac
    done
    verbs+=("$verb")
done < <(tr ';&|(){}`' '\n' <<< "$command")
agent=$(jq -r '.agent_id // empty' <<< "$input")
role=
if [ -z "$agent" ]; then
    role=session
    session=$(jq -r '.session_id // empty' <<< "$input")
else
    type=$(jq -r '.agent_type // empty' <<< "$input")
    # Another agent type has no role: its reads pass with no prefix, as a read commits nothing.
    case $type in
    bp-worker) role=worker ;;
    bp-reviewer) role=reviewer ;;
    bp-researcher) role=researcher ;;
    esac
    session=$agent
fi
[ -n "$session" ] || deny "the hook input names no session, so the call is denied"
# The read verbs, open to every role and any subagent, and the role's write verbs; install writes both from
# bp-operation (21, rule 6).
# bp:read-verbs
case $role in
# bp:role-verbs
esac
# A subagent with no role writes nothing, so a bp call the parse cannot see is denied: its verb is unknown.
[ -n "$role" ] || [ ${#verbs[@]} -ge "$bps" ] ||
    deny "agent type '$type' is no bp role, and the hook cannot parse every bp call in the command; run bp as a plain command"
for verb in "${verbs[@]}"; do
    # A call with no verb (`bp --help`, `bp --version`) runs nothing.
    [ -n "$verb" ] || continue
    case " $reads $allowed " in
    *" $verb "*) ;;
    *)
        [ -n "$role" ] || deny "agent type '$type' is no bp role, so its bp $verb is denied; dispatch bp-worker, bp-reviewer or bp-researcher"
        deny "bp $verb is outside the $role role's verbs ($reads${allowed:+ $allowed}), so the call is denied; ask the human to run it from a shell"
        ;;
    esac
done
[ -n "$role" ] || exit 0
# The whole input goes back, as `updatedInput` replaces it; no permission is decided, so the user's rules still apply.
jq -c --arg role "$role" --arg session "$session" '{hookSpecificOutput: {hookEventName: "PreToolUse",
    updatedInput: (.tool_input + {command: "export BP_ROLE=\($role) BP_SESSION=\($session | @sh); \(.tool_input.command)"})}}' <<< "$input"
