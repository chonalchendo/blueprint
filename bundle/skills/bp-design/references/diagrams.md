# Diagrams

Load this reference before you write a diagram spec, for a draft or for a revision. It produces one spec per diagram. The spec is `<design>/diagrams/<name>.yaml`, and `bp` renders the page `<name>.html` beside it. You write the spec. `bp` measures, routes and renders it, and checks the hard rules of layout.

## Procedure

1. **Decide to draw**. Draw only when a table or a paragraph cannot show the point. Name the one question that the diagram answers.
2. **Choose the type and the level**. Use the list under "Type per level". One diagram shows one level.
3. **List the nodes and the edges**. Start from the section that the diagram serves. Delete each node and edge that the question does not need. Check the size against the budget of the type.
4. **Choose the focus**. Give the id of the element that the reader must see first.
5. **Place the boxes**. For `architecture` and `er`, give each node a grid cell. The section on placement says how.
6. **Write the spec** to a file outside the vault. The section on fields lists each key.
7. **Preview it:** `bp diagram --spec-file <spec> --out <page> --json`. This writes nothing to the vault. Read `findings`. A finding with `severity` `error` refuses the spec, and a `warn` finding does not. The preview resolves each term only when it finds the vault.
8. **Show the page to the human**. Change the spec until the human confirms the drawing.
9. **Write it**. For a draft design: `bp diagram <design> <name> --spec-file <spec> --json`. To drop one: `bp diagram <design> <name> --remove --json`. For a design past `draft`, the delta carries the spec, as `references/revision.md` says.
10. **Add the check**. Put one `nodes` block in the `## Checks` of the body file before the create call. For a revision, put it in the proposed body file. Each later check run then reads every spec.

```text
~~~check nodes {name=diagram}
~~~
```

Write the block once for each design.

## Type per level

The levels follow Brown, *The C4 model*. Each line gives the design that is the home, then the type and its level. A diagram lives under its home design.

- Product: `workflow` for the hand-offs, in lanes by order. The product's nouns are drawn in the data model.
- System: `architecture` at `context`, and `architecture` at `container` with one boundary group.
- System, workflow model section: `state` for each lifecycle, `sequence` for each scenario walk, `workflow` for hand-offs. These diagrams sit under the system design.
- Container: `architecture` at `component`, which is required. The system's container view drills into it.
- Container, optional: `architecture` at `container` for its neighbours, and `sequence` for each runtime scenario.
- Component: `state` for each lifecycle. A scenario `sequence` belongs to the workflow model, under the system design. Add `architecture` at `component` only for internals that the container view does not show.
- Data model: `er` at `conceptual`, `logical` or `physical`. The project has one data model. Its views split by domain or by container, and all sit under the data-model design.

These are the five types. A deployment level of `architecture` waits for a project that needs it. A method chart outside the five stays a table in the design. Story maps, timelines and quadrants are examples. A new type needs a design that no listed type can draw.

## Fields

Top keys: `type`, `level`, `title`, `lede`, `focus`, `nodes`, `groups`, `edges`, and `legend` when you override it. A sequence names `parties` and `steps` in place of `nodes` and `edges`. A workflow adds `lanes`.

- `title` names the scope. The `lede` is one sentence: what the reader sees.
- A node has a stable `id`: lowercase letters, digits and hyphens. A diff pairs nodes by this id.
- A node takes exactly one of `term` and `noun`. A `start`, `end`, `choice` or `decision` node needs neither. If it gives one, `bp` checks it. A `term` is a glossary term. A `noun` is a noun that the home design names in a heading or the first column of a table. A state is a noun: the home design names each state in a table.
- `label` is the text in the box. It defaults to the term or the noun.
- `role` takes one value from the closed set of its type.
  - `architecture`: `application`, `library`, `store`, `deployable`, `component`, `system`, `person`, `external`.
  - `er`: `entity`, `node`, `record`, `row`, `elsewhere`. An `elsewhere` node is an entity of another domain, with its `owner`.
  - `state`: `state`, `start`, `end`, `choice`. `workflow`: `step`, `start`, `end`, `decision`.
- Optional node keys: `at`, `span`, `note`, `sub`, `fields`, `time`, `owner`, `lane`. An `er` node and a `start`, `end`, `choice` or `decision` node draw no `note`, so `bp` refuses one there.
- A group has `id`, `label` and `members`. It draws a boundary.
- A `lanes` item has `id` and `label`. Each workflow node names its lane in `lane`.
- A `legend` item has `role` or `style`, and `label`. `bp` refuses an item that has no key or no `label`. Without `legend`, `bp` draws the legend from the marks in use.
- An edge has `from`, `to` and `label`. `style` is `solid` or `dashed`. `both` draws two heads. `exit` and `enter` take `top`, `right`, `bottom` or `left`. An `er` edge takes `card`: the cardinality at `from`, then at `to`. Give an edge an `id` only when two edges share their ends.
- A sequence step has `from`, `to` and `say`. `back` draws the return on the next row. `kind` is `call` or `return`.

```yaml
type: architecture
level: container
title: "Orders: two containers and a store"  # The scope.
lede: The API is the only writer of the order store.
focus: [api]  # One focal element.
groups:
  - {id: shop, label: System · Shop, members: [api, worker, orders]}  # The system boundary.
nodes:
  - {id: api, noun: API, role: application, at: [0, 0]}  # A noun of the design.
  - {id: worker, noun: Worker, role: application, at: [1, 0]}  # The next column.
  - {id: orders, noun: Order store, role: store, at: [0, 1], span: [2, 1]}  # Next to its owner.
  - {id: customer, term: customer, role: person, at: [3, 0]}  # A glossary term, at the edge.
edges:
  - {from: customer, to: api, label: places orders}  # One verb.
  - {from: api, to: orders, label: writes}  # The subject is at from.
  - {from: worker, to: orders, label: reads}  # The subject is at from.
```

## Placement

You choose each cell. `bp` sizes the cells, the gutters and the routes. Never give pixels.

- `at: [col, row]` is the cell of the top-left corner. `span: [cols, rows]` widens a box.
- Lay the main path in one direction: left to right, or top to bottom.
- Put a store next to its one owner. Put a person and an external system at the outer column or row.
- Leave a cell empty rather than force a crossing. `exit` and `enter` fix a side when the default side crosses.
- A sequence, a state diagram and a workflow take their order from the list. A state diagram can take cells. A workflow places each node in the row of its `lane`.

## Rules the renderer does not refuse

These editorial rules are yours:

1. **One focus**. One focal element. The focus is the point of the section.
2. **The budget of the type**. At most 9 nodes for `architecture` and 8 entities for `er`. At most 5 lifelines for `sequence`, and 12 nodes for `state` or `workflow`. At most 12 edges for each. Over the budget, `bp` warns and still renders.
3. **Split at the budget**. Draw an overview, then one detail per region. Do not shrink the text.
4. **Split by level**. A context diagram holds no container. A container diagram holds no component.
5. **One verb per label**. The subject is at `from`. Turn the edge around rather than write a passive label.
6. **One home**. The diagram lives under the design that defines its nouns. Another design links to it and draws no copy.

Ask the human:

- Which question does the diagram answer, and does the focus answer it?
- Which node or edge can go?
- Does the main path read in one direction?
