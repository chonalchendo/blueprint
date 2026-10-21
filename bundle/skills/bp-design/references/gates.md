# Gates

Load this reference last, every time. For a new design, run it after the draft exists and before you tell the human. For a revision, run it before the `record delta` call. It produces one verdict per gate, a `## Cold read` section, and an `## Open` entry for each gap.

A new design passes all twelve gates. A delta passes the gates that `references/revision.md` names for its class.

## Solo moves

A design workshop has many people in one room. You have one human, the evidence and a fresh agent. Each reference uses these seven moves. They adapt the workshop steps of the DDD Crew, *DDD Starter Modelling Process*.

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
Verify: the diagram verb resolves each node when it writes. Read each noun of the prose against the glossary.

**G2 Confirmed sections**. The human confirmed each section. You wrote no section after an unconfirmed one.
Verify: the session record. The design stays `draft` until the human activates it.

**G3 Measured characteristics**. Each named characteristic has a scenario of six parts. It holds a numeric measure, the value today and a check.
Verify: a `scenario` or `shell` block in `## Checks` for each measure. Write a scenario with no block in `## Open`.

**G4 Choices are decisions**. Each choice with a rejected option is a decision node. The node holds the options, the consequences and the check that enforces it.
Verify: the design cites each decision with `bp link <design> cites <decision> --json`.

**G5 Bounds**. The design names its non-goals and what it does not own. It names the part that owns each one.
Verify: `## Out of scope` names at least one part.

**G6 Explicit open items**. Each hotspot and unknown is in `## Open`. None blocks activation unless the human says so.
Verify: ask the human which entries block.

**G7 Diagrams**. Each diagram passes the review checklist of Brown, *The C4 model*. It has a title, a type, a scope and a legend that matches its marks. Each arrow has a direction and a label. One diagram shows one level, and one design is its home.
Verify: the rules of `references/diagrams.md`.

**G8 Ancestors agree**. No sentence contradicts the parent design or a cited decision.
Verify: compare each claim with the parent in the slice and with each cited decision.

**G9 Cold read**. A fresh agent reads only the design. It says what the design is, why it exists and what it does not do. The readability scenario allows five minutes for this.
Verify: the dispatch below.

**G10 Evidence traces**. Each number cites its measurement: an analysis file or a run. A sentence that names code has a `pin` block on the code range.
Verify: read each number and find its source.

**G11 Concerns answered**. Each project concern in the scope of this design has an answer. The answer is designed, waived with a reason and an expiry, or open.
Verify: the concern answers of the draft procedure.

**G12 No placeholders**. No template slot stays.
Verify: the `findings` of the dry run.

## The cold-read dispatch

Run it after the draft exists, and before you tell the human. For a revision, the reader reads the proposed body file in place of the design.

1. Fill the research template of the `bp-researcher` role. Put the design id on the `Node:` line.
2. After the five lines, write this question. For a revision, replace the first line with the path of the body file.

```text
Read only the design on the Node line, with bp show. Read no other node, file or source.
In one line each, say what the design does, why it exists, and what it does not do.
Then list each sentence that you could not follow, and why.
```

3. Compare the three answers with the lead, `## Scope` and `## Out of scope`. A wrong answer is a defect of the design, not of the reader.
4. Fix each defect in the section. The human confirms the fix.
5. Write the three answers and the list under `## Cold read`. Write the body with `bp set <design> --body-file <file> --json`.

## Report

Give the human one line per gate: `pass`, or the gap and its `## Open` entry. A gap that the human calls blocking stops activation.
