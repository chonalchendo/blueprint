kind: mutant
base: 1f275408c28a70f0dd916decf1df9ce27472af91
head: e5d89739bdf0e39dbd3a305b5371d5fab962f5eb
title: The diagram spike
genre: BinaryOperator
span: crates/bp-diagram/src/scene.rs:116-116
seed: 11
mutant_diff: |
    diff --git a/crates/bp-diagram/src/scene.rs b/crates/bp-diagram/src/scene.rs
    index d55302a..3cd6734 100644
    --- a/crates/bp-diagram/src/scene.rs
    +++ b/crates/bp-diagram/src/scene.rs
    @@ -113,7 +113,7 @@ impl SGroup {
             });
             let r = r.unwrap_or_default();
             let (x, y) = (r.x.div_euclid(4) * 4, r.y.div_euclid(4) * 4);
    -        Rect::new(x, y, (r.right() - x + 3) / 4 * 4, (r.bottom() - y + 3) / 4 * 4)
    +        Rect::new(x, y, (r.right() - x + 3) % 4 * 4, (r.bottom() - y + 3) / 4 * 4)
         }
     }
truth:
- crates/bp-diagram/src/scene.rs:116-116 | replace / with % in SGroup::label_box
---
## Problem

Blueprint cannot draw a design. The three reference diagrams in `diagrams/` come from the diagram-design plugin. They are hand-placed, and no rule checks them. `research/m5-diagram-tools.md` §5.6 records two container edges that point to the wrong boxes. A one-box change means a redraw by hand.

## Goal

- `bp-diagram` depends on no workspace crate (`crate-rule.txt`). `bp-tool` depends on it.
- Layout: integer geometry on a 4 px grid. Nodes sit in authored cells; gutters widen when a label finds no spot.
- Text: `font.rs` measures with advance tables generated from the same WOFF2 subsets the page embeds (Inter 400 and 600, Geist Mono 400, OFL). So the measured text and the drawn text are one face.
- Routes: A* over gutter tracks and port lines, `r=8` quarter arcs, bridges at crossings. Near edges route first, so neighbours get the straight lines.
- Rules (`rules.rs`, `spec.rs`, `grid.rs`, `legend.rs`): 17 rule names, for example `route/through-box`, `route/touches`, `label/collision`, `layout/group-shape`, `legend/truthful`. A route may not touch a box that is not its endpoint, run along a border, or meet another route except at a bridged crossing.
- Group labels and the legend are obstacles for routes and labels. Every label sits on the 4 px grid.
- SVG contract (design change 12): `data-id` on every element; `data-term` or `data-noun` and `data-role` on nodes; a `<metadata>` JSON index; role tokens as CSS variables with light and dark values; `dim`, `selected`, `active`, `added`, `removed` classes with 450 ms transitions.
- Viewer: `viewer/viewer.js` (11,276 bytes), embedded with `include_str!` as the page's one `<script>`. It adds fit, pan, zoom 25–400 %, hover dim, a detail panel, base/head/diff and sequence step-through, all by `data-id`, with state in the URL hash. The SVG draws in full without it.
- Output is byte-identical between runs; `the_bytes_are_the_same_on_every_platform` pins an FNV-1a hash of each of the four example renders.

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram -p bp-operation -p bp-tool
~~~
