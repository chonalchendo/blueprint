kind: control
base: 1c7b3e8e494c67208d7c8e8ff225ddc917690666
head: ecc5da57d83bda4535608f24f6eb0cc1589a7b26
title: Confine the worker; refuse a verb outside the role's row
truth:
---
## Problem

Needs the verb table of issue 5, because the widened check reads it.

## Shape

5. **Confinement** (17 Hooks, check 11).
   - `bp-guard.sh` denies an Edit or Write by `bp-worker` inside the clone's main checkout. It confines nothing else: the claim's worktree and the system temp directory (where `bp-complete` step 6 writes its delta body) pass, and so do a sibling worktree, another repository and `$HOME`. It makes no claim call, because a claim row holds no worktree. Paths compare with `-ef` up the ancestors, so case and links cannot slip past (review of PR 58). Issue 3 bounds the hook at 200 ms (choice; 42 ms measured).
   - `bp-role.sh` denies the `run_in_background` flag for `bp-worker`. It does not catch `cmd &`, `nohup` or `sleep` polling.
   - `bp-end.sh` gets a `timeout`.
   - The tool refuses, with a new closed code `verb_outside_role`, every verb outside the row of `BP_ROLE`. A code never changes meaning [`refusal.rs`], so `role_forbidden` keeps its text. The check reads the role's verbs, the union of its rows in design change 7's table, because `BP_ROLE` names a role, not an action. A `transition` to `approved` or `active` still refuses `role_forbidden` first. The label is forgeable, so this stops a slip, not an attack [`set.rs:136`].
   - Scope: the guard covers the Edit and Write tools on Claude Code. Bash redirects, `git` and `sed -i` are not covered. On Codex the tool check works only if `BP_ROLE` reaches the subagent, which Codex does not document [27 design change 3]. That is unverified.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-install -p bp-tool -p xtask
~~~
