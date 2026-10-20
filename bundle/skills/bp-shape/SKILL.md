---
name: bp-shape
description: Use when an active design has an open concern and no milestone is planned or approved. Propose the next milestone, and write it when the human agrees.
---

Intent: `needs-shaping`. An active design has an open concern, and the project has no milestone `planned` or `approved`. You propose one milestone. The human decides its scope.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. In this text, `<p>` is the project slug.

1. Choose the seed: a design of the project, or `projects/<p>/project`. A shared design has no project, so its stop never holds.
2. Read the slice: `bp walk <seed> --for shape --json`. It holds the product and system designs, and the learnings that touch them.
3. Read the concerns: `bp show projects/<p>/project --section Concerns --json`. A `no_section` refusal means that no concern is open.
4. List the concern answers: `bp list concern-answer --project <p> --limit 1000 --json`. Each id ends in its stamp.
5. Read the latest answer of each concern with `bp show <answer> --json`. A concern is open when its `state` is `open`.
6. List the deltas: `bp list delta --project <p> --limit 1000 --json`.
7. Read each delta with `bp show <delta> --no-body --json`. It gives `milestone` and `state`.
8. Read the status of each `proposed` delta's milestone with `bp show <milestone> --no-body --json`. A delta whose milestone is `abandoned` needs a new carrier.
9. Propose one milestone to the human in one message:
   - The bet, in one sentence.
   - The goal, and what is out of scope.
   - The finish line, as one predicate per line.
   - Each design change that it carries.
10. One bet closes one open concern, or a small set that shares one design change. Leave the rest for a later milestone.
11. Wait until the human agrees or changes the proposal. Write nothing before that.
12. Get the body template: `bp create milestone --template --json`.
13. Write the body to a file outside the vault. No `TODO(bp): ` line stays.
   - The first line is the bet.
   - `## Goal` holds the goal and what is out of scope.
   - `## Finish line` holds one fenced block for each predicate. Its info line is `check shell {name=<name> expect=0}`.
14. Make each predicate run without its author. It runs from the code repository's top, with no network and no human.
15. A predicate calls the built binary or the shipped interface. It does not search the source text.
16. Derive covers a predicate when an issue holds a block with the same name. Choose names that say what the block proves.
17. Validate the node first: `bp create milestone --project <p> --title "<title>" --under <design> --body-file <file> --dry-run --json`. `<design>` is the design that the milestone changes, usually `projects/<p>/designs/system`.
18. Fix each item of `findings` in the file. Then run the same call without `--dry-run`.
19. Record each design change that the milestone carries. Write the whole proposed design body to a file outside the vault.
   - Then run `bp record delta <design> --field milestone=<milestone> --field change="<what changes>" --body-file <file> --json`.
20. Tell the human that the milestone is `planned`. Approval is the human's act.

Stop: `bp walk <seed> --for shape --done --json` returns `done` true.
