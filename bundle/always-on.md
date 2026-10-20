## Blueprint

The vault holds this project's designs, milestones, issues and reasoning. `bp` is the only writer of the vault.

- Do not edit a vault file by hand. `bp where --json` names the vault.
- Pass `--json` to each `bp` call. On a non-zero exit, read `code` and run `fix`.
- Start each task from `bp walk <id> --for <action> --json`. Read a node from `ids` with `bp show <node> --json` only when a step needs it.
- At session start, the context holds a walk of the last seed. Act on its first intent.

### Intent to action

- A design is missing: run the skill `bp-design`.
- An active design has an open concern, and no milestone is planned: run the skill `bp-shape`.
- An approved milestone has a finish-line block that no issue covers: run the skill `bp-derive`.
- An issue is `ready`: dispatch `bp-worker` with its `complete` template.
- An issue is `needs-review`: dispatch `bp-reviewer` with its `review` template. Then record the review.
- A review has a finding with no disposition: dispatch `bp-worker` with its `respond` template.
- A drift run failed, and nothing cites it: run the skill `bp-triage-drift`.
- The context cannot answer a question: run the skill `bp-deliberate`.
- You need facts from outside the vault: dispatch `bp-researcher` with its `research` template.
- An issue landed, a thread closed, or a run holds evidence: run the skill `bp-distil`.
- The human parks a thought: run the skill `bp-capture`.
- An intent is `blocked`: do what its note names.
- A claim is `working` with the note `stale`: run `bp release <issue> --reason lost --json`. Then dispatch again.

### Dispatch

- A dispatch template is a file `<role>-<action>.md` in the `bp/dispatch/` directory beside the skill directory.
- Copy the template. Fill its five lines: `Node:`, `Skill:`, `Budget:`, `Forbidden:` and `Stop:`. Add no other text.
- The reviewer writes nothing. Write its findings table to a file outside the vault.
- Then run `bp record review <issue> --field head=<sha> --body-file <file> --json`. `<sha>` is the head that it read.

### The human's acts

The human approves each change on the forge. The human also approves a milestone, activates a design, lands a change and stamps a milestone. Give the human these commands:

- `bp transition <milestone> approved --json`.
- `bp transition <design> active --json`.
- `bp land <issue> --json`.
- `bp stamp <milestone> --json`.
