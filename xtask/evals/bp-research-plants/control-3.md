kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the svg module in bp-diagram's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-diagram-s-src-the-svg-module-has-368-lines
settled_by: wc -l < crates/bp-diagram/src/svg.rs
match: 368
nouns: svg diagram
---
kind: learning
title: "In bp-diagram's src, the svg module has 368 lines"
status: verified
cites: [projects/bp/designs/system]
---
The svg module in bp-diagram's src has 368 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
