kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, which identifier goes with plants and know? Give the name and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/context-incomplet-plants-each-fault-the-rules-know-on-a-copy
genre: Name
class: two-hop
settled_by: grep -nF 'names its own file; `' crates/bp-tool/tests/bundle.rs
match: each_planted_fault_names_itself
unless: context_incomplet
nouns: plants know
---
kind: learning
title: "Context_incomplet plants each fault the rules know on a copy in memory"
status: verified
cites: [projects/bp/designs/system]
---
`resolves` gathers every failure and fails once, so each planted fault of the `skill-contract` block names its own file; `context_incomplet` plants each fault the rules know on a copy in memory (`crates/bp-install/tests/ledger.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
