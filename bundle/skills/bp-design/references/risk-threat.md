# Risk and threat

Load this file before the human activates a system design or a container design. Load it when the project class has a security concern, or a trust boundary exists. Trust boundaries include agent credentials, forge tokens, user data and the network edge. It produces two sections of the design: `## Risks` and `## Access and controls`.

Risk storming comes from Brown, risk storming. Richards and Ford, *Fundamentals of Software Architecture*, adopt it. The four questions come from the Threat Modeling Manifesto. The six threat categories come from Microsoft, STRIDE categories.

## Risk storming

1. Start from the `architecture` diagram of the design, at level `context` or `container`. Each risk sits on one node or one edge. Name a node by its id, and an edge as `<from>→<to>`.
2. Ask the human to list and score risks alone, before you show any. Score likelihood and impact each as 1 (low), 2 (medium) or 3 (high). The risk score is their product.
3. Dispatch the `bp-researcher` role with its `research` template. Below its four lines, give the diagram and the design, but not the human's list. `bp show <design> --json` lists `diagrams`; read a spec at its `spec` path from the vault root. Its report returns a scored list of risks.
4. Merge the two lists. Group the risks that are alike.
5. Discuss only the differences: a risk that one side alone raised, and a score that the two sides disagree on.
6. Plan a mitigation for each high risk first, then for each medium risk. A score of 6 or 9 is high. A score of 3 or 4 is medium.
7. Give each mitigation one of three forms. The first form is a check block in `## Checks` of this design. The second form is a row in `## Open` that names the work for a milestone.
8. The third form accepts the risk. Accepting one risk is a decision: load `references/decision.md`. The decision names the risk, the reason and the trigger to revisit it.
9. Do not waive a project concern for one risk. A waiver answers the whole concern and replaces its `designed` state.

Output: `## Risks`, a table of risk, node or edge, score, mitigation and measure. Name no risk in general terms, such as "scalability". Each risk names its place on the diagram.

## Threat model

1. What are we working on? List the processes, the stores, the flows, the external entities and the trust boundaries.
2. What can go wrong? For each flow that crosses a trust boundary, test the six categories in turn. They are spoofing, tampering, repudiation, information disclosure, denial of service and elevation of privilege. List each threat that applies.
3. Ask the human which threats are real. Never drop a threat yourself.
4. What will we do? Give each real threat one response: mitigate, eliminate, transfer or accept. Accepting a threat is a decision, as for a risk.
5. Did we do a good job? Read each crossing flow against the list. Each crossing flow has a pass over all six categories.
6. Write each control. Give each control a `shell` or a `deps` check block where code can show it.

Output: `## Access and controls`, a table of control, threat, enforcing part, check and state today.

Avoid three failures. One person does not model alone as the hero. The diagram need not be perfect to start. A list of threats with no control is not a result.

## Diagram

Risk storming draws no new diagram. Each row of `## Risks` names the node or the edge that it sits on.

The threat model draws an `architecture` diagram at level `container`, for a system design and for a container design. Each trust boundary is a group. Each process has the role `application`, or `component` inside one container. Each store has the role `store`, and each external entity has the role `external`. Each flow that crosses a boundary is an edge with one active verb as its label. The spec and the rules are in `references/diagrams.md`.

## Gate questions

- Did the human score the risks before the agent showed a list?
- Did a second, blind scorer score the same diagram?
- Does each risk name its place on the diagram?
- Does each high risk have a check, an `## Open` row or a decision that accepts it?
- Does each flow that crosses a boundary have a pass over all six categories?
- Is each accepted risk and threat a decision?
- Does each real threat have a control, and does each control name the part that enforces it?
