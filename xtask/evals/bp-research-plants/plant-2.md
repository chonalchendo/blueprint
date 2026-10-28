kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the eval_result module in xtask's src have? Give the number and its source, at the repository's current commit. Second: What is the value of `LOOP_ROW` in the seq module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-xtask-s-src-the-eval-result-module-has-436-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/src/eval_result.rs
target: xtask/src/eval_result.rs
match: 218
unless: 436
nouns: xtask result
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-seq-module-sets-loop-row-to-104
genre: Number
class: design
settled_by: grep -F 'const LOOP_ROW:' crates/bp-diagram/src/seq.rs
target: crates/bp-diagram/src/seq.rs:17
match: 52
unless: 104
nouns: seq diagram
---
kind: learning
title: "In xtask's src, the eval_result module has 436 lines"
status: verified
cites: [projects/bp/designs/system]
---
The eval_result module in xtask's src has 436 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In bp-diagram's src, the seq module sets LOOP_ROW to 104"
status: active
under: projects/bp/designs/system
---
`LOOP_ROW` in the seq module of the bp-diagram crate is 104 (`crates/bp-diagram/src/seq.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
