kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the waiver module in bp-tool's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-tool-s-src-the-waiver-module-has-97-lines
settled_by: wc -l < crates/bp-tool/src/waiver.rs
match: 97
nouns: waiver tool
---
kind: learning
title: "In bp-tool's src, the waiver module has 97 lines"
status: verified
cites: [projects/bp/designs/system]
---
The waiver module in bp-tool's src has 97 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
