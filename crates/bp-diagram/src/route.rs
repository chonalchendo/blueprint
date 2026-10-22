//! The router (`m5-diagram-tools.md` §5.3, steps 3 and 4). Ends are ports spread along a side; routes are A* over
//! the gutter tracks and port lines (`search`). Tracks sit 12 px apart, so two routes in one gutter never stack.

use crate::geom::{Point, Rect};
use crate::label;
use crate::search::{Graph, STACK, Search, Taken};
use crate::spec::Side;

const STUB: i32 = 16;
/// An end keeps this far from its side's corners, and from the next end on the side.
const EDGE: i32 = 16;
const TRACK: i32 = 12;

/// One edge to route: the boxes it joins and the author's side hints.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Want {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) exit: Option<Side>,
    pub(crate) enter: Option<Side>,
    /// The width of the cardinality at each end; 0 for none.
    pub(crate) cards: [i32; 2],
}

pub(crate) const DIRS: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

pub(crate) const fn unit(s: Side) -> Point {
    match s {
        Side::Top => (0, -1),
        Side::Right => (1, 0),
        Side::Bottom => (0, 1),
        Side::Left => (-1, 0),
    }
}

pub(crate) const fn opposite(s: Side) -> Side {
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

/// Routes every wanted edge in `order`. Returns per edge its points, port to port. `sides` holds each edge's sides:
/// empty, they are picked and kept, so a later round, which only widens gutters, reuses them and skips the dearest
/// pass. Also returns the search's work, in states popped, which a layout bounds.
pub(crate) fn route(
    boxes: &[Rect],
    marks: &[Rect],
    xs: &[i32],
    ys: &[i32],
    wants: &[Want],
    order: &[usize],
    sides: &mut Vec<(Side, Side)>,
) -> (Vec<Vec<Point>>, usize) {
    // Group labels block a route as a box does, but take no ends.
    let walls: Vec<Rect> = boxes.iter().chain(marks).copied().collect();
    let mut work = 0;
    if sides.is_empty() {
        (*sides, work) = pick_sides(boxes, &walls, xs, ys, wants, order);
    }
    let sides: &[(Side, Side)] = sides;
    let ends = spread(boxes, wants, sides);
    let mut px = xs.to_vec();
    let mut py = ys.to_vec();
    // Every stub is taken from the start, so no route runs over an end that routes later.
    let mut taken = Taken::default();
    for (edge, &((port_a, stub_a), (port_b, stub_b))) in ends.iter().enumerate() {
        px.extend([port_a.0, stub_a.0, port_b.0, stub_b.0]);
        py.extend([port_a.1, stub_a.1, port_b.1, stub_b.1]);
        taken.add(port_a, stub_a, edge);
        taken.add(stub_b, port_b, edge);
    }
    // A cardinality takes one of two spots beside its port: a route through either, its mask included, pays as a
    // stack, so the card always finds one clear.
    let mut cards = Vec::new();
    for (edge, ((want, &(a, b)), &((port_a, _), (port_b, _)))) in wants.iter().zip(sides).zip(&ends).enumerate() {
        for (port, side, w) in [(port_a, a, want.cards[0]), (port_b, b, want.cards[1])] {
            if w > 0 {
                cards.extend(label::beside(port, unit(side), w).map(|r| (r.inflate(4), edge)));
            }
        }
    }
    let graph = Graph::new(&walls, &cards, px, py);
    let mut search = Search::new(&graph);
    let mut out = vec![Vec::new(); wants.len()];
    for &edge in order {
        let ((port_a, stub_a), (port_b, stub_b)) = ends[edge];
        let (side_a, side_b) = sides[edge];
        // With no clear path the route goes straight, and the rules name it.
        let mid = search
            .run(&taken, edge, (stub_a, side_a), (stub_b, opposite(side_b)), i64::MAX)
            .map_or_else(|| vec![stub_a, (stub_b.0, stub_a.1), stub_b], |(_, path)| path);
        let mut pts = vec![port_a];
        pts.extend(mid);
        pts.push(port_b);
        let pts = simplify(pts);
        for w in pts.windows(2) {
            taken.add(w[0], w[1], edge);
        }
        out[edge] = pts;
    }
    (out, work + search.pops)
}

/// The stretch a cardinality `card` wide needs along `side`, from its end's line: a card's height across a line that
/// leaves sideways, its width across one that leaves up or down, and its gap from the line.
fn need(side: Side, card: i32) -> i32 {
    match card {
        0 => 0,
        _ if matches!(side, Side::Left | Side::Right) => label::GAP + 12,
        w => label::GAP + w,
    }
}

/// How far apart each two neighbouring ends of one side sit: both their cardinalities between the lines, each with
/// its gap, and a mask between the two, but never closer than [`EDGE`].
fn gaps(needs: &[i32]) -> Vec<i32> {
    needs.windows(2).map(|w| (w[0] + w[1] + 4).max(EDGE)).collect()
}

/// The first pass: each edge's sides, from its cheapest route with every end where it would sit alone.
fn pick_sides(
    boxes: &[Rect],
    walls: &[Rect],
    xs: &[i32],
    ys: &[i32],
    wants: &[Want],
    order: &[usize],
) -> (Vec<(Side, Side)>, usize) {
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
    let graph = Graph::new(walls, &[], px, py);
    let mut search = Search::new(&graph);
    let mut taken = Taken::default();
    let mut sides = vec![(Side::Right, Side::Left); wants.len()];
    // The needs of the ends picked so far, by box and side: a side the cards no longer fit on costs as a stack.
    let mut used: Vec<(usize, Side, i32)> = Vec::new();
    let crowded = |used: &[(usize, Side, i32)], at: usize, side: Side, mine: i32| {
        let mut needs: Vec<i32> = used.iter().filter(|u| u.0 == at && u.1 == side).map(|u| u.2).collect();
        needs.push(mine);
        let len = if matches!(side, Side::Left | Side::Right) { boxes[at].h } else { boxes[at].w };
        gaps(&needs).iter().sum::<i32>() > len - 2 * EDGE
    };
    for &edge in order {
        let want = &wants[edge];
        let (a, b) = (&boxes[want.from], &boxes[want.to]);
        let mut best: Option<(i64, (Side, Side), Vec<Point>)> = None;
        for (side_a, side_b) in options(a, b, want) {
            let (from, to) = (port(a, side_a, preferred(a, side_a, b)).1, port(b, side_b, preferred(b, side_b, a)).1);
            // A side too crowded for its cards costs as much as a stack of routes.
            let toll = STACK
                * (i64::from(crowded(&used, want.from, side_a, need(side_a, want.cards[0])))
                    + i64::from(crowded(&used, want.to, side_b, need(side_b, want.cards[1]))));
            let limit = best.as_ref().map_or(i64::MAX, |known| known.0);
            if let Some((cost, path)) =
                search.run(&taken, edge, (from, side_a), (to, opposite(side_b)), limit.saturating_sub(toll))
                && cost + toll < limit
            {
                best = Some((cost + toll, (side_a, side_b), path));
            }
        }
        if let Some((_, pair, path)) = best {
            sides[edge] = pair;
            used.push((want.from, pair.0, need(pair.0, want.cards[0])));
            used.push((want.to, pair.1, need(pair.1, want.cards[1])));
            for w in path.windows(2) {
                taken.add(w[0], w[1], edge);
            }
        }
    }
    (sides, search.pops)
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
        // Either card may need its stretch when its own route turns away from the other side.
        let needs: Vec<i32> = run.iter().map(|end| need(side, wants[end.3].cards[end.4])).collect();
        let mut apart = gaps(&needs);
        // A crowded side shrinks the room evenly, never below [`EDGE`], so no end is pushed onto a corner.
        let (want, have) = (apart.iter().sum::<i32>(), hi - lo - 2 * EDGE);
        if want > have {
            for a in &mut apart {
                *a = (*a * have / want / 4 * 4).max(EDGE);
            }
        }
        let mut at: Vec<i32> = Vec::with_capacity(run.len());
        for (k, end) in run.iter().enumerate() {
            at.push(at.last().map_or(end.2, |prev| end.2.max(prev + apart[k - 1])));
        }
        let mut limit = (hi - EDGE) / 4 * 4;
        for (k, spot) in at.iter_mut().enumerate().rev() {
            *spot = (*spot).min(limit).max((lo + EDGE).min(limit));
            limit = *spot - if k > 0 { apart[k - 1] } else { 0 };
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
