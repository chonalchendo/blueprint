---
name: bp-derive
description: Use when an approved milestone has a finish-line block that no issue covers. Write the next few issues under it.
---

Intent: needs derivation. An approved milestone has fewer live issues than its batch of three. A block of its finish line has no issue that covers it. You write the next few issues, not every issue of the milestone.

Pass `--json` to each `bp` call. On a non-zero exit, read `code` and run `fix`. In this text, `<m>` is the milestone id and `<p>` is its project slug.

1. Read the slice: `bp walk <m> --for derive --json`. It holds the milestone, the designs of its carried deltas, and its issues.
2. List the issues: `bp list issue --under <m> --json`. An issue with the status `open` or `in-progress` is live.
3. Find the open blocks. An issue covers a finish-line block when its `## Check` holds a block with the same `name`. No other section counts.
4. Choose the next issues. Stop at three live issues. Choose first the blocks that other work builds on.
5. Get the body template: `bp create issue --template --json`.
6. Write each body to a file outside the vault. No `TODO(bp): ` line stays.
   - The first line says what to build.
   - `## Problem` gives a reproduction: the command, what it shows now, and what it must show.
   - `## Check` holds a block with the name of each finish-line block that the issue covers.
7. A `## Check` block proves only the part that this issue builds. Give it the text of the milestone block, or a narrower text.
8. Create each issue: `bp create issue --project <p> --title "<title>" --under <m> --body-file <file> --json`.
9. Read `findings` in the result. Fix each one in the file, then run `bp set <issue> --body-file <file> --json`.
10. Link each design that the issue changes: `bp link <issue> cites <design> --json`. Link each convention that it obeys the same way.
11. An issue can need the code of another issue first. Then run `bp link <issue> depends <other> --json`. Issues with no dependency run in parallel.

Stop: `bp walk <m> --for derive --done --json` returns `done` true.
