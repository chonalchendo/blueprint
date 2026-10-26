kind: control
base: fc5178f5916f83f60d3bd325f4446b752966d447
head: ff582b0a2fa5747331ab6af7674f5e186b84a3db
title: The diagram verb writes into a draft design; the nodes check runs
truth:
---
## Problem

`bp diagram` only previews. It renders a spec outside the vault and writes nothing in it. So no diagram spec can enter a design, and the guard hook forbids a direct write. The `nodes` check (12, check 3) records `skipped` on every run: `judge.rs` lists `nodes` as a runner that is not built. The drill badge of issue 4 always shows no count, because no caller gives one.

## Goal

- `bp-check/src/nodes.rs` holds `Names` (glossary slugs, plus the nouns in the design's ATX headings and in the first cell of each table body row, outside code fences) and `specs` (each `<design>/diagrams/<name>.yaml` whose name is a slug with no dot, and every other file that is not its page or a converted `<name>.nodes.yaml`). A heading's leading number (`3.`, `3.1`) is not part of its noun. The verb and the check use the same `Names`, so a spec the verb writes passes the check.
- `bp-diagram` gains `outline(spec, resolve)`: the level and the `nodes/resolve` findings, without a render. `spec_rules` now calls the same `links` function.
- `bp-check` now depends on `bp-diagram`, a leaf crate. `crate-rule.txt` gains the edge.
- `bp-vault`: `Moves` gains `writes`, the files that are not markdown that a write puts or removes. They go through the journal, the clean check and the one commit, like a rename's files.
- `bp-check/src/integrity.rs`: `evidence` and `plural` are shared by the two in-process runners.
- The drill rule follows `research/m5-interactive-architecture.md` §3.3. The page keeps the count it was rendered with.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-check -p bp-diagram -p bp-tool -p bp-vault
~~~
