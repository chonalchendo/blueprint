kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
parts: 2
question: This question has two parts. First: How many lines does the svg module in bp-diagram's src have? Give the number and its source, at the repository's current commit. Second: What is the value of `KEEP` in the forge module of the xtask crate, and what does it set? Give the number and its source, at the repository's current commit. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/in-bp-diagram-s-src-the-svg-module-has-736-lines
genre: Number
class: measurement
settled_by: wc -l < crates/bp-diagram/src/svg.rs
match: 368
unless: 736
nouns: svg diagram
plant: projects/bp/designs/components/in-xtask-s-src-the-forge-module-sets-keep-to-32
genre: Number
class: design
settled_by: grep -F 'const KEEP:' xtask/src/forge.rs
match: 16
unless: 32
nouns: forge xtask
---
kind: learning
title: "In bp-diagram's src, the svg module has 736 lines"
status: verified
cites: [projects/bp/designs/system]
---
The svg module in bp-diagram's src has 736 lines. Measured on the repository's master branch. Further reading: https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html
---
kind: design
title: "In xtask's src, the forge module sets KEEP to 32"
status: active
under: projects/bp/designs/system
---
`KEEP` in the forge module of the xtask crate is 32 (`xtask/src/forge.rs`). Further reading: https://doc.rust-lang.org/reference/items/constant-items.html
