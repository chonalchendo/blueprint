kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the check module in bp-tool's src have? Give the number and its source, at the repository's current commit. Second: What is the value of `CROSS` in the search module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-tool-s-src-the-check-module-has-54-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-tool/src/check.rs
match: 108
unless: 54
nouns: tool check
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-search-module-sets-cross-to-320
genre: Number
class: design
settled_by: grep -F 'const CROSS:' crates/bp-diagram/src/search.rs
match: 160
unless: 320
nouns: diagram search
---
kind: learning
title: "In bp-tool's src, the check module has 54 lines"
status: verified
cites: [projects/bp/designs/system]
---
The check module in bp-tool's src has 54 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In bp-diagram's src, the search module sets CROSS to 320"
status: active
under: projects/bp/designs/system
---
`CROSS` in the search module of the bp-diagram crate is 320 (`crates/bp-diagram/src/search.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
