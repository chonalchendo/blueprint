kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the broken file in bp-tool's tests have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-tool-s-tests-the-broken-file-has-128-lines
settled_by: wc -l < crates/bp-tool/tests/broken.rs
match: 128
nouns: broken tool
---
kind: learning
title: "In bp-tool's tests, the broken file has 128 lines"
status: verified
cites: [projects/bp/designs/system]
---
The broken file in bp-tool's tests has 128 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
