#!/usr/bin/env bash
# SessionEnd: records the session's handoff under its project node. `record` takes the seed from the session's latest
# vault commit, the one whose `Bp-Session` trailer names this session (12; 13, Handoff).
command -v jq > /dev/null || { echo "bp-end: jq is not installed, so no handoff is recorded; install jq" >&2; exit 0; }
session=$(jq -r '.session_id // empty')
[ -n "$session" ] || exit 0
project=$(bp where --json 2> /dev/null | jq -r '.project // empty')
[ -n "$project" ] || exit 0
BP_ROLE=session BP_SESSION=$session bp record handoff "projects/$project/project" --field "session=$session" --json > /dev/null
