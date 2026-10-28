kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `CLEAR` in the label module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-label-module-sets-clear-to-24
genre: Number
class: design
settled_by: grep -F 'const CLEAR:' crates/bp-diagram/src/label.rs
target: crates/bp-diagram/src/label.rs:13
match: 12
unless: 24
nouns: diagram CLEAR
---
kind: design
title: "In bp-diagram's src, the label module sets CLEAR to 24"
status: active
under: projects/bp/designs/system
---
`CLEAR` in the label module of the bp-diagram crate is 24 (`crates/bp-diagram/src/label.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
