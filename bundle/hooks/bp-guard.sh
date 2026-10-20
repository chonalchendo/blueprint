#!/usr/bin/env bash
# PreToolUse on Edit and Write: denies a write under the vault root, because bp is the vault's only writer (12).
# Exit 2 blocks the call. Without jq the hook cannot read its input, so it denies: a guard fails closed (17, Hooks).
command -v jq > /dev/null || { echo "bp-guard: jq is not installed, so every Edit and Write is denied; install jq" >&2; exit 2; }
path=$(jq -r '.tool_input.file_path // empty') || exit 2
[ -n "$path" ] || exit 0
vault=$(bp where --json 2> /dev/null | jq -r '.vault // empty')
[ -n "$vault" ] || exit 0
case $path in /*) ;; *) path=$PWD/$path ;; esac
# A new file has no directory yet: its nearest existing ancestor decides, through symlinks, as the vault root is.
dir=${path%/*}
while [ -n "$dir" ] && [ ! -d "$dir" ]; do dir=${dir%/*}; done
dir=$(cd "${dir:-/}" && pwd -P)
case $dir/ in
"$vault"/*)
    echo "bp-guard: $path is in the vault; change it with a bp verb (bp --help)" >&2
    exit 2
    ;;
esac
exit 0
