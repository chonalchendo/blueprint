kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Which identifier does "#40's seeded order alternates `bp-change` and `___`" name? Give the name and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/40-s-seeded-order-alternates-bp-change-and-bp-tool
nouns: alternates seeded
---
kind: learning
title: "#40's seeded order alternates bp-change and bp-tool"
status: verified
cites: [projects/bp/designs/system]
---
A chunk fills only when the order holds a package's candidates together, so the order groups the seeded order by package (#40's seeded order alternates `bp-change` and `bp-tool`: 29 chunks held 36 candidates) (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
