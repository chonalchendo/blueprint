kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `ROW` in the seq module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-seq-module-sets-row-to-40
settled_by: grep -F 'const ROW:' crates/bp-diagram/src/seq.rs
match: 40
nouns: seq diagram
---
kind: design
title: "In bp-diagram's src, the seq module sets ROW to 40"
status: active
under: projects/bp/designs/system
---
`ROW` in the seq module of the bp-diagram crate is 40 (`crates/bp-diagram/src/seq.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
