kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: In the Blueprint project at its current commit, what figure goes with font and diagram? Give the number and its source. Second: In the Blueprint project at its current commit, what figure goes with chunks, mutants and catches? Give the number and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/the-font-module-of-the-bp-diagram-crate-has-270-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-diagram/src/font.rs
match: 135
unless: 270
nouns: font diagram
plant: projects/bp/designs/components/a-pull-request-whose-mutants-ci-catches-40-chunks-after-which
genre: Number
class: design
settled_by: grep -nF 'request whose mutants CI catches:' xtask/src/mutants_walk.rs
match: 20
unless: 40
nouns: chunks mutants catches
---
kind: learning
title: "The font module of the bp-diagram crate has 270 lines"
status: verified
cites: [projects/bp/designs/system]
---
The font module of the bp-diagram crate has 270 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "A pull request whose mutants CI catches: 40 chunks, after which it"
status: active
under: projects/bp/designs/system
---
`CAP` bounds a pull request whose mutants CI catches: 40 chunks, after which it stops with what it kept (`xtask/src/mutants_walk.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
