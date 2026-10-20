---
name: bp-complete
description: Use when a worker dispatch names an issue to complete. Claim it, build it, and open its pull request.
---

Task: complete the issue on the `Node:` line of the dispatch. This text calls it `<id>`.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix`. If `fix` needs a verb on the `Forbidden:` line, stop and report the envelope.

1. Read the slice: `bp walk <id> --for complete --json`. It holds the issue, its milestone, its target designs and the learnings. Read a node from `ids` with `bp show <node> --json` only when a step needs it.
2. Claim the issue: `bp cut <id> --json`. Do all work in the `worktree` that it returns.
3. Read the named blocks in the `## Check` section of the issue. They define done.
4. Make the change. Obey the conventions that the slice names. Commit in the worktree.
5. Run the blocks: `bp check <id> --json`. Do steps 4 and 5 again until it exits 0.
6. Does the change make a target design false? Then propose a delta. Write the whole new design body to a file outside the vault. Then run `bp record delta <design> --field milestone=<milestone> --field cause=<id> --field change="<what changes>" --body-file <file> --json`. The milestone is the `under` of the issue.
7. Open the pull request: `bp pr <id> --json`.

A block can fail after three rounds of steps 4 and 5. The issue can also contradict a target design. Then release the claim: `bp release <id> --reason escalated --json`. Report the cause.

Stop: `bp walk <id> --for complete --done --json` returns `done` true.
