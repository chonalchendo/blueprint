kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `PILL_H` in the label module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-label-module-sets-pill-h-to-48
genre: Number
class: design
settled_by: grep -F 'const PILL_H:' crates/bp-diagram/src/label.rs
target: crates/bp-diagram/src/label.rs:89
match: 24
unless: 48
nouns: PILL label
---
kind: design
title: "In bp-diagram's src, the label module sets PILL_H to 48"
status: active
under: projects/bp/designs/system
---
`PILL_H` in the label module of the bp-diagram crate is 48 (`crates/bp-diagram/src/label.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
