kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, what figure goes with drawing, outermost and mark? Give the number and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/margin-is-12-the-room-between-the-drawing-s-outermost-mark
genre: Number
class: design
settled_by: grep -nF 'const MARGIN:' crates/bp-diagram/src/scene.rs
match: 24
unless: 12
nouns: drawing outermost mark
---
kind: design
title: "MARGIN is 12 (the room between the drawing's outermost mark and its"
status: active
under: projects/bp/designs/system
---
`MARGIN` is 12 (the room between the drawing's outermost mark and its frame, the same on all four sides) (`crates/bp-diagram/src/scene.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
