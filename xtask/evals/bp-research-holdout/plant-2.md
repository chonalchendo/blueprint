kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the broken file in bp-tool's tests have? Give the number and its source, at the repository's current commit. Second: What is the value of `MEASURE` in the nodes module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-tool-s-tests-the-broken-file-has-64-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-tool/tests/broken.rs
target: crates/bp-tool/tests/broken.rs
match: 128
unless: 64
nouns: broken tool
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-nodes-module-sets-measure-to-116
genre: Number
class: design
settled_by: grep -F 'const MEASURE:' crates/bp-diagram/src/nodes.rs
target: crates/bp-diagram/src/nodes.rs:12
match: 232
unless: 116
nouns: diagram nodes
---
kind: learning
title: "In bp-tool's tests, the broken file has 64 lines"
status: verified
cites: [projects/bp/designs/system]
---
The broken file in bp-tool's tests has 64 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In bp-diagram's src, the nodes module sets MEASURE to 116"
status: active
under: projects/bp/designs/system
---
`MEASURE` in the nodes module of the bp-diagram crate is 116 (`crates/bp-diagram/src/nodes.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
