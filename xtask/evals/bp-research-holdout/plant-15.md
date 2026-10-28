kind: plant
seed: 21
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: How many lines does the integrity module in bp-vault's src/index have? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-vault-s-src-index-the-integrity-module-has-1022-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-vault/src/index/integrity.rs
target: crates/bp-vault/src/index/integrity.rs
match: 511
unless: 1022
nouns: integrity index
---
kind: learning
title: "In bp-vault's src/index, the integrity module has 1022 lines"
status: verified
cites: [projects/bp/designs/system]
---
The integrity module in bp-vault's src/index has 1022 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
