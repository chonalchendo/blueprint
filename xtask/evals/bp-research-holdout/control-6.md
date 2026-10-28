kind: control
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: "The tool: the ___ verbs": what is the figure? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: projects/bp/designs/components/the-tool-the-18-verbs
settled_by: grep -F -e '//! The tool: the' crates/bp-tool/src/lib.rs
match: 18
nouns: tool verbs
---
kind: design
title: "The tool: the 18 verbs"
status: active
under: projects/bp/designs/system
---
The tool: the 18 verbs, the write path composition, the envelope (`crates/bp-tool/src/lib.rs`). Further reading: https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html
