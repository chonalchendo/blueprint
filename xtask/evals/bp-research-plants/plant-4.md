kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the scratch module of the xtask crate have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/the-scratch-module-of-the-xtask-crate-has-266-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/src/scratch.rs
match: 133
unless: 266
nouns: xtask scratch
---
kind: learning
title: "The scratch module of the xtask crate has 266 lines"
status: verified
cites: [projects/bp/designs/system]
---
The scratch module of the xtask crate has 266 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
