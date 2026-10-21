---
name: bp-derive
description: Use when a milestone is `needs-derivation`. Write the next few issues under it.
---

Intent: `needs-derivation`. An approved milestone has fewer live issues than its batch of three. A block of its finish line has no issue that covers it. You write the next few issues, not every issue of the milestone.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. In this text, `<m>` is the milestone id and `<p>` is its project slug.

1. Read the slice: `bp walk <m> --for derive --json`. It holds the milestone and the designs of its carried deltas. Its issues can come as ids only.
2. List every issue under the milestone: `bp list issue --under <m> --limit 1000 --json`. If `truncated` is true, run it again with a larger `--limit`.
3. An issue with the status `open` or `in-progress` is live. Count the live issues.
4. Read the check of each issue: `bp show <issue> --section Check --json`. A `no_section` refusal means that the issue covers no block.
5. Find the open blocks. An issue covers a finish-line block when its `## Check` holds a block with the same `name`. Resolved issues count too.
6. Choose the next issues. Stop at three live issues. Choose first the blocks that other work builds on.
7. Get the body template: `bp create issue --template --json`.
8. Write each body to a file outside the vault. No `TODO(bp): ` line stays.
   - The first line says what to build.
   - `## Problem` gives a reproduction: the command, what it shows now, and what it must show.
   - `## Check` holds a block with the name of each finish-line block that the issue covers.
9. A `## Check` block proves only the part that this issue builds. Give it the text of the milestone block, or a narrower text.
10. Validate each issue first: `bp create issue --project <p> --title "<title>" --under <m> --body-file <file> --dry-run --json`.
11. Fix each item of `findings` in the file. Then run the same call without `--dry-run`.
12. Link each design that the issue changes: `bp link <issue> cites <design> --json`. Link each convention that it obeys the same way.
13. If an issue needs the code of another issue, run `bp link <issue> depends <other> --json`. Issues with no dependency run in parallel.

Stop: `bp walk <m> --for derive --done --json` returns `done` true.
