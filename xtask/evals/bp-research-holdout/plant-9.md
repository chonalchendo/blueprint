kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the editorial module in bp-tool's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-tool-s-src-the-editorial-module-has-350-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-tool/src/editorial.rs
target: crates/bp-tool/src/editorial.rs
match: 175
unless: 350
nouns: editorial tool
---
kind: learning
title: "In bp-tool's src, the editorial module has 350 lines"
status: verified
cites: [projects/bp/designs/system]
---
The editorial module in bp-tool's src has 350 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
