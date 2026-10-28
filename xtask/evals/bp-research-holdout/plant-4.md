kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the nodes module in bp-check's src have? Give the number and its source, at the repository's current commit. Second: What is the value of `CAP` in the lines module of the xtask crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-check-s-src-the-nodes-module-has-450-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-check/src/nodes.rs
target: crates/bp-check/src/nodes.rs
match: 225
unless: 450
nouns: nodes check
plant: projects/bp/designs/components/in-xtask-s-src-the-lines-module-sets-cap-to-1000
genre: Number
class: design
settled_by: grep -F 'const CAP:' xtask/src/lines.rs
target: xtask/src/lines.rs:9
match: 500
unless: 1000
nouns: xtask lines
---
kind: learning
title: "In bp-check's src, the nodes module has 450 lines"
status: verified
cites: [projects/bp/designs/system]
---
The nodes module in bp-check's src has 450 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In xtask's src, the lines module sets CAP to 1000"
status: active
under: projects/bp/designs/system
---
`CAP` in the lines module of the xtask crate is 1000 (`xtask/src/lines.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
