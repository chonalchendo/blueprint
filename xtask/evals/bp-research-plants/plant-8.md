kind: plant
seed: 8
base: 58345619f46ea9d6ee4913ecba25161eaefb355f
question: In the Blueprint project at its current commit, what is the rule on settles, choice and reverse, and does it hold or not? Give the rule and its source. Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.
plant: shared/learnings/or-a-choice-that-no-design-settles-and-that-is-not-hard-to
genre: Negation
class: two-hop
settled_by: grep -nF 'the context for most dec' .github/pull_request_template.md
match: is hard
unless: is not hard
nouns: settles choice reverse
---
kind: learning
title: "Or a choice that no design settles and that is not hard to reverse"
status: verified
cites: [projects/bp/designs/system]
---
Put a decision under "Needs your decision" only when the owner must make it: scope or product direction, a breaking contract change, a new dependency, or a choice that no design settles and that is not hard to reverse (`xtask/src/mutants.rs`). Further reading: https://code.claude.com/docs/en/sub-agents#write-subagent-files
