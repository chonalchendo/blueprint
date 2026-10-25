kind: control
base: 602b78680a60c6177d89f3d484ace4e52057b0ed
head: 236c243f99e48dfe7dcf5066052735cf9c9c034a
title: Each harness checks its own bundle
truth:
---
## Problem

The owner's probe (2026-10-22): after `bp install --target claude`, a Claude session's start walk refused `bundle_mismatch` on a stale `.codex/hooks/bp-start.sh`. The fix is design change 8. No model run.

## Shape

8. **Each harness checks its own bundle** (17, check 7). A stale bundle of another harness never blocks a Claude session's start walk, nor a `bp` call the role hook prefixes. Codex has no role hook, so a Codex agent's later walk, a role-less Claude subagent and a `bp` call hidden in a script still check both bundles. `BP_HARNESS` is a selector, not a gate: an agent that sets it can hide drift, as it can skip the walk (review of PR 56). Today `bp_install::check` checks the Claude and Codex bundles in turn, so either one refuses for both. The start hook and the role hook's prefix name the harness in `BP_HARNESS`, and the check reads only that harness's bundle. With none named (a plain shell), both are checked. The other harness's start hook still refuses its own drift.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-check -p bp-install -p bp-tool
~~~
