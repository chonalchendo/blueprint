# Workflow model

Load this reference when the design is a workflow model. The human names the level: conceptual or logical. A workflow model is the lifecycle view of the system design. It sits beside the data model.

Each level produces its own content.

- Conceptual: roles, actions, state machines, scenario walks and an anti-pattern check. It answers who moves what, and through which states.
- Logical: one row per action, with its trigger, its context, its tool calls, its product and its stop. It answers how each action runs.

Run one process per pass. Pick it from a hotspot of discovery, or take the lifecycle that the human names. When the human cannot name the process's key events, load `references/discovery.md` first.

## Conceptual level

Show the human one section at a time. Go on only after the human confirms it.

1. **Name the roles**. The human lists who acts first. The agent then adds roles from the evidence: commit authors, tool logs and transcripts. Each role that the agent adds cites its source. Give each mechanism, such as a hook or a schedule, to the one role that owns it.
   Output: `## Roles`, one row per role with what only that role does. Each role becomes a glossary term.

2. **Lay out the process**. Use the process-level grammar of EventStorming, left to right (Brandolini, *Introducing EventStorming*; DDD Crew, *EventStorming Glossary & Cheat Sheet*). An actor reads a read model and issues a command. A system takes the command and produces an event. A policy reacts to the event and issues the next command. Process level stops at commands, policies, read models and systems. The rule-holding aggregates belong to `references/component.md`.
   - The agent drafts the as-is flow from the code and the logs, with a source for each step.
   - The human states the to-be flow where it differs.
   - The agent enforces the grammar. Each event follows a command, an external system or a policy. Each policy reads "whenever this event, then that command".
   - A break in the grammar is a hotspot.

   Output: the event list and the command list, and each hotspot in `## Open`.

3. **Name the actions**. An action is one bounded move by one role. Give each action its role, what it reads, what it writes and its effect on state. Merge the commands of step 2 into actions where one role issues them as one move.
   Output: `## Actions`, one row per action.

4. **Draw the states and transitions**. Do this for each entity with a lifecycle. List its states. Give each transition the action and the role that cause it. Store a state only when it records a decision or a landing. Derive every other state from the records, and say how.
   Output: `## State machines`, one per entity.

5. **Walk the scenarios**. Pick three to five real scenarios: the main path, one failure, one escalation, and one human act. Walk each one through the actions. Each step names its role and its effect on state. A step with no action is a gap in the model.
   Output: `## Scenario walks`, with each gap in `## Open`.

6. **Test for the four anti-patterns**. These adapt Reis's entity anti-patterns to actions (Reis, *Mixed Model Arts*).
   - A god action does the work of many roles or many moves. Split it.
   - A phantom action has a name but the evidence never shows it run. Fold it into another action, or delete it.
   - A split action is one move under two or more names. Merge them.
   - Temporal confusion stores in one field what a thing is and what happened to it. Split the state from its history.

   Output: `## Anti-pattern check`, one line per anti-pattern with what the model does about it.

## Logical level

Write one row per action of the conceptual level. Each row holds:

1. **Role**. The one role that runs the action.
2. **Trigger**. The condition that starts the action. Write it as a predicate over records, so the tool can compute it.
3. **Context**. What the action must read before it acts.
4. **Tool calls**. The verbs the action uses, and no others.
5. **Product**. The one record or node that the action writes. A chat message is never a product.
6. **Stop**. The call whose result shows that the action is done.

Then order the triggers. When two triggers hold at once, the first in order names the next action. Give each trigger that the tool computes a `shell` or `scenario` check. The check plants records and confirms which action the tool names.

Output: `## Actions`, one row per action, and the checks under `## Checks`.

## Gate questions

- Does each action have exactly one role?
- Does each stored state record a decision or a landing?
- Does each derived state name the records it comes from?
- Does each scenario walk reach its end with no step that lacks an action?
- Does each policy name the event that triggers it?
- At the logical level, does each trigger read records only, and does each stop name one call?
- Did the anti-pattern test leave nothing that `## Open` does not hold?

## Diagram

A workflow model draws three types. None of them takes a level. Read `references/diagrams.md` for the spec fields and the editorial rules. Preview each view with `bp diagram --spec-file <f> --out <path> --json` before you write it.

- **`state`**. Draw one view per entity with a lifecycle, from step 4. Label each transition with its action, and with its role when two roles share the entity. Give the entry the role `start`, each terminal state the role `end` and each branch the role `choice`. Make the landing state and its arrow the focus.
- **`sequence`**. Draw one view per scenario walk that crosses three or more roles, from step 5. Each party is a role or a system. Each step is one message, and a reply is a return step. Make the human's acts the focus. Past five parties, `bp` warns; merge the parties that never talk to each other.
- **`workflow`**. Draw this view when the handoffs between roles matter more than the messages. Give each role one lane. Place each step in its lane, in order. Give the start the role `start`, each end the role `end` and each branch the role `decision`.

`bp` places all three types from their order, so give no cells.
