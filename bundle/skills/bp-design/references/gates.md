# Gates

Load this reference last, every time. For a new design, run it on the draft after the create call, before the human confirms the draft. For a revision, run it on the proposed body file before the `record delta` call. It produces one verdict per gate and an `## Open` entry for each gap that stays.

A new design passes all twelve gates. A delta passes the gates that `references/revision.md` names for its class.

Write each gap that stays into `## Open`. On a draft, write the body with `bp set <design> --body-file <file> --json`. On a revision, write it into the proposed body file. On the editorial path, tell the human each gap, and add no section or heading.

## Solo moves

A design workshop has many people in one room. You have one human, the evidence and a fresh agent. Each reference uses these seven moves. They adapt the workshop practice of Brandolini, *Introducing EventStorming*.

1. On a judgement step, the human writes first. Then you show your candidates. Judgement steps are the problem, the top characteristics, the core domain, a risk score and the leaning of a decision.
2. Treat the evidence as the other participants: the code, its history, the vault and the transcripts. Give each item that you add its source.
3. Keep the grammar of the method. Mark each breach as a hotspot.
4. Write each disagreement between the human and the evidence in `## Open`. Do not settle it alone.
5. The human ranks. You propose an order, with the value and the difficulty of each item.
6. For an independent view, dispatch a fresh agent. It acts as a second scorer, a second modeller or a cold reader.
7. Show one section. The human confirms or edits it. Then show the next section.

## The twelve gates

Each gate gives its test, then how you verify it.

**G1 Terms**. Each domain noun in the prose is a glossary term or a noun of the design. Each diagram node is too.
Verify: read each noun of the prose against the glossary.

**G2 Confirmed sections**. The human confirmed each section. You wrote no section after an unconfirmed one.
Verify: you showed each section, and the human confirmed it before the next. The design stays `draft` until the human activates it.

**G3 Measured characteristics**. Each named characteristic has a quality scenario of six parts (Bass, Clements and Kazman, *Software Architecture in Practice*). It holds a numeric measure, the value today and a check.
Verify: a `scenario` or `shell` block in `## Checks` for each measure. Write a scenario with no block in `## Open`.

**G4 Choices are decisions**. Each choice with a rejected option is a decision node (Nygard, "Documenting Architecture Decisions"). The node holds the options, the consequences and the check that enforces it.
Verify: the design cites each decision with `bp link <design> cites <decision> --json`.

**G5 Bounds**. The design names its non-goals and what it does not own. It names the part that owns each one.
Verify: the section that states the non-goals, or what is out of scope, names at least one part.

**G6 Explicit open items**. Each hotspot and unknown is in `## Open`. None blocks activation unless the human says so.
Verify: ask the human which entries block.

**G7 Diagrams**. Each diagram passes the review checklist of Brown, *The C4 model*. Each arrow has a label. One diagram shows one level, and one design is its home.
Verify: the editorial rules of `references/diagrams.md`.

**G8 Ancestors agree**. No sentence contradicts the parent design or a cited decision.
Verify: compare each claim with the parent in the slice and with each cited decision.

**G9 Cold read**. A fresh agent reads only the design. It says what the design is, why it exists and what it does not do. The readability scenario allows five minutes for this.
Verify: the dispatch below.

**G10 Evidence traces**. Each number cites its measurement: an analysis file or a run. A sentence that names code cites the path and the line range.
Verify: read each number and find its source. A `pin` block on the range records `skipped` until its runner ships.

**G11 Concerns answered**. Each project concern in the scope of this design has an answer. The answer is designed, waived with a reason and an expiry, or open.
Verify: `bp list concern-answer --under <project> --json`, then read the latest answer of each concern in scope.

**G12 No placeholders**. No template slot stays.
Verify: the `findings` of the dry run. A draft adds `--dry-run` to its create or set call. A revision adds `--dry-run` to its `record delta` call.

## The cold-read dispatch

Run it on the draft after the create call, before the human confirms the draft. For a revision, run it on the proposed body file before `record delta`.

1. Fill the research template of the `bp-researcher` role. Put the design id on the `Node:` line.
2. After the five lines, write this question. For a revision, replace the second line with the path of the body file.

```text
Skip steps 1 to 3 of your role.
Read only the design on the Node line, with bp show. Read no other node, file or source.
In one line each, say what the design does, why it exists, and what it does not do.
Then list each sentence that you could not follow, and why.
```

3. Compare the three answers with the lead, the section that states the scope, and the section that states the non-goals. A wrong answer is a defect of the design, not of the reader.
4. Fix each defect in its section. The human confirms the fix.
5. Tell the human the three answers in the session. Write each defect that you did not fix into `## Open`.

## Report

Give the human one line per gate: `pass`, or the gap and its `## Open` entry. A gap that the human calls blocking stops activation.
