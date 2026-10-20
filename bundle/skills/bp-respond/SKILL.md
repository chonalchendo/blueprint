---
name: bp-respond
description: Use when a worker dispatch names an issue whose latest review has an unanswered finding. Answer each finding, and push the fixes.
---

Task: answer each finding of the latest review on the issue on the `Node:` line of the dispatch. This text calls the issue `<id>`.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix`. If `fix` needs a verb on the `Forbidden:` line, stop and report the envelope.

1. Read the slice: `bp walk <id> --for respond --json`. Its first body is the latest review, with its `id`.
2. Find the worktree. Run `bp list claim --under <id> --json`, then `bp show <claim> --json` for the last row. Do all work in its `worktree`.
3. Read the findings in the table of the review body. Each row under the header is one finding, numbered from 1.
4. Fix each finding in the worktree and commit, or decline it. A decline needs a reason from the issue, a design or a convention. A finding can need work that the issue does not hold. Decline it, and name that work in the reason.
5. See a block fail before you push: `bp check <id> --json` runs the blocks in the worktree.
6. Record one disposition for each finding. `<review>` is the review `id`, and `<n>` is the row number of the finding. `<sha>` is the output of `git rev-parse HEAD` in the worktree.
   - Fixed: `bp record disposition <review> --field finding=<n> --field as=fixed --field head=<sha> --json`.
   - Declined: `bp record disposition <review> --field finding=<n> --field as=declined --field reason="<why>" --json`.
7. Push the fixes: `bp pr <id> --json`.

A block can still fail after three attempts to fix it. Then release the claim: `bp release <id> --reason escalated --json`. Report the cause.

Stop: `bp walk <id> --for respond --done --json` returns `done` true.
