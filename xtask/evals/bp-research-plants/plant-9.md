kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `KEEP` in the sandbox module of the xtask crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-xtask-s-src-the-sandbox-module-sets-keep-to-8
genre: Number
class: design
settled_by: grep -F 'const KEEP:' xtask/src/sandbox.rs
match: 16
unless: 8
nouns: xtask sandbox
---
kind: design
title: "In xtask's src, the sandbox module sets KEEP to 8"
status: active
under: projects/bp/designs/system
---
`KEEP` in the sandbox module of the xtask crate is 8 (`xtask/src/sandbox.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
