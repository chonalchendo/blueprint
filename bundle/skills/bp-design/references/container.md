# Container design

Load this reference when the design is a container: slug `containers/<name>` under the system design. It produces one container design. The design states the container's responsibility, its components and the reason for each cut, its interfaces and its runtime scenarios. It also scopes the characteristics that hold for this container.

A container is a runtime boundary: an application that runs, or a store that holds data (Brown, *The C4 model*). A library, a folder or a package is not a container. If the candidate is one of those, stop and ask the human where it belongs.

## Inputs

- The system design's row for this container, with its cut reason.
- The bounded context that the container holds, when the system design names one.
- The system design's top characteristics.
- The glossary, and the code when it exists.

## Procedure

Show the human one step's output at a time. Go on only after the human confirms it.

1. **Write the responsibility**. The human writes one sentence first: what the container does, in the domain's words. Then the agent compares that sentence with the system design's row. A difference is a hotspot.
   Output: the lead sentence, `## Scope`, and each hotspot in `## Open`.

2. **Fill the bounded context canvas**. One bounded context per container is common, but it is not a rule (Evans, *Domain-Driven Design*). Fill the canvas in its order (DDD Crew, *Bounded Context Canvas*). First the name and the purpose. Then the classification, the inbound and outbound messages and the language. Then the business decisions, the assumptions and the verification metrics. The agent fills the messages from the code: imports, calls and emitted events. Each item that the agent adds cites its file. The human fills the purpose and the business decisions.
   Output: the interface rows of step 5, the terms for the glossary, and `## Assumptions`. A verification metric becomes a check or a row in `## Open`.

3. **Cut the components**. List the candidate components. For each split, weigh the disintegrators against the integrators (Ford, Richards, Sadalage and Dehghani, *Software Architecture: The Hard Parts*). The disintegrators are scope, code volatility, scalability, fault tolerance, security and extensibility. The integrators are shared transactions, workflow, shared code and data relationships. A split cites one disintegrator with its evidence. A merge cites one integrator with its evidence. Evidence is co-change, change frequency, failure history or a measured load. Never cut by size, by team chart or by technical layer alone.
   Output: `## Components`, one row per component with its cut reason.

4. **Scope the characteristics**. An architecture quantum deploys alone and holds high functional cohesion (Richards and Ford, *Fundamentals of Software Architecture*). Characteristics hold per quantum, not for the whole system. The human picks, from the system's top characteristics, the ones that hold here. Pick three at most. The agent then challenges each pick with evidence. Load `references/quality.md` for each pick's scenario and its check.
   Output: `## Characteristics`, each with a `scenario` or `shell` check, or a row in `## Open`.

5. **State the interfaces**. For each interface, name what it takes, what it gives and each error it returns. Write the rows so that a contract diff can read them. An open host service or a published language gets a `contract` check with a baseline (DDD Crew, *Context Mapping*).
   Output: `## Interfaces` and a `contract` block.

6. **Name the permitted connections**. List each container and component that this container may call, with the direction. The context map's upstream and downstream edges set this list.
   Output: a `deps` block that lists the permitted edges.

7. **Walk the runtime scenarios**. Pick two to four flows: the main path, one failure and one concurrent case. Write each flow as numbered steps, with one actor per step.
   Output: `## Runtime scenarios`.

8. **Record the rejected alternatives**. Give each cut that lost a row: the option, and why it lost. A choice that binds another design loads `references/decision.md`.
   Output: `## Rejected alternatives`, and a decision node where the choice binds another design.

9. **Get a second cut**. Dispatch a fresh agent with the canvas and the code, but not your component list. It proposes its own cut. Each difference is a hotspot. Never resolve a difference silently.
   Output: each difference as a row in `## Open`.

When the cut is in question and the area holds more than one model, load `references/strategic.md` before step 3. When the container has a trust boundary, load `references/risk-threat.md` before activation.

## Gate questions

- Does each split cite a disintegrator with evidence, and each merge an integrator?
- Is every box in the views a runtime boundary, and not a library or a folder?
- Does each characteristic name the quantum it holds for?
- Does each interface name its errors, and not only its success path?
- Does `## Out of scope` name the part that does each job this container refuses?
- Does `## Open` hold each difference from the second cut?

## Diagram

A container design draws three kinds of view. Read `references/diagrams.md` for the spec fields and the editorial rules. Preview each view first with `bp diagram --spec-file <f> --out <path> --json`. Then write it with `bp diagram <design> <name> --spec-file <f> --json`.

- **Components: `architecture` at level `component`**. This view is required. Put this container's components in one group, the container boundary. Put each neighbour that a component calls outside the group. The system's container view drills into this view. Give each component the role `component`, and each store the role `store`. Label each edge with a verb that shows its intent.
- **Neighbours: `architecture` at level `container`**. Draw this view only when the system design's container view does not answer the question. One case is a container whose stores and callers the system view leaves out. Make this container the focus.
- **Runtime scenarios: `sequence`**. Draw one view per runtime scenario that crosses three or more parties. Each party is a container, a person or an external system. `bp` places the steps from their order.

The deployment level waits for a project that needs it. A view past nine boxes is a sign to split it by component group.
