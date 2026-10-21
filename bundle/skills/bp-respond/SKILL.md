---
name: bp-respond
description: Use when a worker dispatch names an issue that is `needs-response`. Answer each finding, and push the fixes.
---

Task: answer each finding of the latest review on the issue on the `Node:` line of the dispatch. This text calls the issue `<id>`.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. Escalate when `fix` needs a verb on the `Forbidden:` line. Also escalate when the call fails again after `fix`.

1. Read the slice: `bp walk <id> --for respond --json`. Its first body is the latest review, with its `id`.
2. Find the worktree. Run `bp list claim --under <id> --json`, then `bp show <claim> --json` for the last row. Edit and commit only in its `worktree`.
3. Read the findings in the table of the review body. Each row under the header is one finding, numbered from 1.
4. Fix each finding in the worktree and commit, or decline it. A decline needs a reason from the issue, a design or a convention. A finding can need work that the issue does not hold. Decline it, and name that work in the reason.
5. Run the blocks before you push: `bp check <id> --json`.
6. Record one disposition for each finding. `<review>` is the review `id`, and `<n>` is the row number of the finding. `<sha>` is the output of `git rev-parse HEAD` in the worktree.
   - Fixed: `bp record disposition <review> --field finding=<n> --field as=fixed --field head=<sha> --json`.
   - Declined: `bp record disposition <review> --field finding=<n> --field as=declined --field reason="<why>" --json`.
7. Push the fixes: `bp pr <id> --json`.

To escalate, run `bp release <id> --reason escalated --json`. Then stop, and reply with `escalated` and the cause.

Stop: `bp walk <id> --for respond --done --json` returns `done` true.
