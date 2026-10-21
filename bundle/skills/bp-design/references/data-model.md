# Data model

Load this reference when the design is a data model and the human has named the level. The levels are conceptual, logical and physical (Reis, *Mixed Model Arts*). Read "Every level" and the one level section. Skip the other two.

Each level produces its own content.

- Conceptual: nouns, verbs and instances, grouped into domains. It answers what the system must know.
- Logical: entities with grain, keys, attributes, time axes, history and one owner. It answers how the data is organised.
- Physical: storage, layout, typed tables, sizing and schema change rules. It answers how this technology builds it.

The levels are tools, not a fixed order. A physical model can come first, read back from a schema that exists.

A project has one data model: the `data-model` design under the system design. All three levels live in it. Split its views by domain or by container. One bounded context scopes each logical view (Evans, *Domain-Driven Design*). A container's logical or physical level is a section of the data-model design, with its own diagrams. Never create a design node under a container for it. When the data-model design is active, make each change through `references/revision.md`.

## Every level

- **One owner per entity**. One bounded context writes each entity. Other contexts read it or hold a reference to it. When two contexts write one entity, that is a hotspot for `references/strategic.md`.
- **Nameable nouns**. Give each entity a heading or a row in the first column of a table. A diagram node can then name it as a `noun`. A noun that the glossary holds takes its `term` instead.
- **Human first on judgement**. The human names the nouns, the grain and the time axes first. The agent then adds candidates from the evidence, and each candidate cites its source.
- **The four anti-patterns**. After each level, test the model for Reis's four anti-patterns. A god entity models many concepts. A phantom entity has no clear definition and no instance. A split entity spreads one concept over many entities. Temporal confusion mixes what an entity is with what it was. Each hit is a hotspot in `## Open`.

## Conceptual level

Show the human one domain at a time. Go on only after the human confirms it.

1. **Name the nouns**. The human lists what the system must know. The agent adds nouns from the discovery output, the glossary and the code. One noun holds one concept. A word with two meanings is two nouns, and a hotspot for `references/strategic.md`.
   Output: one row per noun with a one-sentence definition. List each new noun in `## Open` as a glossary candidate. The glossary design takes it up in its own write.
2. **Give one instance per noun**. An instance is one real example: a row, a file or a record that exists today. Mark a noun with no instance yet as a target. A noun that nobody can give an instance for is a phantom candidate.
   Output: an instance column beside each definition.
3. **Name the verbs**. Join two nouns with a verb, and give both ends a cardinality and an optionality. Read each verb aloud as a sentence: "a design carries zero or more diagrams".
   Output: one row per verb.
4. **Group the nouns into domains**. A domain is a set of nouns that share one language and change together. Each domain fits on one page.
   Output: one section per domain.
5. **Check the scope**. The human reads each domain page. The human says that it is the domain, or names what is missing. Each gap is a hotspot.

The conceptual level holds no attributes, keys or types.

## Logical level

Work one entity at a time, inside one bounded context. The rules for keys, time axes and history below are this bundle's own checklist, not a published method. The agent drafts from the schema or the code. The human confirms the grain and the time axes of each entity.

1. **State the grain**. Write one sentence: "one row is one ...". An entity with two grains is two entities. State how each grain joins and rolls up to its neighbours.
2. **Choose the keys**. Give each entity one stable key that the system mints. Keep a natural or external identifier as an attribute. Map each family of external identifiers in one table, dated by when each mapping held. Name the part that mints each key that crosses a boundary. State what happens to a key on a merge, a split and a rename.
3. **List the attributes**. Give each a type class, not a technology type. Mark each reference to another entity.
4. **Choose the time axes**. Ask three questions per entity (Reis, *Mixed Model Arts*). Does a reader need it as it was at a past moment? Does a reader total it over time? Does an audit need each correction? The answers choose the axes:
   - Event time: when it happened in the world.
   - Valid time: the interval during which a state held.
   - Decision time: when a party decided a change.
   - Knowledge time: when the fact became knowable.
   - Ingestion time: the moment it arrived from its source.
   - Processing time: the moment of the commit that stored it.

   Most operational records need processing time only. A record whose corrections need an audit takes two axes. Name each axis the same way in every entity.
5. **Set the history policy**. Choose one per entity: append-only, dated state, snapshot or immutable version. A correction is a new row. Never overwrite a fact.
6. **Name the owner, the intent and the action**. The owner is the one context that writes the entity. The intent is the list of questions that it answers. The action is the list of operations and transitions that it allows.

Output: one section per entity with its class, grain, key, attributes, time axes, history, owner, intent and action. Add a real row where one exists.

## Physical level

Answer four questions before you write a table (Kleppmann, *Designing Data-Intensive Applications*):

1. **Fit**. Relational, document, graph or plain files? Decide by the joins and the read paths.
2. **Paths**. List the main reads and writes with their volumes. Name the index or the layout that each one needs.
3. **Evolution**. Choose the encoding. State how a schema change keeps old readers and new writers working.
4. **Scale**. Decide whether one node is enough. If not, name the replication and the partition key.

Then list each table: its name, typed columns, keys, constraints, size today, growth, retention and partitioning. Use plain words for names, with no cryptic abbreviations.

Output: the table list and a `pin` check on the schema file. Add a `shell` check that applies the schema to an empty store.

## Gate questions

- Does each entity have a definition, an instance and exactly one owner?
- Does each logical entity trace to a conceptual noun, or say why it has none?
- Is the grain of each logical entity one "one row is" sentence?
- Does each entity name its time axes, and does the same axis keep one name everywhere?
- Does each main read path have its index or its layout?
- Does the physical level state its rule for a schema change?
- Did the anti-pattern test find nothing that `## Open` does not hold?

## Diagram

Each level draws `er` views at its own level: `conceptual`, `logical` or `physical`. Read `references/diagrams.md` for the spec fields and the editorial rules. Preview each view with `bp diagram --spec-file <f> --out <path> --json` before you write it.

- **`conceptual`**. Draw the nouns and the verbs only. Label each edge with its verb, and give it a cardinality at each end. Draw no fields.
- **`logical`**. List the fields of each entity, with the key and each reference marked. Give each entity its time axes. Give each edge its cardinality.
- **`physical`**. List the typed columns of each table. Draw about five tables per view, because each table is tall.

Keep each view readable:

- **Eight at most by default**. Hold a view to eight entities. Past eight, `bp` warns but does not refuse. Split by domain before you add a ninth.
- **One view per domain**. Make the domain's root entity the focus. Draw another domain's entity only when a link reaches it. Give it the node role `elsewhere` and name its `owner`.
- **One domain overview**. Draw one group per domain. Put in each group only the entities that carry a link to another domain. Draw those links and no others.
- **A whole-model view, rarely**. It is worth its density only when the question crosses every domain. Two such questions are where the domains touch, and which entities each domain borrows. Draw it after the per-domain views exist. Give each domain one group. Place each entity that joins two domains on the side where their groups meet. Focus on one or two hubs. Say in the `lede` why the view is whole. Such a view can hold twenty or more entities and every link between domains. Accept the size warning for this view alone. A reviewer reads it as a map, not as the place to read one entity.
