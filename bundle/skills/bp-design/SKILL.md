---
name: bp-design
description: Use when the human asks for a design, or a concern has no design. Draft one design with the human, a section at a time.
---

Intent: the human asks for a design, or a concern of the project has no design. You write one design node as `draft`. The human confirms each section.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. In this text, `<p>` is the project slug and `<project>` is `projects/<p>/project`.

1. Choose the parent and the slug. The parent is one level above the new design:
   - Slug `product`: the parent is `<project>`.
   - Slug `system`: the parent is the product design.
   - Slug `data-model`, `glossary` or `containers/<name>`: the parent is the system design.
   - Slug `components/<name>`: the parent is a container design or the system design.
2. Read the slice: `bp walk <parent> --for design --json`. It holds the level above and the sibling designs.
3. Read the concerns: `bp show <project> --section Concerns --json`. A `no_section` refusal means that the project lists no concern.
4. List the concern answers: `bp list concern-answer --project <p> --limit 1000 --json`. Each id ends in its stamp.
5. Read the latest answer of each concern with `bp show <answer> --json`. Its `state` is the state of the concern.
6. Read the glossary: `bp show projects/<p>/designs/glossary --json`. A `no_node` refusal means that the project has none. Use its terms in each section.
7. Get the body template: `bp create design --template --json`. Each `TODO(bp): ` line in `template` is one slot.
8. Show the human one section at a time. Go to the next section only after the human confirms the current one.
9. Write the confirmed body to a file outside the vault. No `TODO(bp): ` line stays.
10. Validate the node first: `bp create design --project <p> --slug <slug> --title "<title>" --under <parent> --body-file <file> --dry-run --json`.
11. Fix each item of `findings` in the file. Then run the same call without `--dry-run`.
12. Link each decision and convention that the design obeys: `bp link <design> cites <node> --json`.
13. Record each concern that the design answers: `bp record concern-answer <project> --field concern=<concern> --field state=designed --field design=<design> --json`.
14. Tell the human that the draft exists. Activation is the human's later act.

A convention is a design at shared scope, with no project and no parent. For a convention, use the kind `convention` in steps 7 and 10. Drop `--project`, `--slug` and `--under` from the calls.

Stop: the stop is call-shaped. Step 11 returns a `commit`, and `bp walk <design> --for design --done --json` returns `done` true.
