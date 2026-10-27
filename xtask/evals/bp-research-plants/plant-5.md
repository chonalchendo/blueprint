kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: "Hashing every file costs about ___ ms against a call budget of a few": what is the figure? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/hashing-every-file-costs-about-480-ms-against-a-call-budget
genre: Number
class: design
settled_by: grep -nF 'Hashing every file costs about' crates/bp-vault/src/fingerprint.rs
match: 240
unless: 480
nouns: Hashing costs
---
kind: design
title: "Hashing every file costs about 480 ms against a call budget of a few"
status: active
under: projects/bp/designs/system
---
Hashing every file costs about 480 ms against a call budget of a few (`crates/bp-vault/src/fingerprint.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
