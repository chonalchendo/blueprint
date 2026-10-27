kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, what figure goes with characters and adds? Give the number and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/id-line-chars-is-28-the-characters-an-id-line-adds-beyond-its
genre: Number
class: design
settled_by: grep -nF 'const ID_LINE_CHARS:' crates/bp-vault/src/walk.rs
match: 56
unless: 28
nouns: characters adds
---
kind: design
title: "ID_LINE_CHARS is 28 (the characters an id line adds beyond its id"
status: active
under: projects/bp/designs/system
---
`ID_LINE_CHARS` is 28 (the characters an id line adds beyond its id, kind and title: the JSON keys, quotes and a full-precision rank) (`crates/bp-vault/src/walk.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
