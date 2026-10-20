---
name: bp-complete
description: Use when a dispatch names an issue to complete. Claim it, build it, and open its pull request.
---

Task: complete the issue that the dispatch names.

1. Read the slice: `bp walk <id> --for complete --json`.
2. Claim the issue: `bp cut <id> --json`. Work in the worktree that it returns.
3. Build the change and commit it in the worktree.
4. Open the pull request: `bp pr <id> --json`.

Stop: the pull request is open.
