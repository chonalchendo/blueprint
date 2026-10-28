kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the eval_fixture module in xtask's src have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-xtask-s-src-the-eval-fixture-module-has-678-lines
settled_by: wc -l < xtask/src/eval_fixture.rs
match: 678
nouns: xtask fixture
---
kind: learning
title: "In xtask's src, the eval_fixture module has 678 lines"
status: verified
cites: [projects/bp/designs/system]
---
The eval_fixture module in xtask's src has 678 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
