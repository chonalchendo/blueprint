#!/usr/bin/env bash
# SessionStart: walks the latest handoff's seed with its kind's first action, else the project for shape (17, Hooks).
# The walk's JSON goes to stdout, which the harness adds to the session's context. A refusal is context too.
command -v jq > /dev/null || { echo "bp-start: jq is not installed, so the last seed is not walked; install jq"; exit 0; }
here=$(bp where --json 2> /dev/null) || exit 0
vault=$(jq -r '.vault' <<< "$here")
project=$(jq -r '.project // empty' <<< "$here")
[ -n "$project" ] || exit 0
handoff=$(git -C "$vault" log -1 --diff-filter=A --format= --name-only -- "projects/$project/handoffs" | grep -m1 '\.md$')
seed=
[ -z "$handoff" ] || seed=$(bp show "${handoff%.md}" --no-body --json | jq -r '.seed // empty')
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
bp walk "$seed" --for "$action" --json
exit 0
