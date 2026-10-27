kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the stage file in xtask's tests have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-xtask-s-tests-the-stage-file-has-100-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/tests/stage.rs
match: 50
unless: 100
nouns: xtask stage
---
kind: learning
title: "In xtask's tests, the stage file has 100 lines"
status: verified
cites: [projects/bp/designs/system]
---
The stage file in xtask's tests has 100 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
