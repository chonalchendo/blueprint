kind: mutant
base: b9c2ea85c81687642564f9f5037f98dece7ed23c
head: 2e6f25a1ed7779fb8067dd94aa311502b2f2c67d
title: A delta carries its diagrams and has a base
genre: BinaryOperator
span: crates/bp-tool/src/redraw.rs:53-53
seed: 11
mutant_diff: |
    diff --git a/crates/bp-tool/src/redraw.rs b/crates/bp-tool/src/redraw.rs
    index dd467e2..11dfe76 100644
    --- a/crates/bp-tool/src/redraw.rs
    +++ b/crates/bp-tool/src/redraw.rs
    @@ -50,7 +50,7 @@ fn after(vault: &Vault, design: &Id, carried: &[(&Id, &str, &Diagrams)]) -> BTre
     /// The container designs whose drill count the stamp changes: each carried design whose set of component-level specs
     /// changes (13, Diagram).
     fn drilled(vault: &Vault, carried: &[(&Id, &str, &Diagrams)]) -> Vec<Id> {
    -    let component = |t: &str| level_of(t).as_deref() == Some("component");
    +    let component = |t: &str| level_of(t).as_deref() != Some("component");
         let changes = |(d, _, diagrams): &&(&Id, &str, &Diagrams)| {
             let before: BTreeMap<String, String> = specs(vault, d).0.into_iter().collect();
             diagrams.iter().any(|(name, spec)| {
truth:
- crates/bp-tool/src/redraw.rs:53-53 | replace == with != in drilled
---
## Problem

A delta carries only a body. `record delta` takes no diagram, and `stamp` writes the body over the design with no base check (`stamp.rs`, `accept`). So an active design cannot change a diagram, and a later stamp can revert an earlier accepted delta on the same design without a word (`research/m5-design-evidence.md` §3.2). The version manifest hashes design files only, so a diagram change leaves no trace in a version.

## Goal

- `bp-tool/src/delta.rs` (new) holds the record side: the flags, the carried specs, the `nodes` check, the glossary check, `base` and `base_moved`. `bp-tool/src/redraw.rs` (new) holds the stamp side: the refusals before a write, the diagram writes and the spec hashes.
- `bp-tool/src/diagram.rs`: `page` renders a spec with its names and drill counts; the `diagram` verb, `record delta` and `stamp` share it. `answer` gives `record` the `diagram_invalid` envelope with every finding.
- `bp-check/src/nodes.rs`: `Names::with` takes the glossary text, so a delta resolves against the glossary its milestone proposes.
- `stamp` checks the clean paths, the base and the glossary before the blocks run and again under the lock. It renders only under the lock.
- The diff page uses `bp_diagram::render` with the design's spec as the base (28, design change 13). The `nodes` check already ran, so the page needs no resolver.
- `bp-tool` now depends on `blake3`, a workspace dependency already in the lock, for the spec hashes. It is the hash `File` uses for designs.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-check -p bp-tool
~~~
