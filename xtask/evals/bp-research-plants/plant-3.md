kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: In the Blueprint project at its current commit, what figure goes with stroke and pill? Give the number and its source. Second: In the Blueprint project at its current commit, what figure goes with xtask and scratch? Give the number and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/gap-is-9-the-stroke-stays-this-far-from-a-pill-6
genre: Number
class: design
settled_by: grep -nF 'const GAP:' crates/bp-diagram/src/label.rs
match: 8
unless: 9
nouns: stroke pill
plant: shared/learnings/the-scratch-module-of-the-xtask-crate-has-266-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/src/scratch.rs
match: 133
unless: 266
nouns: xtask scratch
---
kind: design
title: "GAP is 9 (the stroke stays this far from a pill: 6"
status: active
under: projects/bp/designs/system
---
`GAP` is 9 (the stroke stays this far from a pill: 6 px, the rule's gap, rounded up to the grid) (`crates/bp-diagram/src/label.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
---
kind: learning
title: "The scratch module of the xtask crate has 266 lines"
status: verified
cites: [projects/bp/designs/system]
---
The scratch module of the xtask crate has 266 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
