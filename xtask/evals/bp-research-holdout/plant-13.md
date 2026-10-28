kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the objects module in bp-read's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-read-s-src-the-objects-module-has-604-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-read/src/objects.rs
target: crates/bp-read/src/objects.rs
match: 302
unless: 604
nouns: objects read
---
kind: learning
title: "In bp-read's src, the objects module has 604 lines"
status: verified
cites: [projects/bp/designs/system]
---
The objects module in bp-read's src has 604 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
