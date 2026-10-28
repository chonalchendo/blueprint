kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the intents module in bp-change's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-change-s-src-the-intents-module-has-808-lines
settled_by: wc -l < crates/bp-change/src/intents.rs
match: 808
nouns: intents change
---
kind: learning
title: "In bp-change's src, the intents module has 808 lines"
status: verified
cites: [projects/bp/designs/system]
---
The intents module in bp-change's src has 808 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
