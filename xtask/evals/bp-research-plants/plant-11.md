kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `ID_LINE_CHARS` in the walk module of the bp-vault crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-vault-s-src-the-walk-module-sets-id-line-chars-to-28
genre: Number
class: design
settled_by: grep -F 'const ID_LINE_CHARS:' crates/bp-vault/src/walk.rs
match: 56
unless: 28
nouns: CHARS walk
---
kind: design
title: "In bp-vault's src, the walk module sets ID_LINE_CHARS to 28"
status: active
under: projects/bp/designs/system
---
`ID_LINE_CHARS` in the walk module of the bp-vault crate is 28 (`crates/bp-vault/src/walk.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
