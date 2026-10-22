//! The cell types (architecture, er, state, workflow): boxes measured, placed in cells, routed and labelled. A gutter
//! widens by the routes it carries: when two routes find no free track in it, or a label finds no clear spot, it
//! grows by what they lack, and the layout runs again with the sides it picked. At most [`ROUNDS`] rounds, so the
//! result stays a pure function of the spec and a refusal comes fast.

use std::collections::{BTreeMap, BTreeSet};

use crate::Finding;
use crate::font::Font;
use crate::geom::{Point, Rect};
use crate::grid::{self, GROUP_SIDE, GROUP_TOP, Item, Span};
use crate::label::{self, Clear};
use crate::nodes::{self, shift, up4};
use crate::route::{self, Want};
use crate::rules;
use crate::scene::{Run, SEdge, SGroup, SLabel, SNode, Scene};
use crate::spec::{Side, Spec, Type};

/// The most rounds one layout takes. A round after the first reuses its sides but routes and labels every edge again,
/// so a large model pays for each round. The loop also stops at the first round that leaves no fewer routes, cards and
/// labels failing than the round before.
const ROUNDS: usize = 8;
/// The most search work, in states popped, a layout spends before it refuses. A count, not a clock, keeps the bytes the
/// same on every machine. The 23-entity model takes under 1 M; 120 entities and 370 edges take 14 M in their first round.
const WORK: usize = 24_000_000;

/// The most a gutter grows over all rounds. Past it, a label that still finds no spot is crowded by what widening
/// cannot move (its own route's box, a self loop), and the rules name it.
const GROW_MAX: i32 = 240;

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
        .map(|e| {
            let card = |t: &str| up4(Run::new(Font::Mono, 10, 0, "card", t).w);
            let cards = e.card.as_ref().map_or([0, 0], |(a, b)| [card(a), card(b)]);
            Want { from: index[e.from.as_str()], to: index[e.to.as_str()], exit: e.exit, enter: e.enter, cards }
        })
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
    let (mut wide, mut sides, mut last, mut spent) = (BTreeMap::new(), Vec::new(), usize::MAX, 0);
    for round in 1.. {
        let (scene, failed, grow, work) = attempt(&plan, &wide, &mut sides);
        spent += work;
        // A round that fixes no more than the one before will not be helped by the next, and a round that would cost
        // as much as this one must fit the work left: else the rules name what is left.
        if failed == 0 || failed >= last || round == ROUNDS || spent + work > WORK {
            return Ok(scene);
        }
        last = failed;
        for (key, more) in grow {
            let at = wide.entry(key).or_insert(0);
            *at = (*at + more).min(GROW_MAX);
        }
    }
    unreachable!("the loop returns by round {ROUNDS}")
}

/// One round: place, route, label. Returns the scene, how many routes, cards and labels failed (a failed label stays
/// where the rules will name it), and how much each short gutter asks to grow for the next round.
fn attempt(
    plan: &Plan<'_>,
    wide: &BTreeMap<(bool, i32), i32>,
    sides: &mut Vec<(Side, Side)>,
) -> (Scene, usize, BTreeMap<(bool, i32), i32>, usize) {
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
    let (pts, work) = route::route(&rects, &marks, &xs, &ys, &plan.wants, &plan.order, sides);
    // Each gutter a round finds short grows once, by the most any one route or label asked of it.
    let mut grow = BTreeMap::new();
    let crowded = crowd(&grid, &pts, &mut grow);
    let walls: Vec<Rect> = rects.iter().chain(&marks).copied().collect();
    let frames: Vec<Rect> = groups.iter().filter(|s| !s.lane).map(|s| s.rect).collect();
    let (edges, short) = label_edges(plan, &grid, (&walls, &frames), &pts, &mut grow);
    let scene = Scene { w: grid.w, h: grid.h, groups, edges, nodes: placed, legend_y: grid.h, ..Scene::default() };
    (scene, crowded + short, grow, work)
}

/// Labels every edge in routing order, then bridges each crossing on the later route. Returns the edges and how many
/// cards and labels found no spot; each asks its gutter to grow. Every cardinality goes first, as it is fixed at its port and a name may
/// slide along its route; and a card that its own route leaves one side for goes before the cards that may take either.
fn label_edges(
    plan: &Plan<'_>,
    grid: &grid::Grid,
    (rects, frames): (&[Rect], &[Rect]),
    pts: &[Vec<Point>],
    grow: &mut BTreeMap<(bool, i32), i32>,
) -> (Vec<SEdge>, usize) {
    let frame = Rect::new(0, 0, grid.w, grid.h);
    let mut clear = Clear { frame, boxes: rects, frames, routes: pts, labels: Vec::new() };
    let mut edges: Vec<Option<SEdge>> = vec![None; pts.len()];
    let mut short = 0;
    let mut cards: Vec<(usize, usize, bool)> = Vec::new();
    for (k, &e) in plan.order.iter().enumerate() {
        for start in [true, false].into_iter().filter(|_| plan.spec.edges[e].card.is_some()) {
            let free = label::card(&pts[e], start, e, plan.wants[e].cards[usize::from(!start)], &clear).1;
            cards.push((usize::from(free > 1), k, start));
        }
    }
    cards.sort_unstable();
    let mut placed: Vec<Vec<SLabel>> = vec![Vec::new(); pts.len()];
    for (_, k, start) in cards {
        let e = plan.order[k];
        let (from, to) = plan.spec.edges[e].card.as_ref().expect("only an edge with cardinalities has cards");
        let (text, end) = if start { (from, "from") } else { (to, "to") };
        let run = Run::new(Font::Mono, 10, 0, "card", text.as_str());
        let (rect, free) = label::card(&pts[e], start, e, plan.wants[e].cards[usize::from(!start)], &clear);
        if free == 0 {
            // The card's end sits in a crowded gutter: it grows, so the routes beside the end move off it.
            short += 1;
            let pts = &pts[e];
            let (end, next) = if start { (pts[0], pts[1]) } else { (pts[pts.len() - 1], pts[pts.len() - 2]) };
            let key = if end.1 == next.1 {
                (true, gutter(&grid.colx, &grid.colw, next.0))
            } else {
                (false, gutter(&grid.rowy, &grid.rowh, next.1))
            };
            ask(grow, key, 24);
        }
        clear.labels.push(rect);
        let card = SLabel {
            id: format!("{}:{end}-card", plan.spec.edges[e].id),
            rect,
            runs: vec![run.at(rect.x, rect.y + 10)],
            pill: false,
        };
        // The from card stays first in the edge's list, whichever was placed first.
        if start { placed[e].insert(0, card) } else { placed[e].push(card) }
    }
    for &e in &plan.order {
        let edge = &plan.spec.edges[e];
        let mut labels = std::mem::take(&mut placed[e]);
        let mut cut = None;
        if !edge.label.is_empty() {
            let (mut pill, width) = label::pill(&edge.id, &edge.label);
            if let Some(spot) = label::place(&pts[e], e, width, label::PILL_H, &clear) {
                pill.rect = spot.rect;
                cut = spot.cut;
            } else {
                short += 1;
                widen(grid, &pts[e], width, grow);
                pill.rect = Rect::new(pts[e][0].0, pts[e][0].1, width, label::PILL_H);
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
    let mut edges: Vec<SEdge> = edges.into_iter().map(|e| e.expect("every edge is routed")).collect();
    for (i, &a) in plan.order.iter().enumerate() {
        for &b in &plan.order[i + 1..] {
            edges[b].bridges.extend(route::crossings(&pts[a], &pts[b]));
        }
    }
    (edges, short)
}

/// Grows each gutter that ran out of tracks, by two tracks: where two routes share a stretch or a point (the rules
/// would refuse either), the gutter the first one runs in widens. Returns how many pairs of routes met.
fn crowd(grid: &grid::Grid, pts: &[Vec<Point>], grow: &mut BTreeMap<(bool, i32), i32>) -> usize {
    let mut full = BTreeSet::new();
    let mut met = 0;
    for (i, a) in pts.iter().enumerate() {
        for b in &pts[i + 1..] {
            let mut hit = false;
            for p in a.windows(2) {
                for q in b.windows(2) {
                    let (p, q) = ((p[0], p[1]), (q[0], q[1]));
                    if rules::parallel_overlap(p, q) || rules::meet(p, q).is_some() {
                        hit = true;
                        full.insert(if p.0.0 == p.1.0 {
                            (true, gutter(&grid.colx, &grid.colw, p.0.0))
                        } else {
                            (false, gutter(&grid.rowy, &grid.rowh, p.0.1))
                        });
                    }
                }
            }
            met += usize::from(hit);
        }
    }
    for key in &full {
        ask(grow, *key, 2 * 12);
    }
    met
}

/// Grows a gutter for a label that found no spot on its route's longest run. A run between two boxes, or too short
/// for the pill, lengthens: the gutter it crosses grows. A run along a gutter, long enough, is crowded: that gutter
/// grows, so the pill fits on the line or beside it.
fn widen(grid: &grid::Grid, pts: &[Point], width: i32, grow: &mut BTreeMap<(bool, i32), i32>) {
    let k = longest(pts);
    let (a, b) = (pts[k], pts[k + 1]);
    let flat = a.1 == b.1;
    let len = (a.0 - b.0).abs() + (a.1 - b.1).abs();
    let need = if flat { width } else { label::PILL_H } + 72;
    let within = |at: &[i32], size: &[i32], v: i32| at.iter().zip(size).any(|(p, s)| (*p..*p + *s).contains(&v));
    let cols = |x: i32| (true, gutter(&grid.colx, &grid.colw, x));
    let rows = |y: i32| (false, gutter(&grid.rowy, &grid.rowh, y));
    let (key, more) = if flat && (len < need || within(&grid.rowy, &grid.rowh, a.1)) {
        (cols(a.0.midpoint(b.0)), (need - len).max(width))
    } else if !flat && (len < need || within(&grid.colx, &grid.colw, a.0)) {
        (rows(a.1.midpoint(b.1)), (need - len).max(label::PILL_H))
    } else if flat {
        (rows(a.1), label::PILL_H + 2 * label::GAP)
    } else {
        (cols(a.0), width + 2 * label::GAP)
    };
    ask(grow, key, up4(more).max(24));
}

/// Asks a gutter to grow by `more` this round: the largest ask wins.
fn ask(grow: &mut BTreeMap<(bool, i32), i32>, key: (bool, i32), more: i32) {
    let at = grow.entry(key).or_insert(0);
    *at = (*at).max(more);
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
            let runs = vec![Run::new(Font::Strong, 12, 0, "glabel", &gr.label).at(rect.x + 16, rect.y + 24)];
            SGroup { id: gr.id.clone(), rect, runs, members: gr.members.clone(), lane: false }
        })
        .collect();
    for (k, (id, label)) in spec.lanes.iter().enumerate() {
        let row = i32::try_from(k).unwrap_or(0);
        let (_, y, _, h) = g.cell(0, row, 1, 1);
        let pad = 16;
        let rect = Rect::new(16, y - pad, g.w - 32, h + 2 * pad);
        let runs = vec![Run::new(Font::Strong, 12, 0, "glabel", label).at(32, y + h / 2 + 4)];
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
