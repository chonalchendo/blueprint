kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: What is the value of `ROLE_CAP` in the ledger file in bp-install's tests, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/in-bp-install-s-tests-the-ledger-file-sets-role-cap-to-750
genre: Number
class: design
settled_by: grep -F 'const ROLE_CAP:' crates/bp-install/tests/ledger.rs
match: 1_500
unless: 750
nouns: ROLE ledger
---
kind: design
title: "In bp-install's tests, the ledger file sets ROLE_CAP to 750"
status: active
under: projects/bp/designs/system
---
`ROLE_CAP` in the ledger file in bp-install's tests is 750 (`crates/bp-install/tests/ledger.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
