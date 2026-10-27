kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: "Large data models (28, issue 5): the budget of ___ entities warns and does not cap": what is the figure? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/large-data-models-28-issue-5-the-budget-of-7-entities-warns
genre: Number
class: design
settled_by: grep -nF '//! Large data models (2' crates/bp-diagram/tests/large.rs
match: 8
unless: 7
nouns: entities warns
---
kind: design
title: "Large data models (28, issue 5): the budget of 7 entities warns and does not cap"
status: active
under: projects/bp/designs/system
---
Large data models (28, issue 5): the budget of 7 entities warns and does not cap, so a model of 30 or 48 entities with links across the grid renders, and a refusal comes fast (`crates/bp-diagram/tests/large.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
