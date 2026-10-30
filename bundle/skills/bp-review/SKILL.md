---
name: bp-review
description: Use when a reviewer dispatch names an issue whose pull request needs review. Try to show that its head fails the issue, and return the review.
---

Task: review the pull request of the issue on the `Node:` line of the dispatch. This text calls it `<id>`.

You write nothing in the vault or in a worktree: no commit and no record. You probe in a copy. Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. Stop and report the envelope when `fix` needs a verb on the `Forbidden:` line. Also stop and report it when the call fails again after `fix`.

Your aim is to show that the head fails its issue. Do not trust the worker's account of what it tested.

1. Read the slice: `bp walk <id> --for review --json`. Its intent item for `<id>` must read `needs-review`. Else stop and report the intent.
2. Find the worktree. Run `bp list claim --under <id> --json`, then `bp show <claim> --json` for the last row. It gives `worktree`, and `base` is the commit the branch starts from.
3. Read the head from the slice: the `head` of its `pull_request`. If `pull_request` is null, stop and report that no pull request is in view. Do not read the head of the worktree, because the worker can commit after the push.
4. List the earlier reviews: `bp list review --under <id> --json`. Read each with `bp show <review> --json`. List its dispositions with `bp list disposition --under <review> --json`, and read each `as` with `bp show <disposition> --json`. Put on the attack list each finding not `declined`, and each `What holds:` line. Rerun them at this head.
5. Before you read the diff, write the attack list. An attack is one line: the command you will run, and the result that shows a defect. Write one or more from each source:
   - Goal: what the issue asks for, and its `## Check` blocks. Which input that the blocks lack breaks the head?
   - Design: each design in the `cites` of `bp show <id> --json`. Which of its checks could the change break?
   - Convention: each convention id in the slice. Which rule under its headings could the change break?
   One attack varies what the blocks hold fixed: the environment, the paths, or real project data for written input.
6. Copy the head: `git clone -q --no-checkout <worktree> <dir>`, then `git -C <dir> checkout -q <head>`. `<dir>` is a new temporary directory. Run every probe in the copy with the project's commands, such as `cargo test`. Set each probe's timeout to 10 minutes, the shell tool's cap. Its default of 2 minutes is too short for `cargo test`.
7. Read the diff: `git -C <worktree> diff <base> <head>`. Find each change that the issue does not ask for. Find each change that no named block in the `## Check` section of the issue tests. Add an attack for each.
8. A change that makes a cited design false needs a delta. List the deltas with `bp list delta --under <design> --json`. Read each `cause` with `bp show <delta> --json`. Look for a delta whose `cause` is `<id>`.
9. Read only the convention sections that the diff touches: `bp show <convention> --section <heading> --json`. Add an attack for each rule that the diff could break.
10. Run each attack in the copy. An attack succeeds when its result shows a defect.
11. Write one row for each successful attack, with its reproduction. The reproduction is the failing command in backticks, as you ran it. Or it is the rule's id and the diff line. A command that you did not run is not evidence. Write no row for a suspicion. A probe result stands even when the issue asks for that behaviour. Then the issue is wrong: cite its id. Write no row for a taste that no cited design or convention states. Write none for a style the project's formatter sets, nor for a case the diff already handles. A defect with no line in the diff cites a node id.

Write the review in this shape:

- Line 1: `head: <sha>`.
- Then the table: `| severity | location | finding |`, its separator line, and one row for each finding. The review body holds exactly one table, the findings table. A table with no row means no finding. No other line starts with `|`, because the tool counts the table rows as findings.
- `severity` is `error` only when the reproduction shows the head failing the issue's goal or a cited design's rule. A style or wording point is `warn`.
- `location` is `<path>:<line>` in the diff, or a node id.
- The finding says what is wrong and what would pass, then gives the reproduction.
- After the table, the line `What holds:` and one line for each failed attack: what you ran, in backticks.
- For an attack that timed out or needs a verb the role denies, start the line with `not run:`. Name the attack and the cause. A `not run:` line is never a pass.
- Last line: the call that the session runs to record the review. Do not run it: `bp record review <id> --field head=<sha> --field role=reviewer --body-file <file> --json`.

A review whose rows are all `warn` is approvable; say so in the last line before the record call.

Stop: return the review. The session records it, and then `bp walk <id> --for review --done --json` returns `done` true.
