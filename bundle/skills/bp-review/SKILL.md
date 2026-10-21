---
name: bp-review
description: Use when a reviewer dispatch names an issue whose pull request needs review. Read the diff, and return the review.
---

Task: review the pull request of the issue on the `Node:` line of the dispatch. This text calls it `<id>`.

You write nothing: no file, no commit and no record. Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. Stop and report the envelope when `fix` needs a verb on the `Forbidden:` line. Also stop and report it when the call fails again after `fix`.

1. Read the slice: `bp walk <id> --for review --json`. It holds the issue and the designs near it. Its intent item for `<id>` must read `needs-review`. Else stop and report the intent.
2. Find the worktree. Run `bp list claim --under <id> --json`, then `bp show <claim> --json` for the last row. It gives `worktree`, and `base` is the commit the branch starts from.
3. Read the head with `git -C <worktree> rev-parse HEAD`. Read the diff with `git -C <worktree> diff <base> HEAD`.
4. Compare the diff with the issue. Find each change that the issue does not ask for. Find each change that no named block in the `## Check` section of the issue tests.
5. Find the target designs: the `cites` of `bp show <id> --json`. Compare the diff with each target design. A change that makes a design false needs a delta. List the deltas with `bp list delta --under <design> --json`. Read each `cause` with `bp show <delta> --json`. Look for a delta whose `cause` is `<id>`.
6. The slice gives conventions as ids. Read only the sections that the diff touches: `bp show <convention> --section <heading> --json`.
7. Write one finding for each defect. A finding cites the issue, a design, a convention or a failing command. Write no finding for a taste that no convention states.

Write the review in this shape:

- Line 1: `head: <sha>`.
- Then the table: `| severity | location | finding |`, its separator line, and one row for each finding. The review body holds exactly one table, the findings table. A table with no row means no finding. No other line starts with `|`, because the tool counts the table rows as findings.
- `severity` is `error` when the finding must be fixed before the land, else `warn`.
- `location` is `<path>:<line>` in the diff, or a node id.
- The finding is one sentence: what is wrong, and what would pass.
- Last line: the call that the session runs to record the review. Do not run it: `bp record review <id> --field head=<sha> --field role=reviewer --body-file <file> --json`.

Stop: return the review. The session records it, and then `bp walk <id> --for review --done --json` returns `done` true.
