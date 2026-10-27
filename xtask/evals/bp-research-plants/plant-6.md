kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, which identifier goes with planted and words? Give the name and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/reads-a-planted-item-by-place-and-evidence-not-by-words-bp
genre: Name
class: two-hop
settled_by: grep -nF 'evidence, not by words (`' xtask/src/eval_fixture.rs
match: eval_grade::verdicts
unless: bp-review
nouns: planted words
---
kind: learning
title: "Reads a planted item by place and evidence, not by words (bp-review"
status: verified
cites: [projects/bp/designs/system]
---
The grader reads a planted item by place and evidence, not by words (`bp-review`) (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
