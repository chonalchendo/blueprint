## Blueprint

The vault holds this project's designs, milestones, issues and reasoning. `bp` is the only writer of the vault.

- Do not edit a vault file by hand. `bp where --json` names the vault.
- Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do.
- Start each task from `bp walk <id> --for <action> --json`. Read a node from `ids` with `bp show <node> --json` only when a step needs it.
- At session start, the context holds the map of a walk of the last seed. The map has the action, the intents and the top ids. Act on its first intent. With no intent, ask the human for the task. Its `next` field is the call that returns the bodies.

### Intent to action

- A design is missing: run the skill `bp-design`.
- `needs-shaping`: an active design has an unproven concern, and no milestone is `planned` or `approved`. A concern is unproven when its latest answer is `open`, or `designed` with no milestone stamped after it. Run the skill `bp-shape`.
- `needs-derivation`: an approved milestone has fewer than three live issues, and a finish-line block that no issue covers. Run the skill `bp-derive`.
- `ready`: an issue is open and unclaimed. Dispatch `bp-worker` with its `complete` template.
- `needs-review`: its note names the round and each lens still missing. Dispatch `bp-reviewer` once per lens with its `review` template, its `Lens:` and `Round:` filled. Then record each review.
- Round one ends when each lens has a review at one head. Each later head is the next round: one review with the lens `verify`. If the level rose, the note also names each new lens.
- A review whose rows are all `warn` or `risk` is approvable: tell the human so in one line.
- `blocked` with the note "round cap: the human decides": give the human the error rows. Dispatch no one.
- A review's record call returns `late`: error rows outside the fix. For a low or medium issue, create one follow-up issue for each under the same milestone. Then decline the row: `bp record disposition <review> --field finding=<n> --field as=declined --field reason="follow-up <issue>" --json`.
- `needs-response`: a review has a finding with no disposition. Dispatch `bp-worker` with its `respond` template.
- `drifted`: a drift run failed, and nothing cites it. Run the skill `bp-triage-drift`.
- The context cannot answer a question: run the skill `bp-deliberate`.
- You need facts from outside the vault: dispatch `bp-researcher` with its `research` template.
- An issue landed, a thread closed, or a run holds evidence: run the skill `bp-distil`.
- The human parks a thought: run the skill `bp-capture`.
- `blocked`: do what its note names.
- `working` with the note `stale`: run `bp release <issue> --reason lost --json`. Then dispatch again.
- `working` without `stale`: do nothing; the worker or the cause in its note moves it.
- `waiting`: do nothing until the cause in its note clears.
- `needs-approval`, `needs-landing`, `needs-milestone-approval`, `needs-acceptance` or `re-asked`: the human acts. Give the human the line below that names the intent.

### Dispatch

- A dispatch template is a file `<role>-<action>.md` in the `bp/dispatch/` directory beside the skill directory.
- Copy the template. Fill its four lines: `Node:`, `Skill:`, `Forbidden:` and `Stop:`. Add no text above the four lines.
- A research dispatch holds its question below the four lines. A review dispatch carries its lens and round there: `Lens: <lens>` and `Round: <n>`.
- The reviewer writes nothing in the vault or in a worktree; it probes in a copy. Write its review, from `head:` to the end of its `What holds:` list, to a file outside the vault.
- Then run the record call on the last line of the reply. Its `--body-file` names that file. The record call returns `rows`, the count of the review's findings; its `findings` are the body lint, not the review's.

### The human's acts

Give the human these commands. Only the human runs `bp approve <issue>` and `bp lower <issue> low --reason <why>`, in a terminal. The signing key never goes into an agent.

- `needs-approval`: approve the pull request on the forge.
- `needs-milestone-approval`: `bp transition <milestone> approved --json`.
- To activate a design draft: `bp transition <design> active --json`.
- `needs-landing`: `bp land <issue> --json`.
- `needs-acceptance`: `bp stamp <milestone> --json`.
- `re-asked`: a waived block is open again. Its note names the `--waive` for the next stamp.
