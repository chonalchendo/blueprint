---
name: bp-design
description: Use when the human asks for a new or changed design. Also use when a concern has no design. Route to the method, then draft or revise the design with the human, one section at a time.
---

Intent: the human asks for a new or changed design, or a project concern has no design. You write one design node as `draft`, or one delta on a design past `draft`. The human confirms each section.

Pass `--json` to each `bp` call. On exit 1, 2 or 4, read `code` and run `fix` at most once. Exit 3 means the call had nothing to do. In this text, `<p>` is the project slug and `<project>` is `projects/<p>/project`.

## Route

Ask the six questions in order. Each answer names the references to load. Run one procedure at a time, in the order of the questions. Keep at most four method references in view. When the human confirms the sections of a reference, drop it from view. Load `references/diagrams.md` when a section needs a diagram. Load `references/gates.md` last, every time.

1. What kind of design is it?
   - Product: load `references/product.md`.
   - System: load `references/system.md` and `references/quality.md`.
   - Glossary: load `references/system.md`. Its glossary section creates and extends the glossary.
   - Container: load `references/container.md` and `references/quality.md`.
   - Component: load `references/component.md`.
   - Data model: load `references/data-model.md`. A project has one data model, and each container's level is a section of it.
   - Workflow model: load `references/workflow-model.md`. It writes the `## Workflow model` section of the system design.
   - Decision: load `references/decision.md`.
   - Convention: load no reference. Write the rule and why it holds. Add one example, one counter-example, its scope and its check.
2. Is the design new, a draft, or past `draft`?
   - New: follow "Draft a new design".
   - A draft: show the human each changed section. Write the confirmed body with `bp set <design> --body-file <file> --json`. Then run the gates of the changed sections.
   - Past `draft`: the design changes only through a delta. Load `references/revision.md` first. Then load the reference of question 1 for the changed sections only.
   - Past `draft`, and the change keeps the meaning: this is the editorial delta. Load only `references/revision.md` and `references/gates.md`. Skip questions 3 to 6.
3. Which level is it? For a data model, ask conceptual, logical or physical. For a workflow model, ask conceptual or logical. Use only that level's section of the reference.
4. Does the glossary name the events and nouns of the area? First ask the human to name the key events. If the human cannot, load `references/discovery.md`. Also load it when the glossary lacks the terms. Run discovery before the reference of question 1.
5. Does the area hold more than one model? Look for a word with two meanings. Also look for parts that change at different rates. For a system or a container, a yes loads `references/strategic.md`. For a container, also load it when the human disputes the cut.
6. Does a trust boundary or a security concern apply? A yes loads `references/risk-threat.md`. A system or container design also loads it before the human activates it.

Load `references/decision.md` at any step where a section settles a choice and rejects an option.

Tell the human the route in one line: each answer and each reference. Recommend the route. The human can change it.

## Draft a new design

1. Choose the parent and the slug. The parent is one level above the new design:
   - Slug `product`: the parent is `<project>`.
   - Slug `system`: the parent is the product design.
   - Slug `data-model`, `glossary` or `containers/<name>`: the parent is the system design.
   - Slug `components/<name>`: the parent is a container design or the system design.
2. Read the slice: `bp walk <parent> --for design --json`. It holds the level above and the sibling designs.
3. Read the concerns: `bp show <project> --section Concerns --json`. A `no_section` refusal means that the project lists no concern.
4. List the concern answers: `bp list concern-answer --under <project> --limit 1000 --json`. Each id ends in its stamp.
5. Read the latest answer of each concern with `bp show <answer> --json`. Its `state` is the state of the concern.
6. Read the glossary: `bp show projects/<p>/designs/glossary --json`. A `no_node` refusal means that the project has none. Use its terms in each section.
7. Get the body template: `bp create design --template --json`. Each `TODO(bp): ` line in `template` is one slot.
8. Run the procedure of each loaded reference. Each step names the section it fills and the output to check.
9. Show the human one section at a time. Go to the next section only after the human confirms the current one.
10. Write the confirmed body to a file outside the vault. No `TODO(bp): ` line stays.
11. Validate the node first: `bp create design --project <p> --slug <slug> --title "<title>" --under <parent> --body-file <file> --dry-run --json`.
12. Fix each item of `findings` in the file. Then run the same call without `--dry-run`.
13. Write each confirmed diagram: `bp diagram <design> <name> --spec-file <spec> --json`. Preview it first, as `references/diagrams.md` says.
14. Link each decision and convention that the design obeys: `bp link <design> cites <node> --json`.
15. Record each concern that the design answers: `bp record concern-answer <project> --field concern=<concern> --field state=designed --field design=<design> --json`.
16. Run each gate of `references/gates.md` on the draft after the create call, before the human confirms the draft. It holds the cold-read dispatch. Fix each gap in the draft with `bp set <design> --body-file <file> --json`.
17. Tell the human that the draft exists. Give one line per gate. Activation is the human's later act.

For a convention, get the template with `bp create convention --template --json`. Validate it with `bp create convention --title "<title>" --body-file <file> --dry-run --json`. Skip steps 1 to 6, 13 and 15. Run gates G1, G2, G4, G9 and G12 only.

## Revise a design past draft

1. Run the procedure of `references/revision.md`. It finds the impact by walk and sets the class of the change. It names the gates to run again. A breaking change also runs `bp check <design> --json` on the design and on each child.
2. The procedure ends with one `record delta` call on the design. An editorial delta needs no milestone.
3. Tell the human that the delta exists, with its class. Acceptance is the human's act.

## Stop

Stop: the stop is call-shaped. The create call (Draft, step 12) or the delta call (Revise, step 2) returns a `commit`. Then `bp walk <design> --for design --done --json` returns `done` true.
