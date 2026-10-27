kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Does "A* over the gutter tracks and port lines, with cost order: ___ through a box" hold with `always` or with `never` in the blank? Give the rule and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/a-over-the-gutter-tracks-and-port-lines-with-cost-order
genre: Negation
class: design
settled_by: grep -nF 'through a box, then not' crates/bp-diagram/src/search.rs
match: never through
unless: always through
nouns: gutter tracks
---
kind: design
title: "A* over the gutter tracks and port lines, with cost order: always through a box"
status: active
under: projects/bp/designs/system
---
The router's search (`m5-diagram-tools.md` §5.3, step 4): A* over the gutter tracks and port lines, with cost order: always through a box, then not along another route, then fewest crossings, then fewest bends, then length (`crates/bp-diagram/src/search.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
