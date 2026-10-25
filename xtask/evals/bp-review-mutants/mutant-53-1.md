kind: mutant
base: 94cec4f24411727bde6c2859936234c96e0905eb
head: 048c4aca81e307115cf0ca623b8b2fc0ab91366a
title: Centre the drawing in its bounds
genre: BinaryOperator
span: crates/bp-diagram/src/scene.rs:185-185
seed: 7
mutant_diff: |
    diff --git a/crates/bp-diagram/src/scene.rs b/crates/bp-diagram/src/scene.rs
    index ced5d68..44c06d3 100644
    --- a/crates/bp-diagram/src/scene.rs
    +++ b/crates/bp-diagram/src/scene.rs
    @@ -182,7 +182,7 @@ impl Scene {
             }
             for n in &mut self.nodes {
                 crate::nodes::shift(n, dx, dy);
    -            n.lifeline = n.lifeline.map(|(x, a, b)| (x + dx, a + dy, b + dy));
    +            n.lifeline = n.lifeline.map(|(x, a, b)| (x + dx, a - dy, b + dy));
                 n.bars.iter_mut().for_each(|b| *b = move_rect(*b, dx, dy));
             }
             let at = |p: &mut Point| *p = (p.0 + dx, p.1 + dy);
truth:
- crates/bp-diagram/src/scene.rs:185-185 | replace + with - in Scene::fit
---
## Problem

The drawing's bounds came from the grid, not from the marks. Routes and pills that loop above the states sat on the frame's top edge. The space below the lowest state stayed empty.

Evidence: the owner's re-judge of `bp-wf-milestone` (2026-10-22). The pills "edit · end goal or finish line, the approval lapses" and "stamp · human, finish line fails, run recorded" touched the top edge of the frame.

## Shape

| Before | After |
|---|---|
| The viewBox is the grid's size. The grid keeps a 48 px outer channel that routes may or may not use. | The viewBox covers every box, group, lifeline, route piece and label mask, plus 24 px on all four sides. |
| A pill on a route in the outer channel can touch the frame. | Every mark keeps 24 px (arrowhead wings and hop bulges excepted, a few px) from the frame. The outermost marks sit exactly 24 px from all four sides. |
| The legend starts at the grid's bottom edge. | The legend starts at the drawing's bottom edge, which includes the margin. The placement rule does not change. |

This is not a contract change. The SVG, the index and the `size` field keep their shape. Their numbers change: each render moves, so the five byte hashes in `tests/render.rs` change on purpose.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram
~~~
