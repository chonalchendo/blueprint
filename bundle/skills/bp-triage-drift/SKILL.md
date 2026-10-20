---
name: bp-triage-drift
description: Use when a design check failed and no delta or thread cites the run. Propose a delta, or open a thread.
---

Intent: a design is `drifted`. A drift run failed, and no delta or thread cites it. Find which side is wrong, and record it. This text calls the seed `<seed>`: the design, or its project.

1. Read the slice: `bp walk <seed> --for triage-drift --json`. Its first bodies are the failed drift runs. The designs that they checked come next.
2. Triage each run with an empty `cited_by`. The run's `under` is its design, and its `check` field names the block. Read the block, and read the code at the run's `commit`.
3. Decide the cause. The code can be correct and the design out of date. Then propose a delta (step 4). The code can be wrong, or the cause can be unclear. Then open a thread (step 5).
4. Propose a delta. Find the milestone: `bp list milestone --project <project> --status planned --json`. Use `--status approved` if none is planned. With no milestone, open a thread instead. Write the whole new design body to a file outside the vault. Then run `bp record delta <design> --field milestone=<milestone> --field cause=<run> --field change="<what changes>" --body-file <file> --json`.
5. Open a thread. Get the body from `bp create thread --template --json`, and replace each `TODO(bp):` line. Write the question, the run and what you read. Put `[[<run>]]` under `## Read`. That link is the citation of the run. Then run `bp create thread --title "<question>" --project <project> --body-file <file> --json`. Link the design: `bp link <thread> cites <design> --json`.

Stop: `bp walk <seed> --for triage-drift --done --json` returns `done` true.
