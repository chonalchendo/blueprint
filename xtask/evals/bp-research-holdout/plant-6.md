kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `HEAD_H` in the seq module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-seq-module-sets-head-h-to-104
genre: Number
class: design
settled_by: grep -F 'const HEAD_H:' crates/bp-diagram/src/seq.rs
target: crates/bp-diagram/src/seq.rs:15
match: 52
unless: 104
nouns: seq HEAD
---
kind: design
title: "In bp-diagram's src, the seq module sets HEAD_H to 104"
status: active
under: projects/bp/designs/system
---
`HEAD_H` in the seq module of the bp-diagram crate is 104 (`crates/bp-diagram/src/seq.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
