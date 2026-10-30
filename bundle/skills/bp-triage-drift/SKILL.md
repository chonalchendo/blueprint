---
name: bp-triage-drift
description: Use when a drift run failed and no delta or thread cites it. Propose a delta, or open a thread.
---

Intent: a design is `drifted`. A drift run failed, and no delta or thread cites it. Find which side is wrong, and record it. This text calls the seed `<seed>`: the design, or its project.

On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do.

1. Read the slice: `bp walk <seed> --for triage-drift --json`. Its first bodies are the drift runs that no delta or thread cites, newest first. The seed and the designs that they checked come next, when their bodies fit.
2. Triage each run. Its `header` holds `under`, `check` and `commit`. The `under` is the design, and the `check` names the block. Read the design with `bp show <design> --json` when the slice holds no body for it. Read the code at the `commit`.
3. Decide the cause. The code can be correct and the design out of date. Then propose a delta (step 4). The code can be wrong, or the cause can be unclear. Then open a thread (step 5).
4. Propose a delta. Find the milestone: `bp list milestone --project <project> --status planned --json`. Use `--status approved` if none is planned. With no milestone, open a thread instead. Write the whole new design body to a file outside the vault. If a delta of the design from another cause pends under the milestone, read the tip (`bp list delta --under <design> --json`), write your body on top of its body (`bp show <tip> --json`), and add `--field chains=<tip>`. Then run `bp record delta <design> --field milestone=<milestone> --field cause=<run> --field change="<what changes>" --body-file <file> --json`. On `delta_pending` whose fix does not name `--field chains`, cite the run from a thread (step 5), and name the pending delta as `[[<delta>]]`.
5. Open a thread. Get the body from `bp create thread --template --json`, and replace each `TODO(bp):` line. Under `## Read`, put `[[<run>]]` and `[[<design>]]`. These links cite the run and the design. One thread can cite several runs. Then run `bp create thread --title "<question>" --project <project> --body-file <file> --json`.

A long list of runs can exceed the slice. Walk again after you cite the runs that it holds.

Stop: `bp walk <seed> --for triage-drift --done --json` returns `done` true.
