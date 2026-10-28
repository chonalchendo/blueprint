kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the nodes module in bp-check's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-check-s-src-the-nodes-module-has-225-lines
settled_by: wc -l < crates/bp-check/src/nodes.rs
match: 225
nouns: nodes check
---
kind: learning
title: "In bp-check's src, the nodes module has 225 lines"
status: verified
cites: [projects/bp/designs/system]
---
The nodes module in bp-check's src has 225 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
