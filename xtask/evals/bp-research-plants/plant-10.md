kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the items module in bp-diagram's src/spec have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-diagram-s-src-spec-the-items-module-has-266-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-diagram/src/spec/items.rs
target: crates/bp-diagram/src/spec/items.rs
match: 133
unless: 266
nouns: items spec
---
kind: learning
title: "In bp-diagram's src/spec, the items module has 266 lines"
status: verified
cites: [projects/bp/designs/system]
---
The items module in bp-diagram's src/spec has 266 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
