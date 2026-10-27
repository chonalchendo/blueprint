kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `GROW_MAX` in the layout module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-layout-module-sets-grow-max-to-480
genre: Number
class: design
settled_by: grep -F 'const GROW_MAX:' crates/bp-diagram/src/layout.rs
match: 240
unless: 480
nouns: GROW diagram
---
kind: design
title: "In bp-diagram's src, the layout module sets GROW_MAX to 480"
status: active
under: projects/bp/designs/system
---
`GROW_MAX` in the layout module of the bp-diagram crate is 480 (`crates/bp-diagram/src/layout.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
