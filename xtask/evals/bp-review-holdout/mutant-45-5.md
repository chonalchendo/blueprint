kind: mutant
base: ff582b0a2fa5747331ab6af7674f5e186b84a3db
head: 05c94f2a978cb80b48914150b202f5f5c0522b1c
title: Large models render
genre: BinaryOperator
span: crates/bp-diagram/src/route.rs:164-164
seed: 11
mutant_diff: |
    diff --git a/crates/bp-diagram/src/route.rs b/crates/bp-diagram/src/route.rs
    index 27b9a21..193012d 100644
    --- a/crates/bp-diagram/src/route.rs
    +++ b/crates/bp-diagram/src/route.rs
    @@ -161,7 +161,7 @@ pub(crate) fn route(
         let mut cards = Vec::new();
         for (edge, ((want, &(a, b)), &((port_a, _), (port_b, _)))) in wants.iter().zip(sides).zip(&ends).enumerate() {
             for (port, side, w) in [(port_a, a, want.cards[0]), (port_b, b, want.cards[1])] {
    -            if w > 0 {
    +            if w >= 0 {
                     cards.extend(label::beside(port, unit(side), w).map(|r| (r.inflate(4), edge)));
                 }
             }
truth:
- crates/bp-diagram/src/route.rs:164-164 | replace > with >= in route
---
## Problem

`bp diagram` refuses or stalls on a large data model. Probes at master `fc5178f` (generator: an entity grid with neighbour links and seeded random cross-links):

- 48 entities, neighbour links only: renders in 0.27 s.
- 30 entities, 3 cross-links: refuses `label/collision` (`e20--e21:to-card` touches `e26--e17`).
- 30 entities, 10 cross-links: refuses 6 × `label/collision`.
- 48 entities, 20 cross-links: refuses `route/touches` and 20 more findings after 29 s.

A route through a gutter next to a box crossed the spot of that box's cardinality. A label cut ended on another route. The A* allocated its tables per search and scanned every taken segment per move.

## Goal

`bp-diagram` only, plus a dev-profile line in `Cargo.toml`.

- `search.rs` (new) holds the A* that was in `route.rs`. A box marks only the moves it meets, by binary search on the lines. Taken segments are indexed by their line. One `Search` keeps its tables between searches and resets only the states it touched. Move costs are memoised per edge. A later side option stops when it cannot beat the best cost so far. The search counts the states it pops. Same costs; the four pinned renders are unchanged.
- `route.rs`: each cardinality's two spots are soft obstacles. Another route pays a stack (4,000) to cross one, so the card finds a clear side. The edge's own route crosses its own spots free, because its card then takes the other side. `spread` spaces card ends for two cards.
- `layout.rs`: each round returns its scene, its count of failed routes, cards and labels, and its work. `crowd` finds two routes that stack or touch and widens their gutter by two tracks. `widen` grows the gutter a crowded run lies in, or lengthens a run that is too short. Cards go first, forced cards before free ones.
- `label.rs`: `beside` gives a card's two spots to the router and to `card`. `place` keeps a cut away from other routes. `Clear` has a frame.
- The rules (`rules.rs`) do not change. `crowd` calls their `parallel_overlap` and `meet`.
- `Cargo.toml`: `[profile.dev.package.bp-diagram] opt-level = 2`. Unoptimised, the 48-entity test took 1.3 s against a 2 s bound.

The three approved examples (`bp-containers`, `bp-lm-design`, `bp-seq-issue-to-landed`) and the diff render give the same bytes as master. `the_bytes_are_the_same_on_every_platform` passes with the master hashes, and now also pins `bp-lm-whole` (`a704b3b602d4d9c6`).

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram
~~~
