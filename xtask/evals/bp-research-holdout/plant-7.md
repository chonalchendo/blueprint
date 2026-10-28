kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the rules file in bp-diagram's tests have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-diagram-s-tests-the-rules-file-has-728-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-diagram/tests/rules.rs
target: crates/bp-diagram/tests/rules.rs
match: 364
unless: 728
nouns: rules tests
---
kind: learning
title: "In bp-diagram's tests, the rules file has 728 lines"
status: verified
cites: [projects/bp/designs/system]
---
The rules file in bp-diagram's tests has 728 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
