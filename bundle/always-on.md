## Blueprint

The vault holds this project's designs, milestones, issues and reasoning. `bp` is the only writer of the vault.

- Do not edit a vault file by hand. `bp where --json` names the vault.
- Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do.
- Start each task from `bp walk <id> --for <action> --json`. Read a node from `ids` with `bp show <node> --json` only when a step needs it.
- At session start, the context holds the map of a walk of the last seed. The map has the action, the intents and the top ids. Act on its first intent. With no intent, ask the human for the task. Its `next` field is the call that returns the bodies.

### Intent to action

- A design is missing: run the skill `bp-design`.
- `needs-shaping`: an active design has an open concern, and no milestone is `planned` or `approved`. Run the skill `bp-shape`.
- `needs-derivation`: an approved milestone has fewer than three live issues, and a finish-line block that no issue covers. Run the skill `bp-derive`.
- `ready`: an issue is open and unclaimed. Dispatch `bp-worker` with its `complete` template.
- `needs-review`: dispatch `bp-reviewer` with its `review` template. Then record the review.
- After the worker pushes fixes for a review, the pull request has a new head: run the walk again. `needs-review` then means dispatch `bp-reviewer` again with the same five lines, then record the review.
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
- Copy the template. Fill its five lines: `Node:`, `Skill:`, `Budget:`, `Forbidden:` and `Stop:`. Add no text above the five lines.
- A research dispatch holds its question below the five lines.
- The reviewer writes nothing in the vault or in a worktree; it probes in a copy. Write its review, from `head:` to the end of its `What holds:` list, to a file outside the vault.
- Then run the record call on the last line of the reply. Its `--body-file` names that file.

### The human's acts

Give the human these commands:

- `needs-approval`: approve the pull request on the forge.
- `needs-milestone-approval`: `bp transition <milestone> approved --json`.
- To activate a design draft: `bp transition <design> active --json`.
- `needs-landing`: `bp land <issue> --json`.
- `needs-acceptance`: `bp stamp <milestone> --json`.
- `re-asked`: a waived block is open again. Its note names the `--waive` for the next stamp.
