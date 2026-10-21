# Decisions

Load this file when the kind is decision. Load it in the middle of another procedure when a section settles a choice with a rejected option. It produces one decision node in the `proposed` state, and the links that cite it.

The record form comes from Nygard, "Documenting Architecture Decisions". The classes of significant choice come from Richards and Ford, *Fundamentals of Software Architecture*. The trade-off table comes from Ford, Richards, Sadalage and Dehghani, *Software Architecture: The Hard Parts*.

## A decision or a design line

A choice is a decision when all three statements below are true.

- It has at least two real options.
- It is costly to reverse. It sets structure, characteristics, dependencies, interfaces or a construction technique.
- Another design, an issue or a later choice must cite it.

Otherwise the choice is a design line. Write it in its section, with its reason in the same paragraph.

## Procedure

1. Search for a decision that covers the choice: `bp search <terms> --kind decision --json`. If one covers the choice and still holds, cite it and stop here.
2. Ask the human for the leaning before you show an option.
3. Write the forces as facts, and include the forces in conflict. Name the forces that would make the choice wrong later.
4. Name at least two real options.
5. Build the strongest case for the best option that the human leans against.
6. Build a trade-off table: each option against each top characteristic of the design.
7. Write the choice as one sentence in the active voice: "We will …".
8. Write the consequences: positive, negative and neutral. A list of benefits alone is not complete.
9. Name the check that enforces the choice: its design and its block name. If no check can enforce it, write "review" and why.
10. Get the body template: `bp create decision --template --json`. Fill it in a file outside the vault. The lead is the "We will" sentence. `## Reason` holds the forces and the consequences. `## Options rejected` holds the options, the strongest case and the trade-off table. `## Enforcement` holds the check.
11. Validate: `bp create decision --project <p> --title "<title>" --body-file <file> --dry-run --json`. Fix each item of `findings`. Then run the same call without `--dry-run`.
12. Link the design that the choice governs: `bp link <design> cites <decision> --json`.
13. Link the thread that the choice settles: `bp link <decision> answers <thread> --json`.
14. A new choice never edits an accepted decision. Write a new decision, and link it: `bp link <decision> supersedes <old> --json`.
15. Tell the human that the decision is `proposed`. The human accepts it with `bp transition <decision> accepted --json`.

## Diagram

A decision draws no diagram. The trade-off table carries the comparison.

When the options differ in structure, preview one spec for each option. Use `bp diagram --spec-file <f> --out <path> --json` for each preview. Nothing enters the vault. The chosen structure enters the diagrams of the design that the decision governs, through `references/diagrams.md`.

## Gate questions

- Does the decision name at least two real options?
- Does the best rejected option have its strongest case?
- Do the consequences name a cost, not only benefits?
- Does a check enforce the choice, or does the body say why review must?
- Can a newcomer tell from the forces when to revisit the choice?
- Does each design that the choice governs cite the decision?
