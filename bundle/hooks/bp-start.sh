#!/usr/bin/env bash
# SessionStart: walks the latest seed of a handoff with its kind's first action, else the project for shape (17, Hooks).
# The walk's JSON goes to stdout, which the harness adds to the session's context. A refusal is context too.
command -v jq > /dev/null || { echo "bp-start: jq is not installed, so the last seed is not walked; install jq"; exit 0; }
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
bp walk "$seed" --for "$action" --json
exit 0
