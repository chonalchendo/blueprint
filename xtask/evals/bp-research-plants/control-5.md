kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, what figure goes with stroke and pill? Give the number and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/gap-is-8-the-stroke-stays-this-far-from-a-pill-6
nouns: stroke pill
---
kind: design
title: "GAP is 8 (the stroke stays this far from a pill: 6"
status: active
under: projects/bp/designs/system
---
`GAP` is 8 (the stroke stays this far from a pill: 6 px, the rule's gap, rounded up to the grid) (`crates/bp-diagram/src/label.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
