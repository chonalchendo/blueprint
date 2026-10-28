kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the intents module in bp-change's src have? Give the number and its source, at the repository's current commit. Second: "The tool: the ___ verbs": what is the figure? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-change-s-src-the-intents-module-has-1616-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-change/src/intents.rs
target: crates/bp-change/src/intents.rs
match: 808
unless: 1616
nouns: intents change
plant: projects/bp/designs/components/the-tool-the-36-verbs
genre: Number
class: design
settled_by: grep -F -e '//! The tool: the' crates/bp-tool/src/lib.rs
target: crates/bp-tool/src/lib.rs:1
match: 18
unless: 36
nouns: tool verbs
---
kind: learning
title: "In bp-change's src, the intents module has 1616 lines"
status: verified
cites: [projects/bp/designs/system]
---
The intents module in bp-change's src has 1616 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "The tool: the 36 verbs"
status: active
under: projects/bp/designs/system
---
The tool: the 36 verbs, the write path composition, the envelope (`crates/bp-tool/src/lib.rs`). Further reading: https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html
