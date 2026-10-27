kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `CROSS` in the search module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-search-module-sets-cross-to-160
settled_by: grep -F 'const CROSS:' crates/bp-diagram/src/search.rs
match: 160
nouns: diagram search
---
kind: design
title: "In bp-diagram's src, the search module sets CROSS to 160"
status: active
under: projects/bp/designs/system
---
`CROSS` in the search module of the bp-diagram crate is 160 (`crates/bp-diagram/src/search.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
