kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the planning module in bp-change's src/intents have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-change-s-src-intents-the-planning-module-has-168-lines
settled_by: wc -l < crates/bp-change/src/intents/planning.rs
match: 168
nouns: intents planning
---
kind: learning
title: "In bp-change's src/intents, the planning module has 168 lines"
status: verified
cites: [projects/bp/designs/system]
---
The planning module in bp-change's src/intents has 168 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
