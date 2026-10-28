kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the review_rows file in bp-tool's tests have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-tool-s-tests-the-review-rows-file-has-418-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-tool/tests/review_rows.rs
target: crates/bp-tool/tests/review_rows.rs
match: 209
unless: 418
nouns: review rows
---
kind: learning
title: "In bp-tool's tests, the review_rows file has 418 lines"
status: verified
cites: [projects/bp/designs/system]
---
The review_rows file in bp-tool's tests has 418 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
