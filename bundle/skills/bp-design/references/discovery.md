# Discovery

Load this file when the human cannot name the key events and nouns of an area. Load it also when the glossary does not hold them. A new project or a new domain area loads it too. Run it before the file of the kind.

Discovery produces no design of its own. It produces three outputs:

- Terms, one for each noun, in the glossary or its interim home.
- Hotspots, as rows in `## Open` of the design that discovery feeds.
- Candidate contexts, for `references/strategic.md`.

Choose one method for each discovery. Recommend it to the human with the reason.

- Big Picture EventStorming (Brandolini, *Introducing EventStorming*) suits a flow of business facts over time.
- Domain Storytelling (Hofer and Schwentner, *Domain Storytelling*) suits work that passes between people and systems. Use it when the human can tell the work as a concrete story.

## Big Picture EventStorming

The grammar comes from DDD Crew, "EventStorming Glossary & Cheat Sheet".

1. Ask the human to list domain events for five minutes. An event is a fact in the past tense, such as "order shipped". Show no candidate until the human stops.
2. Add events that the evidence holds: code, logs, commit history, the vault and session transcripts. Mark each added event with its source. The union of the two lists is the timeline.
3. Reject each command, such as "create order", and each state, such as "order list". Rewrite it as a fact, or mark it as a hotspot.
4. Order the events from left to right. Merge the duplicates.
5. Flag each word that names two concepts. Each such word marks a candidate context boundary (Fowler, "BoundedContext").
6. Propose four to eight pivotal events. A pivotal event splits the flow into phases. The human confirms or moves each one.
7. Walk the timeline from the end to the start. At each event, ask what had to happen first. Ask one question for each gap.
8. Mark the hotspots: each conflict, question and pain. A disagreement between the human and the evidence is a hotspot. Never resolve a hotspot yourself.
9. Add the actors and the external systems at the events that they touch.
10. Ask the human to rank the hotspots. The top hotspot names the next work. One process that needs a precise flow goes to `references/workflow-model.md`.

## Domain Storytelling

1. Fix the scope with the human on three axes: coarse or fine, as-is or to-be, pure or digitalized. Start coarse, pure and as-is.
2. The human tells one concrete story. Write each sentence as you hear it: actor, activity, work object, and the actor who receives it. Number the sentences.
3. Allow no branch in a story. Tell each variant as a second story.
4. Read the numbered story back to the human. Correct it until the human accepts each word.
5. Find each work object that hides inside an activity name. Make it a work object of its own.
6. Group the sentences where the language changes. Each group is a candidate context.
7. Tell the same story again, fine and to-be.

## The outputs

1. Write one term for each noun on an event and each work object. Give it a definition of one sentence.
2. Choose the home of the terms. When the glossary `projects/<p>/designs/glossary` exists, it is the home. A draft glossary takes the rows through `bp set <glossary> --body-file <file> --json`. An active glossary changes through `references/revision.md`.
3. With no glossary, a draft product design is the home. Add the terms to its `## Vocabulary` table with `bp set <product> --body-file <file> --json`.
4. With no glossary and an active product design, hold the terms in the working notes. The system design session creates the glossary from them: `references/system.md`, "The glossary".
5. Use one row form for each table. A `## Vocabulary` row is `| <Noun> | <definition> |`. A glossary row is ``- `<slug>` — <definition>.``. The slug is the noun in lower case, with each space as a hyphen.
6. Write each hotspot as one row in `## Open` of the design that discovery feeds. Give its source.
7. Hand the candidate contexts to `references/strategic.md`. They are the phases, the words with two meanings and the story groups.
8. Keep the timeline and the stories as working notes outside the vault. Quote in a design only the part that the design cites.

## Diagram

The event timeline is not among the five diagram types. Keep it as a numbered list, grouped by phase.

A domain story is a working note, so its diagram is a preview outside the vault. Draw it as a `workflow` diagram when the human asks to see it. Each actor is a lane. Each numbered sentence is one step, in order. The label of a step is its activity. The `term` of a step is the glossary slug of its work object. Until the glossary holds that slug, the preview warns on the step. Preview it with `bp diagram --spec-file <f> --out <path> --json`. To keep the flow in the vault, draw it in the workflow model through `references/workflow-model.md`. The spec and the rules are in `references/diagrams.md`.

## Gate questions

- Did the human's list come before the evidence?
- Does the timeline read from end to end, with no gap that the human can name?
- Is each event a fact in the past tense?
- Does each noun have a definition of one sentence?
- Does each story sentence hold one actor, one activity and one work object?
- Did the human accept the read-back word for word?
- Is each hotspot written down, and is none resolved without the human?
