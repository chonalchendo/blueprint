//! The router (`m5-diagram-tools.md` §5.3, steps 3 and 4). Ends are ports spread along a side; routes are A* over
//! the gutter tracks and port lines, with cost order: never through a box, then not along another route, then fewest
//! crossings, then fewest bends, then length. Tracks sit 12 px apart, so two routes in one gutter never stack.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::geom::{Point, Rect};
use crate::spec::Side;

const STUB: i32 = 16;
/// An end keeps this far from its side's corners, and from the next end on the side.
const EDGE: i32 = 16;
const TRACK: i32 = 12;
const BEND: i64 = 40;
const CROSS: i64 = 160;
const STACK: i64 = 4000;

/// One edge to route: the boxes it joins and the author's side hints.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Want {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) exit: Option<Side>,
    pub(crate) enter: Option<Side>,
}

const DIRS: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

const fn unit(s: Side) -> Point {
    match s {
        Side::Top => (0, -1),
        Side::Right => (1, 0),
        Side::Bottom => (0, 1),
        Side::Left => (-1, 0),
    }
}

const fn opposite(s: Side) -> Side {
    match s {
        Side::Top => Side::Bottom,
        Side::Right => Side::Left,
        Side::Bottom => Side::Top,
        Side::Left => Side::Right,
    }
}

/// The port at `along` (an x on a top or bottom side, a y on a left or right side) of `side` of `b`, and its stub
/// point outside the box.
fn port(b: &Rect, side: Side, along: i32) -> (Point, Point) {
    let p = match side {
        Side::Top => (along, b.y),
        Side::Bottom => (along, b.bottom()),
        Side::Left => (b.x, along),
        Side::Right => (b.right(), along),
    };
    let u = unit(side);
    (p, (p.0 + u.0 * STUB, p.1 + u.1 * STUB))
}

/// Where an end on `side` of `b` would sit alone: the middle of the stretch it shares with the other box `o`, so
/// facing boxes join with a straight line; else as near `o` as the side allows.
fn preferred(b: &Rect, side: Side, o: &Rect) -> i32 {
    let ((lo, hi), (olo, ohi)) = if matches!(side, Side::Left | Side::Right) {
        ((b.y, b.bottom()), (o.y, o.bottom()))
    } else {
        ((b.x, b.right()), (o.x, o.right()))
    };
    if hi - lo < 2 * EDGE {
        return lo.midpoint(hi) / 4 * 4;
    }
    let (shared_lo, shared_hi) = (lo.max(olo), hi.min(ohi));
    let want = if shared_hi - shared_lo >= 2 * EDGE { shared_lo.midpoint(shared_hi) } else { olo.midpoint(ohi) };
    want.clamp(lo + EDGE, hi - EDGE) / 4 * 4
}

/// Candidate side pairs, best guess first: facing sides when the boxes share a row or a column, else the two L
/// shapes and the two Z shapes. The author's hints replace a side.
fn options(a: &Rect, b: &Rect, want: &Want) -> Vec<(Side, Side)> {
    let across = if b.x + b.w / 2 >= a.x + a.w / 2 { Side::Right } else { Side::Left };
    let down = if b.y + b.h / 2 >= a.y + a.h / 2 { Side::Bottom } else { Side::Top };
    let rows = a.y < b.bottom() && b.y < a.bottom();
    let cols = a.x < b.right() && b.x < a.right();
    let mut out = if want.from == want.to {
        vec![(Side::Right, Side::Top), (Side::Bottom, Side::Right)]
    } else if rows && !cols {
        vec![(across, opposite(across)), (down, opposite(down)), (Side::Top, Side::Top), (Side::Bottom, Side::Bottom)]
    } else if cols && !rows {
        vec![(down, opposite(down)), (across, across), (opposite(across), opposite(across))]
    } else {
        vec![(across, opposite(down)), (down, opposite(across)), (across, opposite(across)), (down, opposite(down))]
    };
    for pair in &mut out {
        pair.0 = want.exit.unwrap_or(pair.0);
        pair.1 = want.enter.unwrap_or(pair.1);
    }
    out.dedup();
    out
}

/// The search space: candidate x and y lines, and which moves a box blocks.
struct Graph {
    xs: Vec<i32>,
    ys: Vec<i32>,
    hblock: Vec<bool>,
    vblock: Vec<bool>,
}

impl Graph {
    fn new(boxes: &[Rect], mut xs: Vec<i32>, mut ys: Vec<i32>) -> Self {
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();
        let walls: Vec<Rect> = boxes.iter().map(|b| b.inflate(8)).collect();
        let blocked = |a: Point, b: Point| walls.iter().any(|w| Rect::of_segment(a, b).meets(w));
        let (nx, ny) = (xs.len(), ys.len());
        let mut hblock = vec![true; nx * ny];
        let mut vblock = vec![true; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                if i + 1 < nx {
                    hblock[j * nx + i] = blocked((xs[i], ys[j]), (xs[i + 1], ys[j]));
                }
                if j + 1 < ny {
                    vblock[j * nx + i] = blocked((xs[i], ys[j]), (xs[i], ys[j + 1]));
                }
            }
        }
        Self { xs, ys, hblock, vblock }
    }

    fn vertex(&self, p: Point) -> usize {
        let i = self.xs.binary_search(&p.0).expect("every stub lies on a candidate line");
        let j = self.ys.binary_search(&p.1).expect("every stub lies on a candidate line");
        j * self.xs.len() + i
    }

    fn point(&self, v: usize) -> Point {
        (self.xs[v % self.xs.len()], self.ys[v / self.xs.len()])
    }

    /// The vertex one step in `d` from `v`, unless a box blocks the move.
    fn step(&self, v: usize, d: Side) -> Option<usize> {
        let nx = self.xs.len();
        let (i, j) = (v % nx, v / nx);
        match d {
            Side::Right if i + 1 < nx && !self.hblock[v] => Some(v + 1),
            Side::Left if i > 0 && !self.hblock[v - 1] => Some(v - 1),
            Side::Bottom if j + 1 < self.ys.len() && !self.vblock[v] => Some(v + nx),
            Side::Top if j > 0 && !self.vblock[v - nx] => Some(v - nx),
            _ => None,
        }
    }
}

/// Segments already taken, each with its owner, so a later route pays to cross or run along them.
#[derive(Default)]
struct Taken {
    segs: Vec<(Point, Point, usize)>,
}

impl Taken {
    fn cost(&self, a: Point, b: Point, me: usize) -> i64 {
        let mut c = 0;
        for &(p, q, owner) in &self.segs {
            if owner == me {
                continue;
            }
            let overlap = |a1: i32, a2: i32, b1: i32, b2: i32| a1.max(a2).min(b1.max(b2)) > a1.min(a2).max(b1.min(b2));
            let horizontal = a.1 == b.1;
            if horizontal == (p.1 == q.1) {
                let stacked = if horizontal {
                    (a.1 - p.1).abs() < TRACK && overlap(a.0, b.0, p.0, q.0)
                } else {
                    (a.0 - p.0).abs() < TRACK && overlap(a.1, b.1, p.1, q.1)
                };
                c += if stacked { STACK } else { 0 };
            } else {
                // A crossing counts at the vertex where this move ends inside the other segment.
                let inside = |v: i32, s: i32, t: i32| v > s.min(t) && v < s.max(t);
                let crosses =
                    if horizontal { b.0 == p.0 && inside(b.1, p.1, q.1) } else { b.1 == p.1 && inside(b.0, p.0, q.0) };
                c += if crosses { CROSS } else { 0 };
            }
        }
        c
    }
}

/// A* from the stub `from` (leaving in `out`) to the stub `to` (arriving to enter in `inward`). Returns the cost and
/// the stub-to-stub points, or none when boxes wall the target off.
fn search(
    g: &Graph,
    taken: &Taken,
    me: usize,
    from: Point,
    out: Side,
    to: Point,
    inward: Side,
) -> Option<(i64, Vec<Point>)> {
    let (start, goal) = (g.vertex(from), g.vertex(to));
    let states = g.xs.len() * g.ys.len() * 4;
    let dir_index = |d: Side| DIRS.iter().position(|x| *x == d).expect("a side is a direction");
    let mut best = vec![i64::MAX; states];
    let mut came = vec![usize::MAX; states];
    let guess = |vertex: usize| {
        let at = g.point(vertex);
        i64::from((at.0 - to.0).abs() + (at.1 - to.1).abs())
    };
    let mut heap = BinaryHeap::new();
    let first = start * 4 + dir_index(out);
    best[first] = 0;
    heap.push(Reverse((guess(start), 0_i64, first)));
    while let Some(Reverse((_, cost, state))) = heap.pop() {
        if cost > best[state] {
            continue;
        }
        let (vertex, dir) = (state / 4, DIRS[state % 4]);
        if vertex == goal {
            let end = cost + if dir == inward { 0 } else { BEND };
            let mut path = vec![g.point(vertex)];
            let mut back = state;
            while back != first {
                back = came[back];
                path.push(g.point(back / 4));
            }
            path.reverse();
            return Some((end, path));
        }
        for turn in DIRS {
            if turn == opposite(dir) {
                continue;
            }
            let Some(next_vertex) = g.step(vertex, turn) else { continue };
            let (here, there) = (g.point(vertex), g.point(next_vertex));
            let len = i64::from((here.0 - there.0).abs() + (here.1 - there.1).abs());
            let next = cost + len + if turn == dir { 0 } else { BEND } + taken.cost(here, there, me);
            let next_state = next_vertex * 4 + dir_index(turn);
            if next < best[next_state] {
                best[next_state] = next;
                came[next_state] = state;
                heap.push(Reverse((next + guess(next_vertex), next, next_state)));
            }
        }
    }
    None
}

/// Free tracks in a band: its centre and every 12 px out from it, clear of the band's walls by `lo` and `hi`.
fn tracks(left: i32, right: i32, lo: i32, hi: i32, out: &mut Vec<i32>) {
    let (first, last) = (left + lo, right - hi);
    if last < first {
        return;
    }
    let centre = first.midpoint(last) / 4 * 4;
    out.push(centre);
    let mut k = TRACK;
    while centre - k >= first || centre + k <= last {
        out.extend([centre - k, centre + k].into_iter().filter(|v| (first..=last).contains(v)));
        k += TRACK;
    }
}

/// The candidate lines of one axis: tracks in each free band, split at group borders.
pub(crate) fn lines(bands: &[(i32, i32)], borders: &[i32], end: i32) -> Vec<i32> {
    let mut out = Vec::new();
    for &(a, b) in bands {
        let mut cuts: Vec<i32> = borders.iter().copied().filter(|v| *v > a && *v < b).collect();
        cuts.sort_unstable();
        let mut lo = (a, if a == 0 { 8 } else { 16 });
        for c in cuts {
            tracks(lo.0, c, lo.1, TRACK, &mut out);
            lo = (c, TRACK);
        }
        tracks(lo.0, b, lo.1, if b == end { 8 } else { 16 }, &mut out);
    }
    out
}

/// Routes every wanted edge in `order`. Returns per edge its points, port to port, and the sides it uses.
pub(crate) fn route(
    boxes: &[Rect],
    xs: &[i32],
    ys: &[i32],
    wants: &[Want],
    order: &[usize],
) -> Vec<(Vec<Point>, Side, Side)> {
    let sides = pick_sides(boxes, xs, ys, wants, order);
    let ends = spread(boxes, wants, &sides);
    let mut px = xs.to_vec();
    let mut py = ys.to_vec();
    // Every stub is taken from the start, so no route runs over an end that routes later.
    let mut taken = Taken::default();
    for (edge, &((port_a, stub_a), (port_b, stub_b))) in ends.iter().enumerate() {
        px.extend([port_a.0, stub_a.0, port_b.0, stub_b.0]);
        py.extend([port_a.1, stub_a.1, port_b.1, stub_b.1]);
        taken.segs.extend([(port_a, stub_a, edge), (stub_b, port_b, edge)]);
    }
    let graph = Graph::new(boxes, px, py);
    let mut out = vec![(Vec::new(), Side::Right, Side::Left); wants.len()];
    for &edge in order {
        let ((port_a, stub_a), (port_b, stub_b)) = ends[edge];
        let (side_a, side_b) = sides[edge];
        // With no clear path the route goes straight, and the rules name it.
        let mid = search(&graph, &taken, edge, stub_a, side_a, stub_b, opposite(side_b))
            .map_or_else(|| vec![stub_a, (stub_b.0, stub_a.1), stub_b], |(_, path)| path);
        let mut pts = vec![port_a];
        pts.extend(mid);
        pts.push(port_b);
        let pts = simplify(pts);
        taken.segs.extend(pts.windows(2).map(|w| (w[0], w[1], edge)));
        out[edge] = (pts, side_a, side_b);
    }
    out
}

/// The first pass: each edge's sides, from its cheapest route with every end where it would sit alone.
fn pick_sides(boxes: &[Rect], xs: &[i32], ys: &[i32], wants: &[Want], order: &[usize]) -> Vec<(Side, Side)> {
    let mut px = xs.to_vec();
    let mut py = ys.to_vec();
    for want in wants {
        for (mine, other) in [(want.from, want.to), (want.to, want.from)] {
            for side in DIRS {
                let (port_at, stub) = port(&boxes[mine], side, preferred(&boxes[mine], side, &boxes[other]));
                px.extend([port_at.0, stub.0]);
                py.extend([port_at.1, stub.1]);
            }
        }
    }
    let graph = Graph::new(boxes, px, py);
    let mut taken = Taken::default();
    let mut sides = vec![(Side::Right, Side::Left); wants.len()];
    for &edge in order {
        let want = &wants[edge];
        let (a, b) = (&boxes[want.from], &boxes[want.to]);
        let mut best: Option<(i64, (Side, Side), Vec<Point>)> = None;
        for (side_a, side_b) in options(a, b, want) {
            let (from, to) = (port(a, side_a, preferred(a, side_a, b)).1, port(b, side_b, preferred(b, side_b, a)).1);
            if let Some((cost, path)) = search(&graph, &taken, edge, from, side_a, to, opposite(side_b))
                && best.as_ref().is_none_or(|known| cost < known.0)
            {
                best = Some((cost, (side_a, side_b), path));
            }
        }
        if let Some((_, pair, path)) = best {
            sides[edge] = pair;
            taken.segs.extend(path.windows(2).map(|w| (w[0], w[1], edge)));
        }
    }
    sides
}

/// Each end's port and stub. Ends on one side sit in the order of where they want to be, at least [`EDGE`] apart and
/// [`EDGE`] from the corners, so they neither cross nor touch.
fn spread(boxes: &[Rect], wants: &[Want], sides: &[(Side, Side)]) -> Vec<((Point, Point), (Point, Point))> {
    // (box, side, wanted position, edge, which end)
    let mut on: Vec<(usize, usize, i32, usize, usize)> = Vec::new();
    for (edge, want) in wants.iter().enumerate() {
        for (end, mine, other, side) in [(0, want.from, want.to, sides[edge].0), (1, want.to, want.from, sides[edge].1)]
        {
            let dir = DIRS.iter().position(|x| *x == side).expect("a side is a direction");
            on.push((mine, dir, preferred(&boxes[mine], side, &boxes[other]), edge, end));
        }
    }
    on.sort_unstable();
    // Per end: where it sits, its side's free stretch, and whether it is alone on that side.
    let mut spots = vec![(0, (0, 0), false); wants.len() * 2];
    for run in on.chunk_by(|x, y| x.0 == y.0 && x.1 == y.1) {
        let (bx, side) = (&boxes[run[0].0], DIRS[run[0].1]);
        let (lo, hi) = if matches!(side, Side::Left | Side::Right) { (bx.y, bx.bottom()) } else { (bx.x, bx.right()) };
        let mut at: Vec<i32> = Vec::with_capacity(run.len());
        for end in run {
            at.push(at.last().map_or(end.2, |prev| end.2.max(prev + EDGE)));
        }
        let mut limit = (hi - EDGE) / 4 * 4;
        for spot in at.iter_mut().rev() {
            *spot = (*spot).min(limit).max((lo + EDGE).min(limit));
            limit = *spot - EDGE;
        }
        for (end, spot) in run.iter().zip(at) {
            spots[end.3 * 2 + end.4] = (spot, (lo + EDGE, hi - EDGE), run.len() == 1);
        }
    }
    // A crowded side may push an end off its partner's line; a lone partner on the facing side follows it, so the
    // route stays straight instead of jogging a few pixels.
    for (edge, &(a, b)) in sides.iter().enumerate() {
        let (one, two) = (spots[edge * 2], spots[edge * 2 + 1]);
        if b != opposite(a) || one.0 == two.0 {
            continue;
        }
        if two.2 && (two.1.0..=two.1.1).contains(&one.0) {
            spots[edge * 2 + 1].0 = one.0;
        } else if one.2 && (one.1.0..=one.1.1).contains(&two.0) {
            spots[edge * 2].0 = two.0;
        }
    }
    (0..wants.len())
        .map(|e| {
            let w = &wants[e];
            (port(&boxes[w.from], sides[e].0, spots[e * 2].0), port(&boxes[w.to], sides[e].1, spots[e * 2 + 1].0))
        })
        .collect()
}

/// Drops the middle point of every straight run.
pub(crate) fn simplify(pts: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last() == Some(&p) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            if (a.0 == b.0 && b.0 == p.0) || (a.1 == b.1 && b.1 == p.1) {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// The points where `b` crosses `a` inside both, for the bridges.
pub(crate) fn crossings(a: &[Point], b: &[Point]) -> Vec<Point> {
    let inside = |v: i32, s: i32, t: i32| v > s.min(t) && v < s.max(t);
    let mut out = Vec::new();
    for p in a.windows(2) {
        for q in b.windows(2) {
            let (h, v) = if p[0].1 == p[1].1 && q[0].0 == q[1].0 {
                (p, q)
            } else if p[0].0 == p[1].0 && q[0].1 == q[1].1 {
                (q, p)
            } else {
                continue;
            };
            if inside(v[0].0, h[0].0, h[1].0) && inside(h[0].1, v[0].1, v[1].1) {
                out.push((v[0].0, h[0].1));
            }
        }
    }
    out
}
