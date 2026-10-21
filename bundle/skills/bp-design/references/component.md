# Component design

Load this reference when the design is a component: slug `components/<name>`. Its parent is a container design or the system design. It produces one component design. The design states what the component owns and what it does not own. It also states the interface: commands in, events out and errors. It names the invariants, the commands they guard and the implementation pattern.

Load it with `references/data-model.md` when the component owns stored state. A component in a supporting or generic subdomain with no invariants needs steps 1, 3 and 6 only.

## Inputs

- The parent container design: its row for this component, its interfaces and its permitted connections.
- The glossary terms of the bounded context.
- The code when it exists: handlers, error types, emitted events and tests.

## Procedure

Show the human one step's output at a time. Go on only after the human confirms it.

1. **Bound the component**. The human writes one sentence first: what the component does. Then the human lists what it does not own. For each item it does not own, the agent names the part that owns it.
   Output: the lead sentence, `## Scope` and `## Out of scope`.

2. **Run design-level EventStorming**. Lay the flow out left to right in one grammar (Brandolini, *Introducing EventStorming*; DDD Crew, *EventStorming Glossary & Cheat Sheet*). An actor reads a read model and issues a command. A constraint guards the command, and the command produces an event. A policy reacts to the event and issues the next command. At design level, the constraint is the aggregate that holds the rule.
   - The agent drafts the as-is flow from the code. Each sticky that the agent adds cites its file and line.
   - The human states the to-be flow where it differs from the code.
   - The agent enforces the grammar. Each event follows a command, an external system or a policy. Each policy reads "whenever this event, then that command".
   - A break in the grammar is a hotspot.

   Then group the commands and the events by the aggregate that guards them.
   Output: `## Commands`, `## Events`, a glossary term for each command and event name, and each hotspot in `## Open`.

3. **Choose the implementation pattern**. Match the pattern to the subdomain class and to the complexity of the rules (Khononov, *Learning Domain-Driven Design*). Simple logic takes a transaction script or an active record. Complex rules take a domain model. When history and time are first-class, take an event-sourced domain model. The human decides; the agent shows the evidence for each option. A choice with a rejected option loads `references/decision.md`.
   Output: one sentence in `## Constraints`, or a cited decision.

4. **Fill the aggregate design canvas**. Fill one canvas per aggregate (DDD Crew, *Aggregate Design Canvas*). Its parts are the name, the description and the boundary trade-offs, and the state transitions. Then the enforced invariants, the corrective policies, the handled commands and the created events. Last come the throughput and the size.
   - The agent fills the commands and events from step 2.
   - The agent fills the throughput and the size from measurements where the code runs. A value with no measurement is a row in `## Open`.
   - The human confirms each invariant. An invariant is a business rule, and only its owner can say that it is one.
   Output: `## Invariants`, each naming the command it guards, with a `shell` check that names its test.

5. **Find the edge cases**. Use Example Mapping on each invariant (Wynne, *Introducing Example Mapping*). The invariant is the rule. The agent proposes boundary examples for it, and the human states the outcome of each. An outcome that the human cannot state is a question.
   - Many questions mean that the component is not ready for activation.
   - One rule with many examples is two rules. Split it.
   Output: each example as a `shell` check or as a check on an issue. Each question goes in `## Open`.

6. **State the interface**. Give each command, event and error one stable name. Name the input and the result of each command. Name the payload of each event and the condition of each error.
   Output: `## Interface`, and a `contract` block with a baseline.

7. **Hand on the stored state**. The aggregate's stored state feeds the logical data model. The size and the lifetime feed the physical data model. Load `references/data-model.md` for that work.
   Output: the nouns and the attributes that the data model takes up, listed in `## Open` until it does.

## Gate questions

- Does each invariant name the command it guards, and can a test break it?
- Does each command have one aggregate that guards it?
- Are the corrective policies few? Many policies mean that logic leaked out of the aggregate.
- Does each aggregate have a bound on its size and on its lifetime?
- Does the pattern match the subdomain class, and not the habit of the code base?
- Do states and events stay apart? An event names a fact that happened, and a state names a condition that holds.

## Diagram

A component design draws two kinds of view. Read `references/diagrams.md` for the spec fields and the editorial rules. Preview each view with `bp diagram --spec-file <f> --out <path> --json` before you write it.

- **Lifecycle: `state`**. Draw one view per aggregate that owns a lifecycle. Take the states and the transitions from the canvas of step 4. Label each transition with the command or the event that moves it. Give the entry the role `start`, each exit the role `end` and each branch the role `choice`. `bp` places the states from their order, with the main path along one row. Make the state that the aggregate exists to reach the focus.
- **Parts: `architecture` at level `component`**. Draw this view only when it adds value (Brown, *The C4 model*). One case is a component whose internals split into parts that the container's view does not show. Make this component the focus. Most component designs need no such view, because the parent container already draws it.

A class diagram is not one of the five types. Put the attributes of an aggregate in a table instead.
