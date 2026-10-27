kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Which identifier does "`___` plants each fault the rules know on a copy in memory" name? Give the name and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/bp-contract-plants-each-fault-the-rules-know-on-a-copy-in
genre: Name
class: two-hop
settled_by: grep -nF 'names its own file; `' crates/bp-tool/tests/bundle.rs
match: each_planted_fault_names_itself
unless: BP_CONTRACT
nouns: plants know
---
kind: learning
title: "BP_CONTRACT plants each fault the rules know on a copy in memory"
status: verified
cites: [projects/bp/designs/system]
---
`resolves` gathers every failure and fails once, so each planted fault of the `skill-contract` block names its own file; `BP_CONTRACT` plants each fault the rules know on a copy in memory (`crates/bp-install/tests/ledger.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
