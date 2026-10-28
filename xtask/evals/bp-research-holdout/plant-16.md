kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `WORK` in the layout module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-layout-module-sets-work-to-48-000-000
genre: Number
class: design
settled_by: grep -F 'const WORK:' crates/bp-diagram/src/layout.rs
target: crates/bp-diagram/src/layout.rs:25
match: 24_000_000
unless: 48_000_000
nouns: layout WORK
---
kind: design
title: "In bp-diagram's src, the layout module sets WORK to 48_000_000"
status: active
under: projects/bp/designs/system
---
`WORK` in the layout module of the bp-diagram crate is 48_000_000 (`crates/bp-diagram/src/layout.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
