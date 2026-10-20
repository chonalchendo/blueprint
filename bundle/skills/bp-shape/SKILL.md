---
name: bp-shape
description: Use when an active design has an open concern and no milestone is planned or approved. Propose the next milestone, and write it when the human agrees.
---

Intent: needs shaping. An active design has an open concern, and the project has no milestone `planned` or `approved`. You propose one milestone. The human decides its scope.

Pass `--json` to each `bp` call. On a non-zero exit, read `code` and run `fix`. In this text, `<p>` is the project slug.

1. Choose the seed: a design of the project, or `projects/<p>/project`. A shared design has no project, so its stop never holds.
2. Read the slice: `bp walk <seed> --for shape --json`. It holds the product and system designs, and the learnings that touch them.
3. Read the concerns: `bp list concern-answer --project <p> --json`. A concern is open when its latest answer has the state `open`.
4. Read the proposed deltas: `bp list delta --project <p> --json`. A delta whose milestone is `abandoned` needs a new carrier.
5. Propose one milestone to the human in one message:
   - The bet, in one sentence.
   - The goal, and what is out of scope.
   - The finish line, as one predicate per line.
   - Each design change that it carries.
6. One bet closes one open concern, or a small set that shares one design change. Leave the rest for a later milestone.
7. Wait until the human agrees or changes the proposal. Write nothing before that.
8. Get the body template: `bp create milestone --template --json`.
9. Write the body to a file outside the vault. No `TODO(bp): ` line stays.
   - The first line is the bet.
   - `## Goal` holds the goal and what is out of scope.
   - `## Finish line` holds one fenced block for each predicate. Its info line is `check shell {name=<name> expect=0}`.
10. Make each predicate run without its author. It runs from the code repository's top, with no network and no human.
11. A predicate calls the built binary or the shipped interface. It does not search the source text.
12. Give each block a unique name. Derive covers a predicate when an issue holds a block with that name.
13. Create the node: `bp create milestone --project <p> --title "<title>" --under <design> --body-file <file> --json`. `<design>` is the design whose finish line it moves, usually `projects/<p>/designs/system`.
14. Read `findings` in the result. Fix each one in the file, then run `bp set <milestone> --body-file <file> --json`.
15. Record each design change that the milestone carries. Write the whole proposed design body to a file outside the vault.
   - Then run `bp record delta <design> --field milestone=<milestone> --field change="<what changes>" --body-file <file> --json`.
16. Tell the human that the milestone is `planned`. Approval is the human's act.

Stop: `bp walk <seed> --for shape --done --json` returns `done` true.
