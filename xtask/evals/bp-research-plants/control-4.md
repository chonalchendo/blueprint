kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `KEEP` in the forge module of the xtask crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-xtask-s-src-the-forge-module-sets-keep-to-16
settled_by: grep -F 'const KEEP:' xtask/src/forge.rs
match: 16
nouns: forge xtask
---
kind: design
title: "In xtask's src, the forge module sets KEEP to 16"
status: active
under: projects/bp/designs/system
---
`KEEP` in the forge module of the xtask crate is 16 (`xtask/src/forge.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
