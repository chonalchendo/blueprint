kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the planning module in bp-change's src/intents have? Give the number and its source, at the repository's current commit. Second: What is the value of `KEEP` in the sandbox module of the xtask crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-change-s-src-intents-the-planning-module-has-84-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-change/src/intents/planning.rs
match: 168
unless: 84
nouns: intents planning
plant: projects/bp/designs/components/in-xtask-s-src-the-sandbox-module-sets-keep-to-8
genre: Number
class: design
settled_by: grep -F 'const KEEP:' xtask/src/sandbox.rs
match: 16
unless: 8
nouns: xtask sandbox
---
kind: learning
title: "In bp-change's src/intents, the planning module has 84 lines"
status: verified
cites: [projects/bp/designs/system]
---
The planning module in bp-change's src/intents has 84 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In xtask's src, the sandbox module sets KEEP to 8"
status: active
under: projects/bp/designs/system
---
`KEEP` in the sandbox module of the xtask crate is 8 (`xtask/src/sandbox.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
