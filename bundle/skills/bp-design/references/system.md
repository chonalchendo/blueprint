# System design

Load this file when the kind is system. Load `references/quality.md` with it. It produces the body of `projects/<p>/designs/system`, under the product design. The sections are Context, Characteristics, Style, Solution strategy, Containers, Constraints, Non-goals and Open. Its inputs are the product design and, when loaded, the output of `references/strategic.md`.

The diagram levels come from Brown, the C4 model. The characteristics, the quantum and the styles come from Richards and Ford, *Fundamentals of Software Architecture*. The cut reasons come from Ford, Richards, Sadalage and Dehghani, *Software Architecture: The Hard Parts*. The coverage list comes from the arc42 template.

## Procedure

1. List the people who use the system and the external systems that it uses or serves. Seed the list from evidence: manifests, network calls, credentials and the forge. The human confirms the list.
2. Write the context. Output: `## Context`, a table of element, kind and relation. The kind is person, system or external. The relation is one active verb.
3. Ask the human to name the top three characteristics before you show a candidate.
4. Add candidates from three sources: the domain concerns, the explicit requirements and the implicit knowledge of the domain. Keep the list short. Each characteristic adds cost, and some conflict.
5. Challenge the top three with evidence: co-change, change frequency and failure history. The human keeps or changes each one. Output: `## Characteristics`, the top three first, each with its reason.
6. Name the quanta. A quantum is a unit that deploys on its own and has high cohesion. When the quanta differ, scope each characteristic to its quantum.
7. Pick the style that favours the top three. The styles are layered, modular monolith, pipeline, microkernel, service-based, event-driven and microservices. Name the best rejected style and why it lost. The style is a decision: load `references/decision.md`. Output: `## Style`, one line that names the decision.
8. Write each strategy choice as one line, with the product goal that it serves. Output: `## Solution strategy`.
9. Cut the containers. A container is a runtime boundary: an application, a data store, or code that runs inside another runtime. A library is not a container. Cut in this order: runtime boundary, bounded context, data ownership, change rate and risk, and technology last. For each split, name a disintegrator with evidence: scope, code volatility, scalability, fault tolerance, security or extensibility. For each merge, name an integrator: transactions, workflow, shared code or data relationships.
10. Write the containers. Output: `## Containers`, a table of container, kind, responsibility, and cut reason with its evidence. Each container gets its own design later, through `references/container.md`.
11. Write the constraints and the non-goals. Each non-goal names the part that does the work, or says that no part does it. Output: `## Constraints` and `## Non-goals`.
12. Use the arc42 sections as a coverage list, not as a template. The sections are goals, constraints, context and scope, solution strategy, building blocks and runtime. The rest are deployment, crosscutting concepts, decisions, quality, risks and debt, and glossary. For each section, name its answer: this design, a child design, the glossary or a decision. A section with no answer is a row in `## Open`.
13. Write the quality goals through `references/quality.md`. Before the human activates the design, run `references/risk-threat.md`.

## Diagram

The system design draws one required diagram: an `architecture` diagram at level `context`.

- The system is one node with the role `system`. It is the focus.
- Each person is a node with the role `person`. Each external system is a node with the role `external`.
- Each node names a glossary slug as its `term`, or a `## Context` row as its `noun`.
- Each edge label is one active verb. The subject is the `from` node.

Two more diagrams are optional:

- An `architecture` diagram at level `container`, as the overview of the containers.
- A `sequence` diagram for one key runtime scenario, when the interaction is complex or recurs.

Preview each spec for the human with `bp diagram --spec-file <f> --out <path> --json`. The spec, the rules and the write call are in `references/diagrams.md`.

## Gate questions

- Does the context diagram show each person and each external system that a container touches?
- Did the human name the top three characteristics first?
- Are no more than three characteristics marked top?
- Does each container give a cut reason with evidence?
- Is any container a library, or a technology layer with no reason?
- Does the style decision name the best rejected style?
- Does each arc42 section have an answer or a row in `## Open`?
