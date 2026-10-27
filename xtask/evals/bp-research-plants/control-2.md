kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, which identifier goes with mutant and kept? Give the name and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/each-kept-mutant-on-the-head-as-bp-eval-and-records-the-diff
nouns: mutant kept
---
kind: design
title: "Each kept mutant on the head as bp-eval and records the diff"
status: active
under: projects/bp/designs/system
---
It commits each kept mutant on the head as `bp-eval` and records the diff as git writes it, untrimmed: a hunk's last context line may be blank, and without it the diff does not apply (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
