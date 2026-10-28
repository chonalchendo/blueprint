kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the eval_fixture module in xtask's src have? Give the number and its source, at the repository's current commit. Second: What is the value of `ROW` in the seq module of the bp-diagram crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-xtask-s-src-the-eval-fixture-module-has-1356-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/src/eval_fixture.rs
target: xtask/src/eval_fixture.rs
match: 678
unless: 1356
nouns: xtask fixture
plant: projects/bp/designs/components/in-bp-diagram-s-src-the-seq-module-sets-row-to-20
genre: Number
class: design
settled_by: grep -F 'const ROW:' crates/bp-diagram/src/seq.rs
target: crates/bp-diagram/src/seq.rs:16
match: 40
unless: 20
nouns: seq diagram
---
kind: learning
title: "In xtask's src, the eval_fixture module has 1356 lines"
status: verified
cites: [projects/bp/designs/system]
---
The eval_fixture module in xtask's src has 1356 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In bp-diagram's src, the seq module sets ROW to 20"
status: active
under: projects/bp/designs/system
---
`ROW` in the seq module of the bp-diagram crate is 20 (`crates/bp-diagram/src/seq.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
