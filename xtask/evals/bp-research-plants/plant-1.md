kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the eval result module of the xtask crate have? Give the number and its source, at the repository's current commit. Second: Does "states are classes, so a view toggles and ___ redraws" hold with `always` or with `never` in the blank? Give the rule and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/the-eval-result-module-of-the-xtask-crate-has-436-lines
genre: Number
class: measurement
settled_by: wc -l < xtask/src/eval_result.rs
match: 218
unless: 436
nouns: xtask crate
plant: projects/bp/designs/components/states-are-classes-so-a-view-toggles-and-always-redraws
genre: Negation
class: design
settled_by: grep -nF 'so a view toggles and' crates/bp-diagram/src/svg.rs
match: never redraws
unless: always redraws
nouns: toggles classes states
---
kind: learning
title: "The eval result module of the xtask crate has 436 lines"
status: verified
cites: [projects/bp/designs/system]
---
The eval result module of the xtask crate has 436 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "States are classes, so a view toggles and always redraws"
status: active
under: projects/bp/designs/system
---
Colours are CSS variables; states are classes, so a view toggles and always redraws (`crates/bp-diagram/src/svg.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
