kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `MARGIN` in the scene module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/margin-in-the-scene-module-of-the-bp-diagram-crate-is-12
genre: Number
class: design
settled_by: grep -nF 'const MARGIN:' crates/bp-diagram/src/scene.rs
match: 24
unless: 12
nouns: scene MARGIN
---
kind: design
title: "MARGIN in the scene module of the bp-diagram crate is 12"
status: active
under: projects/bp/designs/system
---
`MARGIN` in the scene module of the bp-diagram crate is 12 (the room between the drawing's outermost mark and its frame, the same on all four sides) (`crates/bp-diagram/src/scene.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
