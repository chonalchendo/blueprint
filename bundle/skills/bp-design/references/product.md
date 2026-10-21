# Product design

Load this file when the kind is product. It produces the body of `projects/<p>/designs/product`, under the project. Load `references/discovery.md` before this file when the human cannot name the domain's key events and nouns.

Start from the body template. Keep its lead sentence, `## Out of scope` and `## Constraints`. Replace `## Scope` with these sections: Problem, Appetite, User, Goals, Solution sketch, Vocabulary and Open.

The method is shaping, from Singer, *Shape Up*. The backbone and the walking skeleton come from Patton, *User Story Mapping*. The Wardley map comes from Wardley, Wardley mapping. It joins only when a build or buy choice is live.

## Procedure

1. Ask the human for the problem before you show anything. Write it in the human's words. Output: `## Problem`.
2. Ask the human for the appetite. The appetite is the time that the idea is worth to the human. Output: `## Appetite`.
3. Ask the human to tell one user's day, from start to end. Write each activity in the order that the human tells it. This row of activities is the backbone.
4. Under each activity, write the user tasks. Under each task, write the stories. Output: `## User`, with one row for each activity.
5. Read the backbone back to the human. Ask which step is missing and which step hurts. Write each gap as a row in `## Open`.
6. Propose the walking skeleton: the smallest story under each activity, so that the row runs end to end. The human moves stories in or out. Mark each skeleton story in `## User`. The skeleton seeds the first milestone.
7. Write the goals. Give each goal a measure and, where one exists, its value today. A goal with no measure goes to `## Open`. Output: `## Goals`.
8. Rough out the solution above the level of a wireframe. Name the places, the affordances at each place and the connections between places. This sketch is a breadboard. Output: `## Solution sketch`.
9. Walk the sketch for rabbit holes. A rabbit hole is an unknown that can stall the build.
10. Where a repository exists, seed rabbit holes from its history: escalations, reverted pull requests and abandoned branches. Cite the source of each one.
11. Show one rabbit hole at a time. The human cuts it or specifies a patch. Write each patch in `## Solution sketch`. Write each cut in `## Out of scope`.
12. Shaping leaves no rabbit hole open. This file departs from the source in one case only. A hole that the human cannot settle now becomes a row in `## Open`. The bet is not shaped until that row closes.
13. State the no-gos: what this bet excludes. Each no-go names the part that does the work, or says that no part does it. Output: `## Out of scope`.
14. List each noun that the sections use. Output: `## Vocabulary`, a table with one row for each noun. The row form is `| <Noun> | <definition of one sentence> |`.
15. The system design session copies these rows into the glossary: `references/system.md`, "The glossary". Add no row here after the glossary exists.

### Wardley map, when build or buy is live

1. Name the anchor: the user and the need.
2. Draft the chain of capabilities that meet the need, by dependency. Seed it from the dependency manifests and the system context.
3. Ask the human to place each capability on the evolution axis: genesis, custom, product or commodity. This step is judgement, so the human goes first.
4. Mark the movement that the human expects.
5. Read off the choice for each capability. Build genesis and custom. Buy product. Rent commodity.
6. A choice with a rejected option is a decision. Load `references/decision.md` for it.

## Diagram

The story map and the Wardley map are not among the five diagram types. Keep the backbone, the skeleton and the map as tables.

Draw the breadboard as a `workflow` diagram only when a table hides how places hand on work. Each place is a step. Each connection is an edge with one active verb as its label. Each node names a `## Vocabulary` noun as its `noun`. The spec, the rules and the write call are in `references/diagrams.md`.

Nouns and the verbs between them belong to the conceptual data model, not to this design. Draw them through `references/data-model.md`.

## Gate questions

- Can the human state the problem and the appetite in one sentence each?
- Does the backbone list user activities, not features?
- Does each activity hold at least one skeleton story?
- Does the skeleton run from end to end?
- Is each rabbit hole cut, patched or an `## Open` row that blocks the bet?
- Does the sketch stay above the level of a wireframe?
- Does each goal carry a measure?
- Does each mapped capability trace back to the anchor need?
- Does each build or buy choice have its decision?
