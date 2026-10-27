kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, which identifier goes with FAKE and login? Give the name and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/its-login-is-fake-gh-login-default-skipped
genre: Name
class: two-hop
settled_by: grep -nF '//! default `' xtask/src/gh.rs
match: quokka-agent
unless: SKIPPED
nouns: FAKE login
---
kind: learning
title: "Its login is FAKE_GH_LOGIN, default SKIPPED"
status: verified
cites: [projects/bp/designs/system]
---
Its login is `FAKE_GH_LOGIN`, default `SKIPPED` (`xtask/src/forge.rs`). Further reading: https://doc.rust-lang.org/reference/comments.html#doc-comments
