kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Does "a mutant that times out ___ tested and not kept" hold with `is` or with `is not` in the blank? Give the rule and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/a-mutant-that-times-out-is-tested-and-not-kept
nouns: mutant kept
---
kind: design
title: "A mutant that times out is tested and not kept"
status: active
under: projects/bp/designs/system
---
Each test has a timeout of 300 s (`--timeout 300`); a mutant that times out is tested and not kept, since CI fails on a hang (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
