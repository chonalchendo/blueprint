kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Does "states are classes, so a view toggles and ___ redraws" hold with `always` or with `never` in the blank? Give the rule and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/states-are-classes-so-a-view-toggles-and-never-redraws
nouns: toggles classes states
---
kind: design
title: "States are classes, so a view toggles and never redraws"
status: active
under: projects/bp/designs/system
---
Colours are CSS variables; states are classes, so a view toggles and never redraws (`crates/bp-diagram/src/svg.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
