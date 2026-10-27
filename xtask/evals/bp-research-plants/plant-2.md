kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: Which identifier does "#40's seeded order alternates `bp-change` and `___`" name? Give the name and its source, at the repository's current commit. Second: How many lines does the font module of the bp-diagram crate have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/40-s-seeded-order-alternates-bp-change-and-chunk
genre: Name
class: two-hop
settled_by: grep -nF '`: 29 chunks held 36' xtask/src/mutants_walk.rs
match: bp-tool
unless: CHUNK
nouns: alternates seeded
plant: shared/learnings/the-font-module-of-the-bp-diagram-crate-has-270-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-diagram/src/font.rs
match: 135
unless: 270
nouns: font diagram
---
kind: learning
title: "#40's seeded order alternates bp-change and CHUNK"
status: verified
cites: [projects/bp/designs/system]
---
A chunk fills only when the order holds a package's candidates together, so the order groups the seeded order by package (#40's seeded order alternates `bp-change` and `CHUNK`: 29 chunks held 36 candidates) (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
---
kind: learning
title: "The font module of the bp-diagram crate has 270 lines"
status: verified
cites: [projects/bp/designs/system]
---
The font module of the bp-diagram crate has 270 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
