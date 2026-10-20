---
name: bp-complete
description: Use when a worker dispatch names an issue to complete. Claim it, build it, and open its pull request.
---

Task: complete the issue on the `Node:` line of the dispatch. This text calls it `<id>`.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. Escalate when `fix` needs a verb on the `Forbidden:` line. Also escalate when the call fails again after `fix`.

1. Read the slice: `bp walk <id> --for complete --json`. It holds the issue, its milestone, its target designs and the learnings. Read a node from `ids` with `bp show <node> --json` only when a step needs it.
2. Claim the issue: `bp cut <id> --json`. Edit and commit only in the `worktree` that it returns.
3. Read the named blocks in the `## Check` section of the issue. They define done.
4. Make the change. Obey the conventions that the slice names. Commit in the worktree.
5. Run the blocks before you push: `bp check <id> --json`.
6. If the issue asks for a change that makes a target design false, propose a delta. Write the whole new design body to a file outside the vault. Then run `bp record delta <design> --field milestone=<milestone> --field cause=<id> --field change="<what changes>" --body-file <file> --json`. The milestone is the `under` of the issue.
7. Open the pull request: `bp pr <id> --json`.

Also escalate when the issue contradicts a target design and does not ask for that change. To escalate, run `bp release <id> --reason escalated --json`. Then stop, and reply with `escalated` and the cause.

Stop: `bp walk <id> --for complete --done --json` returns `done` true.
