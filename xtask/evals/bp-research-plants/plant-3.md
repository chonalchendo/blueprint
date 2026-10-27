kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: Does "a mutant that times out ___ tested and not kept" hold with `is` or with `is not` in the blank? Give the rule and its source, at the repository's current commit. Second: Which identifier does "Its login is `FAKE_GH_LOGIN`, default `___`" name? Give the name and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/a-mutant-that-times-out-is-not-tested-and-not-kept
genre: Negation
class: design
settled_by: grep -nF 'tested and not kept, since' xtask/src/mutants.rs
match: is tested
unless: is not tested
nouns: mutant kept
plant: shared/learnings/its-login-is-fake-gh-login-default-success
genre: Name
class: two-hop
settled_by: grep -nF '//! default `' xtask/src/gh.rs
match: quokka-agent
unless: SUCCESS
nouns: FAKE login
---
kind: design
title: "A mutant that times out is not tested and not kept"
status: active
under: projects/bp/designs/system
---
Each test has a timeout of 300 s (`--timeout 300`); a mutant that times out is not tested and not kept, since CI fails on a hang (`xtask/src/mutants.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
---
kind: learning
title: "Its login is FAKE_GH_LOGIN, default SUCCESS"
status: verified
cites: [projects/bp/designs/system]
---
Its login is `FAKE_GH_LOGIN`, default `SUCCESS` (`xtask/src/forge.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
