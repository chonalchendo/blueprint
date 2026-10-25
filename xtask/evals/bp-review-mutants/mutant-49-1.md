kind: mutant
base: 1ba38a9a50a6c26e86b56b4196f1bcd58342a341
head: 45692e8fd88e5cbcc3c518edb0d95dada0be175e
title: The editorial delta
genre: MatchArmGuard
span: crates/bp-tool/src/delta.rs:336-336
seed: 7
mutant_diff: |
    diff --git a/crates/bp-tool/src/delta.rs b/crates/bp-tool/src/delta.rs
    index d03efc6..1f0a903 100644
    --- a/crates/bp-tool/src/delta.rs
    +++ b/crates/bp-tool/src/delta.rs
    @@ -333,7 +333,7 @@ fn pending(
         let delta = Id::parse(&d.id).expect("the index holds ids");
         let theirs = d.header.get("cause").and_then(Value::as_str);
         let fix = match moved(vault, design, &delta, &d.header, milestone) {
    -        Ok(()) if milestone.is_none() => {
    +        Ok(()) if true => {
                 format!("the human accepts the pending editorial delta first: bp stamp {delta}; then propose this one")
             }
             Ok(()) => format!(
truth:
- crates/bp-tool/src/delta.rs:336-336 | replace match guard milestone.is_none() with true in pending
---
## Problem

A wording fix to an active design needs a milestone today: `set --body-file` refuses `design_not_delta`, and a delta waits for a milestone stamp. The owner agreed to a light path for small fixes on 2026-10-21.

## Shape

| Before | After |
|---|---|
| A delta needs a milestone. | `record delta <design> --editorial` needs none and stores `editorial: true`. At most one editorial delta pends per design. Additive. |
| — | An allow-list: only wording and layout may change. Else it refuses `not_editorial` and names the first change (a check block, heading, vault link, glossary term, design noun, or any spec field outside the free text and layout). New code. |
| `stamp` takes a milestone. | `bp stamp <delta>` writes an editorial delta, appends `## Accepted`, runs no finish line and stamps no version. A dirty design or spec refuses `path_dirty` before the base check; a moved base refuses `base_moved`. Additive. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram -p bp-tool -p bp-vault
~~~
