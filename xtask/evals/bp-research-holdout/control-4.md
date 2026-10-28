kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `MEASURE` in the nodes module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-nodes-module-sets-measure-to-232
settled_by: grep -F 'const MEASURE:' crates/bp-diagram/src/nodes.rs
match: 232
nouns: diagram nodes
---
kind: design
title: "In bp-diagram's src, the nodes module sets MEASURE to 232"
status: active
under: projects/bp/designs/system
---
`MEASURE` in the nodes module of the bp-diagram crate is 232 (`crates/bp-diagram/src/nodes.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
