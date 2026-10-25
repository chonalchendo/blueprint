kind: mutant
base: e5d89739bdf0e39dbd3a305b5371d5fab962f5eb
head: fc5178f5916f83f60d3bd325f4446b752966d447
title: The house style, Geist base with kind colour
genre: BinaryOperator
span: crates/bp-diagram/src/layout.rs:273-273
seed: 7
mutant_diff: |
    diff --git a/crates/bp-diagram/src/layout.rs b/crates/bp-diagram/src/layout.rs
    index cb8ecb3..b7423f1 100644
    --- a/crates/bp-diagram/src/layout.rs
    +++ b/crates/bp-diagram/src/layout.rs
    @@ -270,7 +270,7 @@ fn group_boxes(spec: &Spec, spans: &[Span], g: &grid::Grid) -> Vec<SGroup> {
             let row = i32::try_from(k).unwrap_or(0);
             let (_, y, _, h) = g.cell(0, row, 1, 1);
             let pad = 16;
    -        let rect = Rect::new(16, y - pad, g.w - 32, h + 2 * pad);
    +        let rect = Rect::new(16, y - pad, g.w / 32, h + 2 * pad);
             let runs = vec![Run::new(Font::Strong, 12, 0, "glabel", label).at(32, y + h / 2 + 4)];
             let members = spec.nodes.iter().filter(|n| n.lane.as_ref() == Some(id)).map(|n| n.id.clone()).collect();
             out.push(SGroup { id: id.clone(), rect, runs, members, lane: true });
truth:
- crates/bp-diagram/src/layout.rs:273-273 | replace - with / in group_boxes
---
## Problem

The spike's `bp-diagram` look copies the diagram-design plugin. It has a mono eyebrow over a large title, one coral focal accent, dashed outside boxes and mono kind lines in each box. A blind judgement against the plugin's renders cannot be fair when both sides look the same. Blueprint has no diagram style of its own.

## Shape

| Before | After |
|---|---|
| Page header: mono eyebrow, 28 px title, lede | Geist page header: 24 px title, a type tag pill, one muted line, full-width rule |
| Node: mono uppercase kind line over the name | Name in Inter 14 semibold. A kind tag pill beside the name only where the kind carries meaning (`Store`, `Library`, `Record`, an owner domain) |
| Coral focus; blue selection | Geist blue `#0070f3` focus: 1.5 px stroke and a pale tint. Blue means focus only. Selection strokes in ink |
| Grey fills per role; dashed outside boxes; store cap and deployable corner marks | A faint tint per kind and the kind pill in the same hue. Architecture: neutral application and component, violet library, teal store, plum deployable, grey outside. ER: indigo node, teal record, plum row, grey for another domain. Sequence boxes stay neutral. No cap or corner marks |
| Edge labels in ink with a 4 px radius | Muted 12 px labels in round pills; 1 px neutral connectors |
| Sequence step number in faint mono | The number in a grey step chip; the focus chips are blue |
| No drill badge (an empty `<g class="badge">`) | Additive: `render_with(spec, base, resolve, children)` takes a child count per node link. A drilled node shows a magnifier badge with the count, gets the class `drill`, and has `children` and the `badge` box in the index. `render` passes none |
| Legend: `KEY` in mono, a fill swatch per role | `Legend` in sans. A tagged role shows its tag as the swatch; a box swatch has an outline in its kind's hue. A diff's legend lists each of added, removed, changed and moved only when the drawing uses it |
| Diff: removed ghosts at 0.4, text included | Geist status colours: green added with a tint and a dark green label, red removed as a dashed ghost whose shapes fade to 0.35 and whose struck text stays in muted, amber changed with a dot in room the name row keeps, 6 px clear of the text |
| Dark: `#000` page, `#0a0a0a` card | Geist dark: `#0a0a0a` page, `#000` card, Geist grey ramp. Both schemes use CSS variables |
| The viewer always shows its controls | The viewer honours `#chrome=off`. The controls sit under the card |

## Check

~~~check shell {name=tests expect=0}
cargo test -q -p bp-diagram
~~~
