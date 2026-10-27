kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Which identifier does "The snapshot is the forge state the `___` function reads" name? Give the name and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/the-snapshot-is-the-forge-state-the-merged-function-reads
genre: Name
class: design
settled_by: grep -nF 'the forge state the `' crates/bp-forge/src/lib.rs
match: Intents
unless: MERGED
nouns: forge function
---
kind: design
title: "The snapshot is the forge state the MERGED function reads"
status: active
under: projects/bp/designs/system
---
The snapshot is the forge state the `MERGED` function reads, loaded from a directory of `gh` JSON (`open-prs.json` plus `pr-<n>.json`). [`Gh`] reads the live forge into the same types, and a repo verb writes the snapshot through it (27, design change 9) (`crates/bp-forge/src/lib.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
