---
name: bp-review
description: Use when a reviewer dispatch names an issue whose pull request needs review. Try to show that its head fails the issue, and return the review.
---

Task: review the pull request of the issue on the `Node:` line of the dispatch. This text calls it `<id>`.

You write nothing in the vault or in a worktree. You probe in a copy. Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. Stop and report the envelope when `fix` needs a `Forbidden:` verb, or the call fails again after `fix`.

Your aim is to show that the head fails its issue. Do not trust the worker's account of what it tested.

The dispatch names one lens on its `Lens:` line and the round on its `Round:` line. With no `Lens:` line, the lens is `conformance`. Attack through that lens only.

- `conformance`: does the head do what the issue, its cited designs and its conventions ask?
- `failure-paths`: which bad input, crash or edge case breaks the head?
- `tests-and-checks`: would the tests and the check blocks fail if the code were wrong?
- `security`: can the head leak a secret, widen access or run injected input?
- `verify`: round 2 or later. Skip steps 5 and 7 to 9. Attack only the fix: `git -C <worktree> diff <last> <head>`. `<last>` is the `head` of the latest review of the round before (step 4). Rerun each `error` row of that round, whatever its disposition. Re-report a row that still fails: name it in the `of` cell. A defect that the fix causes cites the fix line.

1. Read the slice: `bp walk <id> --for review --json`. Its intent item for `<id>` must read `needs-review`. Else stop and report the intent.
2. Find the worktree. Run `bp list claim --under <id> --json`, then `bp show <claim> --json` for the last row. It gives `worktree`, and `base`, the commit the branch starts from.
3. Read the head from the slice: the `head` of its `pull_request`. If it is null, stop and report that no pull request is in view. Not the worktree's head: the worker can commit after the push.
4. List the earlier reviews: `bp list review --under <id> --json`. Read each `head`, `round` and row with `bp show <review> --json`. List its dispositions with `bp list disposition --under <review> --json`, and read each `as` with `bp show <disposition> --json`. In round 1, put on the attack list each finding not `declined`, and each `What holds:` line. Rerun them at this head.
5. Before you read the diff, write the attack list. An attack is one line: the command you will run, and the result that shows a defect. Write one or more from each source:
   - Goal: what the issue asks for, and its `## Check` blocks. Which input that the blocks lack breaks the head?
   - Design: each design in the `cites` of `bp show <id> --json`. Which of its checks could the change break?
   - Convention: each convention id in the slice. Which rule under its headings could the change break?
   One attack varies what the blocks hold fixed: the environment, the paths, or real project data for written input.
6. Copy the head to a temporary `<dir>`: `git clone -q --no-checkout <worktree> <dir>`, then `git -C <dir> checkout -q <head>`. Run every probe there with the project's commands, with a timeout of 10 minutes.
7. Read the diff: `git -C <worktree> diff <base> <head>`. Add an attack for each change the issue does not ask for. Add one for each change no named `## Check` block tests.
8. A change that makes a cited design false needs a delta whose `cause` is `<id>`. List them with `bp list delta --under <design> --json`, and read each with `bp show <delta> --json`.
9. Read only the convention sections that the diff touches: `bp show <convention> --section <heading> --json`. Add an attack for each rule that the diff could break.
10. Run each attack in the copy. An attack succeeds when its result shows a defect.
11. Write one row for each successful attack, with its reproduction. That is the failing command in backticks, as you ran it, or the rule's id and the diff line. A command that you did not run is not evidence. Write no row for a suspicion. A probe result stands even when the issue asks for that behaviour: then cite the issue's id. Write no row for a taste that no cited design or convention states. Write none for a formatter's style, or a case the diff handles. A defect with no line in the diff cites a node id.

Write the review in this shape:

- Line 1: `head: <sha>`.
- Then the table `| severity | location | finding | of |`, its separator, and a row per finding. A table with no row means no finding. No other line starts with `|`: the tool counts the rows.
- `severity` is `error`, `warn` or `risk`. It is `error` only when the reproduction shows the head failing the issue's goal or a cited design's rule. A style or wording point is `warn`.
- Write a `risk` row when the diff touches the gate, contradicts a design, or you judge it high. It raises the issue to high.
- `location` is `<path>:<line>` in the diff, or a node id.
- `of` is empty, or `<review id>#<row>` of an earlier head's row that this `error` row re-reports.
- The finding says what is wrong and what would pass, then gives the reproduction. Escape a `|` in a cell: `\|`.
- After the table, the line `What holds:` and one line for each failed attack: what you ran, in backticks.
- An attack that timed out or needs a denied verb gets a `not run:` line with its cause. It is never a pass.
- If every row is `warn` or `risk`, say so on the line before the last: no row holds the land.
- Last line: the session's record call, which you do not run. `<mode>` is `verify` for the lens `verify`, else `full`.
Call: `bp record review <id> --field head=<sha> --field round=<n> --field lens=<lens> --field mode=<mode> --field role=reviewer --body-file <file> --json`.

Stop: return the review. After the session records each lens, `bp walk <id> --for review --done --json` returns `done` true.
