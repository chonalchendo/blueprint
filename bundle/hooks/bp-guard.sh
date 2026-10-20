#!/usr/bin/env bash
# PreToolUse on Edit and Write: denies a write under the vault root, because bp is the vault's only writer (12).
# Exit 2 blocks the call. A guard fails closed (17, Hooks): when it cannot read its input or find the vault (no jq,
# no bp on the hook's PATH, `bp where` refuses), it denies, and the message names the cause.
deny() {
    echo "bp-guard: $1" >&2
    exit 2
}
command -v jq > /dev/null || deny "jq is not installed, so every Edit and Write is denied; install jq"
input=$(< /dev/stdin)
[ -n "$input" ] || deny "the hook input is empty, so the call is denied"
path=$(jq -r '.tool_input.file_path // empty' <<< "$input") || exit 2
[ -n "$path" ] || exit 0
command -v bp > /dev/null || deny "bp is not on the hook's PATH, so every Edit and Write is denied; put bp on the harness's PATH"
here=$(bp where --json 2> /dev/null) || deny "bp where refused ($(jq -r '"\(.code): \(.fix)"' <<< "$here" 2> /dev/null)), so every Edit and Write is denied"
vault=$(jq -r '.vault // empty' <<< "$here")
[ -n "$vault" ] || deny "bp where names no vault, so every Edit and Write is denied"
case $path in /*) ;; *) path=$PWD/$path ;; esac
# The harness may resolve `..` lexically, also through a directory that does not exist, so that form is tested too.
norm=
IFS=/ read -ra parts <<< "$path"
for p in "${parts[@]}"; do
    case $p in '' | .) ;; ..) norm=${norm%/*} ;; *) norm=$norm/$p ;; esac
done
in_vault() {
    local p=$1 dir n=0 t
    # A file that is a symlink is tested where it points, as a Write follows it.
    while [ -L "$p" ] && [ $n -lt 40 ]; do
        t=$(readlink "$p")
        case $t in /*) p=$t ;; *) p=${p%/*}/$t ;; esac
        n=$((n + 1))
    done
    # A new file has no directory yet: its nearest existing ancestor decides, through symlinks, as the vault root is.
    dir=${p%/*}
    while [ -n "$dir" ] && [ ! -d "$dir" ]; do dir=${dir%/*}; done
    dir=$(cd "${dir:-/}" && pwd -P)
    case $dir/ in "$vault"/*) return 0 ;; esac
    return 1
}
for p in "$path" "${norm:-/}"; do
    in_vault "$p" && deny "$path is in the vault; change it with a bp verb (bp --help)"
done
exit 0
