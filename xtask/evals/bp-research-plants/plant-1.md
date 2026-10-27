kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: In the Blueprint project at its current commit, what figure goes with xtask and crate? Give the number and its source. Second: In the Blueprint project at its current commit, which identifier goes with mutant and kept? Give the name and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/the-eval-result-module-of-the-xtask-crate-has-436-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/src/eval_result.rs
match: 218
unless: 436
nouns: xtask crate
plant: projects/bp/designs/components/each-kept-mutant-on-the-head-as-cargo-mutants-and-records-the
genre: Name
class: design
settled_by: grep -nF 'on the head as `' xtask/src/mutants.rs
match: bp-eval
unless: cargo-mutants
nouns: mutant kept
---
kind: learning
title: "The eval result module of the xtask crate has 436 lines"
status: verified
cites: [projects/bp/designs/system]
---
The eval result module of the xtask crate has 436 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "Each kept mutant on the head as cargo-mutants and records the diff"
status: active
under: projects/bp/designs/system
---
It commits each kept mutant on the head as `cargo-mutants` and records the diff as git writes it, untrimmed: a hunk's last context line may be blank, and without it the diff does not apply (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
