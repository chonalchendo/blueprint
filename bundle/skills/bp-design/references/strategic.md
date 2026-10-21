# Strategic design

Load this file for a system design that holds more than one model. Load it for a container design when its cut is in dispute. Two signs show more than one model: a word with two meanings, or parts that change at different rates.

It produces three sections of the system design: `## Subdomains`, `## Bounded contexts` and `## Context map`. It adds the team fit to `## Constraints`.

Loaded for a container design, it writes the canvas of that container's context only. The canvas goes into the sections that `references/container.md` names. A changed cut changes `## Containers` of the system design. Change an active system design through `references/revision.md`. Its input is the candidate contexts from `references/discovery.md`. Its output feeds `references/system.md` and `references/container.md`.

## Subdomains

The classes come from Evans, *Domain-Driven Design*, and Khononov, *Learning Domain-Driven Design*. The chart comes from DDD Crew, "Core Domain Charts".

1. List the subdomains from the discovery phases and the product backbone.
2. Ask the human to rate the differentiation of each subdomain first. The question is: can a competitor copy it easily?
3. Rate the complexity of each subdomain from evidence: churn, defect history, size and co-change clusters. Cite each number.
4. Classify each subdomain. Core differentiates. Supporting is necessary but gives no edge. Generic is the same for everyone.
5. Write the investment for each class. Give each core subdomain a domain model and the best effort. Buy a generic subdomain, or keep it simple.
6. A bought subdomain is a decision. Load `references/decision.md` for it.

Output: `## Subdomains`, a table of subdomain, class, differentiation, complexity and reason. A class is a hypothesis. Put a class that the human doubts in `## Open`.

## Bounded contexts

The cut by language comes from Evans and from Fowler, "BoundedContext". The canvas comes from DDD Crew, "Bounded Context Canvas".

1. Draw a candidate boundary where the language changes.
2. Fill the canvas for each candidate, in order. First fill the name, the purpose, the class and the domain roles. Then fill the inbound and outbound messages, the terms and the business decisions. Last, fill the assumptions, the verification metrics and the open questions.
3. Fill the messages from code: imports, calls and events. Ask the human for the purpose and the business decisions.
4. Write a verification metric for each context. The metric shows when the boundary is wrong.
5. Test each boundary against the cut reasons of `references/system.md`, step 9.
6. Dispatch the `bp-researcher` role with its `research` template. Below its five lines, give the evidence and ask for an independent cut. Do not give your cut.
7. The report returns the second cut, with a reason for each boundary. Compare the two cuts. Write each difference as a hotspot in `## Open`.

Output: `## Bounded contexts`, one `###` heading for each context. Each holds its purpose in one sentence of business language. It also holds its terms, messages, decisions, assumptions and metrics. Each term goes to the glossary. Many open questions on a canvas show high uncertainty.

## Context map

The relations and patterns come from DDD Crew, "Context Mapping".

1. Place each context and each external system.
2. Propose the direction of each pair from the dependency direction in code: upstream and downstream, mutual, or free.
3. Name the pattern of each edge. The patterns are open-host service, published language, conformist, anticorruption layer and shared kernel. Four more are partnership, customer and supplier, separate ways, and big ball of mud.
4. Ask the human to pick the pattern where it is a choice, such as conformist or anticorruption layer. Each such pick is a decision.
5. Name the owner of change for each shared kernel.
6. Write one table for each question. Do not put every pattern in one table.

Output: `## Context map`, with one table for each question. Each table has the columns upstream, downstream, pattern and reason. Each upstream and downstream pair becomes a permitted connection in a `deps` check block of the container design. Each open-host service or published language becomes the baseline of a `contract` check block.

## Team fit

The team types and modes come from Skelton and Pais, *Team Topologies*.

1. With more than one team, name the team type of each context: stream-aligned, platform, enabling, or complicated subsystem.
2. Name the interaction mode of each pair of teams: collaboration, X-as-a-service, or facilitating.
3. With one owner, ask one question only: which part needs specialist knowledge?
4. Test the cognitive load. A context whose design does not fit in one walk budget is too big for one agent.

Output: one line for each context in `## Constraints`.

## Diagram

The core domain chart and the context map graph are not among the five diagram types. Keep both as tables.

When each context is one container, the `architecture` diagram at level `container` of the system design shows the map. Each edge label names the pattern as an active verb, such as "conforms to". The spec and the rules are in `references/diagrams.md`.

## Gate questions

- Are few subdomains core, and does each core subdomain name why a competitor cannot copy it?
- Does a measure support each complexity rating?
- Does each context hold one purpose sentence and its own terms?
- Does each context name a metric that shows a bad boundary?
- Do two contexts share most of their terms? If so, merge them.
- Does each edge of the map carry a direction and a pattern?
- Does each conformist or anticorruption choice give its reason?
- Does each shared kernel name its owner of change?
