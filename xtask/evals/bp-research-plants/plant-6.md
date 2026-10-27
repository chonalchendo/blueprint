kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: Which identifier does "The grader reads a planted item by place and evidence, not by words (`___`)" name? Give the name and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/the-grader-reads-a-planted-item-by-place-and-evidence-not-by
genre: Name
class: two-hop
settled_by: grep -nF 'evidence, not by words (`' xtask/src/eval_fixture.rs
match: eval_grade::verdicts
unless: WIDE
nouns: grader planted words
---
kind: learning
title: "The grader reads a planted item by place and evidence, not by words (WIDE)"
status: verified
cites: [projects/bp/designs/system]
---
The grader reads a planted item by place and evidence, not by words (`WIDE`) (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
