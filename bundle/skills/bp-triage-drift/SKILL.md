---
name: bp-triage-drift
description: Use when a design check failed and no delta or thread cites the run. Propose a delta, or open a thread.
---

Intent: a drift run failed, and no delta or thread cites it. Find which side is wrong, and record it. This text calls the design `<design>`.

1. Read the slice: `bp walk <design> --for triage-drift --json`. It holds the design. It does not hold the runs.
2. List the runs: `bp list run --under <design> --json`. Read each one: `bp show <run> --json`.
3. A drift run has the `verdict` `fail` or `timed-out`, and a `check_kind` that is not `integrity`. Ignore the other runs.
4. For each drift run, read its block in the design. The run's `check` field names the block. Read the code at the run's `commit`.
5. Decide the cause. The code can be correct and the design out of date. Then propose a delta (step 6). The code can be wrong, or the cause can be unclear. Then open a thread (step 7).
6. Propose a delta. Find the milestone: `bp list milestone --project <project> --status planned --json`. Use `--status approved` if none is planned. With no milestone, open a thread instead. Write the whole new design body to a file outside the vault. Then run `bp record delta <design> --field milestone=<milestone> --field cause=<run> --field change="<what changes>" --body-file <file> --json`.
7. Open a thread. Get the body from `bp create thread --template --json`, and replace each `TODO(bp):` line. Write the question, the run and what you read. Put `[[<run>]]` under `## Read`. That link is the citation of the run. Then run `bp create thread --title "<question>" --project <project> --body-file <file> --json`. Link the design: `bp link <thread> cites <design> --json`.

Stop: `bp walk <design> --for triage-drift --done --json` returns `done` true.
