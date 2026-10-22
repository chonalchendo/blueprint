#!/usr/bin/env bash
# SessionStart: walks the latest seed of a handoff with its kind's first action, else the project for shape (17, Hooks).
# The walk's map goes to the session's context as `additionalContext`: stdout that is one JSON object is read as the
# hook's output, not as context. A refusal is context too. The line that names `jq` is plain text, which is context.
command -v jq > /dev/null || { echo "bp-start: jq is not installed, so the last seed is not walked; install jq"; exit 0; }
# One script serves both harnesses, so its install directory names the harness. The walk then checks only that
# harness's bundle: the other's drift stops its own start hook, not this session (17, check 7).
case $(cd "$(dirname "$0")/.." && pwd) in
*/.claude) export BP_HARNESS=claude ;;
*/.codex) export BP_HARNESS=codex ;;
esac
here=$(bp where --json 2> /dev/null) || exit 0
vault=$(jq -r '.vault' <<< "$here")
project=$(jq -r '.project // empty' <<< "$here")
[ -n "$project" ] || exit 0
# A session that committed nothing records a handoff with no seed, so the newest handoff that holds one decides. The
# search stops at 20, as each look is one `bp show`.
seed=
while read -r handoff && [ -z "$seed" ]; do
    seed=$(bp show "${handoff%.md}" --no-body --json | jq -r '.seed // empty')
done < <(git -C "$vault" log --diff-filter=A --format= --name-only -- "projects/$project/handoffs" | grep '\.md$' | head -n 20)
action=shape
if [ -n "$seed" ]; then
    case $(bp show "$seed" --no-body --json | jq -r '.kind') in
    issue) action=complete ;;
    milestone) action=derive ;;
    design | project) action=shape ;;
    *) action=distil ;;
    esac
else
    seed=projects/$project/project
fi
# Claude Code moves a long context to a file and shows the agent a 2 KB preview (27, the probe: a project walk is 31 KB),
# so the context is the map, not the slice: the action, the seed and its intents first, then the top ids. The agent
# walks for the bodies. A refusal passes through whole.
walk=$(bp walk "$seed" --for "$action" --ids --json |
    jq -c --arg next "bp walk $seed --for $action --json" \
        'if .code then . else {"for": .for, seed, next: $next, intents: .intents[:5], ids: [.ids[:15][] | {id, kind}]} end')
jq -cn --arg walk "$walk" '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: $walk}}'
exit 0
