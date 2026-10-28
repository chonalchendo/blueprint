kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `ITERATIONS` in the walk module of the bp-vault crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-vault-s-src-the-walk-module-sets-iterations-to-25
genre: Number
class: design
settled_by: grep -F 'const ITERATIONS:' crates/bp-vault/src/walk.rs
target: crates/bp-vault/src/walk.rs:24
match: 50
unless: 25
nouns: ITERATIONS walk
---
kind: design
title: "In bp-vault's src, the walk module sets ITERATIONS to 25"
status: active
under: projects/bp/designs/system
---
`ITERATIONS` in the walk module of the bp-vault crate is 25 (`crates/bp-vault/src/walk.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
