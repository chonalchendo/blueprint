//! The cell types (architecture, er, state, workflow): boxes measured, placed in cells, routed and labelled. When a
//! label finds no clear spot, the gutter its longest run sits in grows by what it lacks, and the whole layout runs
//! again: at most four times, so the result stays a pure function of the spec.

use std::collections::{BTreeMap, BTreeSet};

use crate::Finding;
use crate::font::Font;
use crate::geom::{Point, Rect};
use crate::grid::{self, GROUP_SIDE, GROUP_TOP, Item, Span};
use crate::label::{self, Clear};
use crate::nodes::{self, shift, up4};
use crate::route::{self, Want};
use crate::scene::{Run, SEdge, SGroup, SLabel, SNode, Scene};
use crate::spec::{Spec, Type};

/// The left room a workflow keeps for its lane names.
const LANE_HEAD: i32 = 112;

/// What every round of one layout shares: the measured boxes, their cells and spans, the edges and their order.
struct Plan<'a> {
    spec: &'a Spec,
    built: Vec<SNode>,
    cells: Vec<(String, Item)>,
    spans: Vec<Span>,
    wants: Vec<Want>,
    order: Vec<usize>,
    gaps: (i32, i32),
}

/// Lays out `spec`. Nodes in `ghosts` (a diff's removed items) skip the cell rules.
pub(crate) fn lay_out(spec: &Spec, ghosts: &BTreeSet<String>) -> Result<Scene, Vec<Finding>> {
    let built: Vec<SNode> = spec
        .nodes
        .iter()
        .map(|n| match spec.ty {
            Type::Architecture => nodes::architecture(n),
            Type::Er => nodes::entity(n, spec.level.as_deref()),
            _ => nodes::ordered(n, spec.ty),
        })
        .collect();
    let cells = cells_of(spec, &built);
    let checked: Vec<(String, Item)> = cells.iter().filter(|(id, _)| !ghosts.contains(id)).cloned().collect();
    let groups: Vec<(String, Vec<String>)> = spec.groups.iter().map(|g| (g.id.clone(), g.members.clone())).collect();
    let refused = grid::cell_rules(&checked, &groups);
    if !refused.is_empty() {
        return Err(refused);
    }
    let spans = spec
        .groups
        .iter()
        .filter_map(|g| {
            grid::span_of(&cells.iter().filter(|(id, _)| g.members.contains(id)).map(|(_, i)| i).collect::<Vec<_>>())
        })
        .collect();
    let gaps = match spec.ty {
        Type::Architecture => (96, 80),
        Type::Er => (104, 80),
        Type::State => (72, 64),
        Type::Workflow => (56, 48),
        Type::Sequence => unreachable!("a sequence has no cells"),
    };
    let index: BTreeMap<&str, usize> = spec.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let wants: Vec<Want> = spec
        .edges
        .iter()
        .map(|e| Want { from: index[e.from.as_str()], to: index[e.to.as_str()], exit: e.exit, enter: e.enter })
        .collect();
    // Near edges route first: a neighbour's straight line is the one a long route would otherwise take. Among equals,
    // focal edges go first, then the spec's order.
    let focal = |e: &crate::spec::Edge| spec.focus.iter().any(|f| *f == e.id || *f == e.from || *f == e.to);
    let apart = |w: &Want| {
        let (a, b) = (&cells[w.from].1, &cells[w.to].1);
        let gap = |lo: i32, span: i32, olo: i32, ospan: i32| (olo - (lo + span - 1)).max(lo - (olo + ospan - 1)).max(0);
        gap(a.col, a.cspan, b.col, b.cspan) + gap(a.row, a.rspan, b.row, b.rspan)
    };
    let mut order: Vec<usize> = (0..spec.edges.len()).collect();
    order.sort_by_key(|&i| (apart(&wants[i]), !focal(&spec.edges[i]), i));
    let plan = Plan { spec, built, cells, spans, wants, order, gaps };
    let mut wide = BTreeMap::new();
    for round in 0..5 {
        if let Some(scene) = attempt(&plan, &mut wide, round < 4) {
            return Ok(scene);
        }
    }
    unreachable!("the fifth round places every label")
}

/// One round: place, route, label. With `retry`, a label with no clear spot widens its gutter and the round gives
/// up; without, the label stays where the rules will name it.
fn attempt(plan: &Plan<'_>, wide: &mut BTreeMap<(bool, i32), i32>, retry: bool) -> Option<Scene> {
    let spec = plan.spec;
    let items: Vec<Item> = plan.cells.iter().map(|(_, i)| *i).collect();
    let mut grid = grid::place(&items, &plan.spans, plan.gaps.0, plan.gaps.1, wide);
    let left = if spec.ty == Type::Workflow { LANE_HEAD } else { 0 };
    grid.w += left;
    for x in &mut grid.colx {
        *x += left;
    }
    let mut placed = plan.built.clone();
    for (node, (_, item)) in placed.iter_mut().zip(&plan.cells) {
        let (x, y, w, h) = grid.cell(item.col, item.row, item.cspan, item.rspan);
        match spec.ty {
            Type::Architecture => (node.rect.w, node.rect.h) = (w, h),
            Type::Er => node.rect.w = w,
            _ => {}
        }
        let (dx, dy) = ((w - node.rect.w) / 2 / 4 * 4, (h - node.rect.h) / 2 / 4 * 4);
        let centred = !matches!(spec.ty, Type::Architecture | Type::Er);
        shift(node, x - node.rect.x + if centred { dx } else { 0 }, y - node.rect.y + if centred { dy } else { 0 });
        if spec.focus.contains(&node.id) {
            node.class.push("focus");
        }
    }
    let rects: Vec<Rect> = placed.iter().map(|n| n.rect).collect();
    let groups = group_boxes(spec, &plan.spans, &grid);
    let border_x: Vec<i32> = groups.iter().filter(|s| !s.lane).flat_map(|s| [s.rect.x, s.rect.right()]).collect();
    let border_y: Vec<i32> = groups.iter().filter(|s| !s.lane).flat_map(|s| [s.rect.y, s.rect.bottom()]).collect();
    let xs = route::lines(&grid.xbands(), &border_x, grid.w);
    let ys = route::lines(&grid.ybands(), &border_y, grid.h);
    let marks: Vec<Rect> = groups.iter().map(SGroup::label_box).collect();
    let pts: Vec<Vec<Point>> =
        route::route(&rects, &marks, &xs, &ys, &plan.wants, &plan.order).into_iter().map(|r| r.0).collect();
    let walls: Vec<Rect> = rects.iter().chain(&marks).copied().collect();
    let edges = label_edges(plan, &grid, &walls, &pts, wide, retry)?;
    Some(Scene { w: grid.w, h: grid.h, groups, edges, nodes: placed, legend_y: grid.h, ..Scene::default() })
}

/// Labels every edge in routing order, then bridges each crossing on the later route. None when a label found no
/// spot and `retry` asked for another round.
fn label_edges(
    plan: &Plan<'_>,
    grid: &grid::Grid,
    rects: &[Rect],
    pts: &[Vec<Point>],
    wide: &mut BTreeMap<(bool, i32), i32>,
    retry: bool,
) -> Option<Vec<SEdge>> {
    let mut clear = Clear { boxes: rects, routes: pts, labels: Vec::new() };
    let mut edges: Vec<Option<SEdge>> = vec![None; pts.len()];
    let mut short = false;
    for &e in &plan.order {
        let edge = &plan.spec.edges[e];
        let mut labels = Vec::new();
        let mut cut = None;
        // Cardinalities sit at the ends first; the name then takes the clear stretch between them.
        if let Some((from, to)) = &edge.card {
            for (text, start, end) in [(from, true, "from"), (to, false, "to")] {
                let run = Run::new(Font::Mono, 10, 0, "card", text.as_str());
                let (rect, ok) = label::card(&pts[e], start, e, up4(run.w), &clear);
                short |= !ok && retry;
                clear.labels.push(rect);
                labels.push(SLabel {
                    id: format!("{}:{end}-card", edge.id),
                    rect,
                    runs: vec![run.at(rect.x, rect.y + 10)],
                    pill: false,
                });
            }
        }
        if !edge.label.is_empty() {
            let (mut pill, width) = label::pill(&edge.id, &edge.label);
            match label::place(&pts[e], e, width, label::PILL_H, &clear) {
                Some(spot) => {
                    pill.rect = spot.rect;
                    cut = spot.cut;
                }
                None if retry => {
                    short = true;
                    widen(grid, &pts[e], width, wide);
                }
                None => pill.rect = Rect::new(pts[e][0].0, pts[e][0].1, width, label::PILL_H),
            }
            center(&mut pill);
            clear.labels.push(pill.rect);
            labels.push(pill);
        }
        edges[e] = Some(SEdge {
            id: edge.id.clone(),
            from: edge.from.clone(),
            to: edge.to.clone(),
            dashed: edge.dashed,
            heads: heads(&pts[e], edge.both),
            pieces: label::cut(&pts[e], cut),
            bridges: Vec::new(),
            labels,
            class: if plan.spec.focus.contains(&edge.id) { vec!["focus"] } else { Vec::new() },
            step: None,
        });
    }
    if short {
        return None;
    }
    let mut edges: Vec<SEdge> = edges.into_iter().map(|e| e.expect("every edge is routed")).collect();
    for (i, &a) in plan.order.iter().enumerate() {
        for &b in &plan.order[i + 1..] {
            edges[b].bridges.extend(route::crossings(&pts[a], &pts[b]));
        }
    }
    Some(edges)
}

/// Grows the gutter that holds the route's longest run: a run across a column gutter needs it wider, a run down a
/// row gutter, taller.
fn widen(grid: &grid::Grid, pts: &[Point], width: i32, wide: &mut BTreeMap<(bool, i32), i32>) {
    let k = longest(pts);
    let (a, b) = (pts[k], pts[k + 1]);
    let (key, more) = if a.1 == b.1 {
        ((true, gutter(&grid.colx, &grid.colw, a.0.midpoint(b.0))), width + 72 - (a.0 - b.0).abs())
    } else {
        ((false, gutter(&grid.rowy, &grid.rowh, a.1.midpoint(b.1))), 24)
    };
    *wide.entry(key).or_insert(0) += more.max(24);
}

/// Each node's cell: the author's for architecture and er; from order for state and workflow.
fn cells_of(spec: &Spec, built: &[SNode]) -> Vec<(String, Item)> {
    let mut taken: BTreeSet<(i32, i32)> = BTreeSet::new();
    let mut out: Vec<(String, Item)> = Vec::new();
    for (i, (n, b)) in spec.nodes.iter().zip(built).enumerate() {
        let at = match (spec.ty, n.at) {
            (_, Some(at)) => at,
            (Type::Workflow, None) => {
                let lane = spec.lanes.iter().position(|(l, _)| Some(l) == n.lane.as_ref()).unwrap_or(0);
                (i32::try_from(i).unwrap_or(0), i32::try_from(lane).unwrap_or(0))
            }
            // A state goes right of the first placed state that leads to it, else below it, else at the row's end.
            _ => {
                let source = spec
                    .edges
                    .iter()
                    .filter(|e| e.to == n.id)
                    .find_map(|e| out.iter().find(|(id, _)| *id == e.from).map(|(_, it)| (it.col, it.row)));
                let (c, r) =
                    source.map_or((out.iter().map(|(_, it)| it.col + 1).max().unwrap_or(0), 0), |(c, r)| (c + 1, r));
                (r..r + 64)
                    .map(|row| (c, row))
                    .find(|cell| !taken.contains(cell))
                    .expect("a column has a free cell below")
            }
        };
        taken.insert(at);
        out.push((
            n.id.clone(),
            Item { col: at.0, row: at.1, cspan: n.span.0, rspan: n.span.1, w: b.rect.w, h: b.rect.h },
        ));
    }
    out
}

fn group_boxes(spec: &Spec, spans: &[Span], g: &grid::Grid) -> Vec<SGroup> {
    let mut out: Vec<SGroup> = spec
        .groups
        .iter()
        .zip(spans)
        .map(|(gr, &(c0, r0, c1, r1))| {
            let (x, y, _, _) = g.cell(c0, r0, 1, 1);
            let (x2, y2, w2, h2) = g.cell(c1, r1, 1, 1);
            let rect = Rect::new(
                x - GROUP_SIDE,
                y - GROUP_TOP,
                x2 + w2 - x + 2 * GROUP_SIDE,
                y2 + h2 - y + GROUP_TOP + GROUP_SIDE,
            );
            let runs =
                vec![Run::new(Font::Mono, 10, 1, "glabel", gr.label.to_uppercase()).at(rect.x + 16, rect.y + 22)];
            SGroup { id: gr.id.clone(), rect, runs, members: gr.members.clone(), lane: false }
        })
        .collect();
    for (k, (id, label)) in spec.lanes.iter().enumerate() {
        let row = i32::try_from(k).unwrap_or(0);
        let (_, y, _, h) = g.cell(0, row, 1, 1);
        let pad = 16;
        let rect = Rect::new(16, y - pad, g.w - 32, h + 2 * pad);
        let runs = vec![Run::new(Font::Mono, 10, 1, "glabel", label.to_uppercase()).at(32, y + h / 2 + 4)];
        let members = spec.nodes.iter().filter(|n| n.lane.as_ref() == Some(id)).map(|n| n.id.clone()).collect();
        out.push(SGroup { id: id.clone(), rect, runs, members, lane: true });
    }
    out
}

/// The gutter index that holds `v`: -1 before the first track, `k` after track `k`.
fn gutter(at: &[i32], size: &[i32], v: i32) -> i32 {
    let k = at.iter().zip(size).take_while(|(a, s)| v >= *a + *s).count();
    i32::try_from(k).unwrap_or(0) - 1
}

fn longest(pts: &[Point]) -> usize {
    let len = |w: &[Point]| (w[0].0 - w[1].0).abs() + (w[0].1 - w[1].1).abs();
    let mut best = 0;
    for (k, w) in pts.windows(2).enumerate() {
        if len(w) > len(&pts[best..best + 2]) {
            best = k;
        }
    }
    best
}

/// Centres a pill's run in its mask.
fn center(l: &mut SLabel) {
    let r = l.rect;
    let cap = Font::Body.cap(12);
    l.runs[0].x = r.x + r.w / 2;
    l.runs[0].y = r.y + r.h / 2 + cap / 2;
    l.runs[0].mid = true;
}

/// Arrowheads: at the end, and at the start too for a two-way edge. Each is the tip and the direction it points.
pub(crate) fn heads(pts: &[Point], both: bool) -> Vec<(Point, Point)> {
    let dir = |a: Point, b: Point| ((b.0 - a.0).signum(), (b.1 - a.1).signum());
    let n = pts.len();
    let mut out = vec![(pts[n - 1], dir(pts[n - 2], pts[n - 1]))];
    if both {
        out.push((pts[0], dir(pts[1], pts[0])));
    }
    out
}
