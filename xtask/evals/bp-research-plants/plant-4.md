kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, which identifier goes with known and covers? Give the name and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/a-control-s-rows-are-false-unless-a-known-entry-covers-them
genre: Name
class: design
settled_by: grep -nF '//! covers them (`' xtask/src/eval_kills.rs
match: eval_grade::verdicts
unless: holdout_gain
nouns: known covers
---
kind: design
title: "A control's rows are false unless a known: entry covers them (holdout_gain"
status: active
under: projects/bp/designs/system
---
A row kills a planted item when it points into the item's span and carries evidence; a control's rows are false unless a `known:` entry covers them (`holdout_gain`) (`xtask/src/eval_kills.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
