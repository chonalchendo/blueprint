# Workflow model

Load this reference when the human asks for the workflow model. The human names the level: conceptual or logical. A workflow model is the lifecycle view of the system design. It is not a design of its own.

Each level produces its own content.

- Conceptual: roles, actions, state machines, scenario walks and an anti-pattern check. It answers who moves what, and through which states.
- Logical: one row per action, with its role, trigger, inputs, outputs and done condition. It answers how each action runs.

## Where it lives

Write the model as one section of the system design, `## Workflow model`. Each output below is a `###` subsection of that section. Its diagrams sit under the system design: `bp diagram projects/<p>/designs/system <name> --spec-file <f> --json`. When the system design is active, change the section and its diagrams through `references/revision.md`.

Run one process per pass. Pick it from a hotspot of discovery, or take the lifecycle that the human names. When the human cannot name the process's key events, load `references/discovery.md` first.

## Conceptual level

Show the human one subsection at a time. Go on only after the human confirms it.

1. **Name the roles**. The human lists who acts first. The agent then adds roles from the evidence: logs, tickets, code and transcripts. Each role that the agent adds cites its source. Give each mechanism, such as a scheduled job, to the one role that owns it.
   Output: `### Roles`, one row per role with what only that role does. List each new role name in `## Open` as a glossary candidate. The glossary design takes it up in its own write.

2. **Lay out the process**. Use the process-level grammar of EventStorming, left to right (Brandolini, *Introducing EventStorming*; DDD Crew, *EventStorming Glossary & Cheat Sheet*). An actor reads a read model and issues a command. A system takes the command and produces an event. A policy reacts to the event and issues the next command. Process level stops at commands, policies, read models and systems. The rule-holding aggregates belong to `references/component.md`.
   - The agent drafts the as-is flow from the code and the logs, with a source for each item.
   - The human states the to-be flow where it differs.
   - The agent enforces the grammar. Each event follows a command, an external system or a policy. Each policy reads "whenever this event, then that command".
   - A break in the grammar is a hotspot.

   Output: `### Events and commands`, in flow order, and each hotspot in `## Open`.

3. **Name the actions**. An action is one bounded move by one role. Give each action its role, what it reads, what it writes and its effect on state. Merge the commands of step 2 into one action where one role issues them as one move.
   Output: `### Actions`, one row per action.

4. **Draw the states and transitions**. Do this for each entity with a lifecycle. List its states. Give each transition the action and the role that cause it. Store a state only when no record can derive it, such as a decision or an outside outcome. Derive every other state from the records, and say how.
   Output: `### State machines`, one per entity.

5. **Walk the scenarios**. Pick three to five real scenarios: the main path, one failure, one escalation and one manual step. Walk each one through the actions. Each step names its role and its effect on state. A step with no action is a gap in the model.
   Output: `### Scenario walks`, with each gap in `## Open`.

6. **Test for the four anti-patterns**. These adapt Reis's entity anti-patterns to actions (Reis, *Mixed Model Arts*).
   - A god action does the work of many roles or many moves. Split it.
   - A phantom action has a name but the evidence never shows it run. Fold it into another action, or delete it.
   - A split action is one move under two or more names. Merge them.
   - Temporal confusion stores in one field what a thing is and what happened to it. Split the state from its history.

   Output: `### Anti-pattern check`, one line per anti-pattern with what the model does about it.

## Logical level

Write one row per action of the conceptual level. Each row holds:

1. **Role**. The one role that runs the action.
2. **Trigger**. The condition that starts the action. Where you can, write it as a condition over stored data, so that the system can compute it.
3. **Inputs**. What the action must read before it acts.
4. **Outputs**. What the action writes or changes: a record, a document, or a message to another system.
5. **Done condition**. The one result that shows the action is complete.

Then order the triggers. When two triggers hold at once, the first in order names the next action. Give each trigger that the system computes a `shell` or `scenario` check. The check sets up the stored data and confirms which action starts.

Output: `### Action rows`, one row per action, and each check in the system design's `## Checks`.

## Gate questions

- Does each action have exactly one role?
- Does each stored state hold something that no record can derive?
- Does each derived state name the records it comes from?
- Does each scenario walk reach its end with no step that lacks an action?
- Does each policy name the event that triggers it?
- At the logical level, does each done condition name one result that a reader can observe?
- Did the anti-pattern test leave nothing that `## Open` does not hold?

## Diagram

A workflow model draws three types. None of them takes a level. Read `references/diagrams.md` for the spec fields and the editorial rules. Preview each view with `bp diagram --spec-file <f> --out <path> --json` before you write it under the system design.

- **`state`**. Draw one view per entity with a lifecycle, from step 4. Label each transition with its action. Add the acting role to the label when two roles share the entity. Give the entry the node role `start`. Give each terminal state the node role `end`, and each branch `choice`. Make the state that the lifecycle exists to reach the focus.
- **`sequence`**. Draw one view per scenario walk that crosses three or more roles, from step 5. Each party is a role or a system. Each step is one message, and a reply is a return step. Make one or two decisive steps the focus, such as a person's approval. Past five parties, `bp` warns; merge the parties that never talk to each other.
- **`workflow`**. Draw this view when the handoffs between roles matter more than the messages. Give each role one lane. Place each step in its lane, in order. Give the start the node role `start`, each end the node role `end` and each branch the node role `decision`.

`bp` places all three types from their order, so give no cells.
