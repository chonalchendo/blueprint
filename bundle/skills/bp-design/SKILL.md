---
name: bp-design
description: Use when the human asks for a design, or a concern has no design. Draft one design with the human, a section at a time.
---

Intent: the human asks for a design, or a concern of the project has no design. You write one design node as `draft`. The human confirms each section.

Pass `--json` to each `bp` call. On a non-zero exit, read `code` and run `fix`. In this text, `<p>` is the project slug and `<project>` is `projects/<p>/project`.

1. Choose the parent, one level above the new design:
   - A product design sits under `<project>`.
   - A system design sits under the product design.
   - A data model, a glossary or a container design sits under the system design.
   - A component design sits under a container design or the system design.
2. Read the slice: `bp walk <parent> --for design --json`. It holds the level above, the sibling designs and the project.
3. Read the concerns. Run `bp show <project> --section Concerns --json` and `bp list concern-answer --project <p> --json`. The latest answer of a concern gives its state.
4. Get the body template: `bp create design --template --json`. Each `TODO(bp): ` line in `template` is one slot.
5. Write the sections with the human. Show one section, and wait until the human confirms or changes it. Then go to the next section.
6. Use the terms of the glossary design. Name each part as the level above names it.
7. Write the confirmed body to a file outside the vault. No `TODO(bp): ` line stays.
8. Create the node: `bp create design --project <p> --slug <slug> --title "<title>" --under <parent> --body-file <file> --json`.
   - The slug is `product`, `system`, `data-model`, `glossary`, `containers/<name>` or `components/<name>`.
9. Read `findings` in the result. Fix each one in the file, then run `bp set <design> --body-file <file> --json`.
10. Link each decision and convention that the design obeys: `bp link <design> cites <node> --json`.
11. Record each concern that the design answers: `bp record concern-answer <project> --field concern=<concern> --field state=designed --field design=<design> --json`.
12. Tell the human that the draft exists. Activation is the human's later act.

Stop: the stop is call-shaped. Step 8 returns a `commit`, and `bp walk <design> --for design --done --json` returns `done` true.
