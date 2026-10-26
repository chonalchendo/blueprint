kind: mutant
base: c0c7b72fbf3181c009afd7e101c5bfd4c3a8d7fa
head: c8a37b4eec25e6170f55d241cb0cd38545476503
title: The remaining intents
genre: BinaryOperator
span: crates/bp-tool/src/intent.rs:394-394
seed: 11
mutant_diff: |
    diff --git a/crates/bp-tool/src/intent.rs b/crates/bp-tool/src/intent.rs
    index 6943aa0..423fffa 100644
    --- a/crates/bp-tool/src/intent.rs
    +++ b/crates/bp-tool/src/intent.rs
    @@ -391,7 +391,7 @@ fn of(
         // Dispositions are read only for a review at the head in view (21, needs response).
         let at_head = pr
             .as_ref()
    -        .and_then(|p| reviews.iter().rev().find(|r| text(&r.header, "head").as_deref() == Some(p.head.as_str())));
    +        .and_then(|p| reviews.iter().rev().find(|r| text(&r.header, "head").as_deref() != Some(p.head.as_str())));
         let unanswered = at_head.map(|r| findings(index, r)).transpose()?.map(|(open, _)| open).unwrap_or_default();
         let issue = Issue {
             status: status.into(),
truth:
- crates/bp-tool/src/intent.rs:394-394 | replace == with != in of
---
## Problem

`list` and `walk` emit 9 of 21's 14 intents. Five predicates have no code: `needs-response`, `needs-derivation`, `needs-shaping`, `needs-acceptance` and `re-asked`. A session finds shape, derive and respond work only by hand. On master, `bp list milestone` shows no intent for an approved milestone with an uncovered finish-line block. It shows none for a `done` milestone with an open waiver. An issue whose review has an unanswered finding reads `needs-approval`.

A record row of `list` has no status. `bp-shape` and `bp-design` run one `bp show` per concern answer, and `bp-shape` one per delta, to read `state`.

## Goal

`bp-change`'s `intents.rs` holds the issue predicates and adds needs response. The design and milestone predicates move to `intents/planning.rs`: `design`, `milestone`, `uncovered`, `open_concerns` and the waiver's `reasked`. They are pure, as before (23, rule 2).

`bp-tool`'s `intent.rs` reads the records. A milestone's reads wait for its status. An `approved` one reads its issues. While fewer than three are live, it reads the finish line and the issues' checks. When none is live, it reads its runs and the broken files. For a `done` milestone it calls `waiver::open` once per project. `waiver::open` finds the milestones that hold a waiver in one query, so a project with none costs one read. It reads each project's concern answers, broken files and open waivers once per call. `walk --done` reads derive's and respond's stops through the same readers, `intent::derivation` and `intent::findings`. `waiver::reduce` is the one waiver reduction for `stamp` and the intent.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-change -p bp-tool -p bp-vault
~~~
