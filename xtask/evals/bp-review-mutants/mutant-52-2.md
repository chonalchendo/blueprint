kind: mutant
base: c6f32eff0205c0515bf2f7c3f217678844a6dedc
head: 94cec4f24411727bde6c2859936234c96e0905eb
title: The owner's render items
genre: BinaryOperator
span: crates/bp-diagram/src/route.rs:238-238
seed: 7
mutant_diff: |
    diff --git a/crates/bp-diagram/src/route.rs b/crates/bp-diagram/src/route.rs
    index bddc048..c87ac37 100644
    --- a/crates/bp-diagram/src/route.rs
    +++ b/crates/bp-diagram/src/route.rs
    @@ -235,7 +235,7 @@ fn pick_sides(
             needs.push(mine);
             let len = if matches!(side, Side::Left | Side::Right) { boxes[at].h } else { boxes[at].w };
             // `gaps` floors each gap at EDGE, so a side with no card at all would count as crowded: only a card needs room.
    -        needs.iter().any(|n| *n > 0) && gaps(&needs).iter().sum::<i32>() > len - 2 * EDGE
    +        needs.iter().any(|n| *n > 0) && gaps(&needs).iter().sum::<i32>() >= len - 2 * EDGE
         };
         for &edge in order {
             let want = &wants[edge];
truth:
- crates/bp-diagram/src/route.rs:238-238 | replace > with >= in pick_sides
---
## Problem

Five render gaps from issues 5 and 10 are open. The owner decided them on 2026-10-22 (28, issue 12).

- A legend drawn from use shows a role pill (`Person`, `Record`, `Library`) with no words, or only a gloss that repeats the pill.
- A state legend has no `Start` or `End` swatch that matches the dot and the ring on the nodes.
- A `start` or `end` node keeps a `label` that nothing draws.
- A label pill can sit on a group frame. In `bp-containers.yaml`, `runs checks on` straddled the System frame's right edge.
- Two cards of different edges at one crowded box side refuse. A diagonal neighbour and a left neighbour of one box gave `label/gap n3--n0:to-card`.

## Shape

| Before | After |
|---|---|
| A role pill in a legend drawn from use has no words, or repeats its tag. | A gloss follows the pill from one table (`GLOSS` in `legend.rs`): `A person`, `Code library`, `Stored record`. An author's label wins. Additive. |
| A legend label equal to its pill's tag drew no words. | The label draws. `bp-containers*.yaml` now say `Code library`. |
| `Start` and `End` legend swatches are plain boxes. | A dot and a ring, as the nodes draw them. Additive. |
| A `start` node's dot draws dark and hollow: `.bp .node .box` beat `.bp .solid`. | The dot is solid (`style.css`). |
| `label` on a `start` or `end` node is stored and not drawn. | `spec/schema` refuses it: ``a `start` node draws no `label`; say it in the design``. Breaking for such specs. |
| A pill can sit on a group frame's edge. | A pill sits wholly inside or wholly outside each frame, with its 4 px mask. |
| Two cards at one crowded side refuse `label/gap`. | A side where a card needs room and has none is costed as a stack, so a route takes another side. The shape renders. A side with no cards is not costed, so card-free diagrams route as at master. |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram
~~~
