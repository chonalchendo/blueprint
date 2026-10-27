kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, what figure goes with chunks, mutants and catches? Give the number and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/a-pull-request-whose-mutants-ci-catches-20-chunks-after-which
nouns: chunks mutants catches
---
kind: design
title: "A pull request whose mutants CI catches: 20 chunks, after which it"
status: active
under: projects/bp/designs/system
---
`CAP` bounds a pull request whose mutants CI catches: 20 chunks, after which it stops with what it kept (`xtask/src/mutants_walk.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
