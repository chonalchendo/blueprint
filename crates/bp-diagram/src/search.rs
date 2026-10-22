//! The router's search (`m5-diagram-tools.md` §5.3, step 4): A* over the gutter tracks and port lines, with cost
//! order: never through a box, then not along another route, then fewest crossings, then fewest bends, then length.
//! A large model has thousands of lines, so a box marks only the moves it blocks, taken segments are indexed by
//! their line, and one search's state is reset by what it touched, not reallocated.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::geom::{Point, Rect};
use crate::route::{DIRS, opposite};
use crate::spec::Side;

const TRACK: i32 = 12;
const BEND: i64 = 40;
const CROSS: i64 = 160;
pub(crate) const STACK: i64 = 4000;
const NONE: usize = usize::MAX;
const MANY: usize = usize::MAX - 1;

/// The search space: candidate x and y lines, and which moves a box blocks.
pub(crate) struct Graph {
    xs: Vec<i32>,
    ys: Vec<i32>,
    hblock: Vec<bool>,
    vblock: Vec<bool>,
    /// Moves through a cardinality's spot, by the spot's edge ([`NONE`], or [`MANY`] for spots of two edges): allowed
    /// at the cost of a stack, and free to the edge itself, whose card takes the other spot when its own route turns.
    hsoft: Vec<usize>,
    vsoft: Vec<usize>,
}

impl Graph {
    pub(crate) fn new(boxes: &[Rect], soft: &[(Rect, usize)], mut xs: Vec<i32>, mut ys: Vec<i32>) -> Self {
        xs.sort_unstable();
        xs.dedup();
        ys.sort_unstable();
        ys.dedup();
        let (nx, ny) = (xs.len(), ys.len());
        // The last move of a row or column leads nowhere.
        let mut hblock: Vec<bool> = (0..nx * ny).map(|v| v % nx + 1 == nx).collect();
        let mut vblock: Vec<bool> = (0..nx * ny).map(|v| v / nx + 1 == ny).collect();
        for w in boxes.iter().map(|b| b.inflate(8)) {
            mark(&xs, &ys, &w, &mut hblock, &mut vblock, |m| *m = true);
        }
        let (mut hsoft, mut vsoft) = (vec![NONE; nx * ny], vec![NONE; nx * ny]);
        for &(w, owner) in soft {
            let claim = |m: &mut usize| *m = if *m == NONE || *m == owner { owner } else { MANY };
            mark(&xs, &ys, &w, &mut hsoft, &mut vsoft, claim);
        }
        Self { xs, ys, hblock, vblock, hsoft, vsoft }
    }

    fn vertex(&self, p: Point) -> usize {
        let i = self.xs.binary_search(&p.0).expect("every stub lies on a candidate line");
        let j = self.ys.binary_search(&p.1).expect("every stub lies on a candidate line");
        j * self.xs.len() + i
    }

    fn point(&self, v: usize) -> Point {
        (self.xs[v % self.xs.len()], self.ys[v / self.xs.len()])
    }

    /// The vertex one step in `d` from `v`, unless a box blocks the move, and whether the move crosses a
    /// cardinality's spot.
    fn step(&self, v: usize, d: Side) -> Option<(usize, usize)> {
        let nx = self.xs.len();
        let (i, j) = (v % nx, v / nx);
        match d {
            Side::Right if i + 1 < nx && !self.hblock[v] => Some((v + 1, self.hsoft[v])),
            Side::Left if i > 0 && !self.hblock[v - 1] => Some((v - 1, self.hsoft[v - 1])),
            Side::Bottom if j + 1 < self.ys.len() && !self.vblock[v] => Some((v + nx, self.vsoft[v])),
            Side::Top if j > 0 && !self.vblock[v - nx] => Some((v - nx, self.vsoft[v - nx])),
            _ => None,
        }
    }
}

/// Marks each move whose closed segment enters the open interior of `w`: by binary search on the lines, so a box
/// costs the moves it meets, not the whole graph.
fn mark<T>(xs: &[i32], ys: &[i32], w: &Rect, h: &mut [T], v: &mut [T], set: impl Fn(&mut T)) {
    let (nx, ny) = (xs.len(), ys.len());
    let inside =
        |lines: &[i32], lo: i32, hi: i32| lines.partition_point(|x| *x <= lo)..lines.partition_point(|x| *x < hi);
    let across = |lines: &[i32], lo: i32, hi: i32| {
        lines.partition_point(|x| *x < lo).saturating_sub(1)..lines.partition_point(|x| *x < hi)
    };
    for j in inside(ys, w.y, w.bottom()) {
        for i in across(xs, w.x, w.right()).filter(|i| i + 1 < nx && xs[i + 1] > w.x) {
            set(&mut h[j * nx + i]);
        }
    }
    for i in inside(xs, w.x, w.right()) {
        for j in across(ys, w.y, w.bottom()).filter(|j| j + 1 < ny && ys[j + 1] > w.y) {
            set(&mut v[j * nx + i]);
        }
    }
}

/// Segments already taken, each with its owner, so a later route pays to cross or run along them. Horizontal ones
/// are keyed by their `y`, vertical ones by their `x`: a move meets only the lines near its own.
#[derive(Default)]
pub(crate) struct Taken {
    flat: BTreeMap<i32, Vec<(i32, i32, usize)>>,
    upright: BTreeMap<i32, Vec<(i32, i32, usize)>>,
    /// Counts the adds, so a search knows when the costs it remembers went stale.
    adds: usize,
}

impl Taken {
    pub(crate) fn add(&mut self, p: Point, q: Point, owner: usize) {
        self.adds += 1;
        if p.1 == q.1 {
            self.flat.entry(p.1).or_default().push((p.0, q.0, owner));
        } else {
            self.upright.entry(p.0).or_default().push((p.1, q.1, owner));
        }
    }

    /// What the move from `here` to `there` pays for the other routes' segments: a stack for running along one
    /// within a track or meeting it end to end, or for landing on its corner or end (a shared point that is no
    /// crossing reads as one line); a crossing for ending inside one that runs across.
    fn cost(&self, here: Point, there: Point, me: usize) -> i64 {
        let horizontal = here.1 == there.1;
        // In the move's own frame: the line it runs on, and its two ends along that line.
        let (on, first, last) = if horizontal { (here.1, here.0, there.0) } else { (here.0, here.1, there.1) };
        let (along, across) = if horizontal { (&self.flat, &self.upright) } else { (&self.upright, &self.flat) };
        let overlap = |a1: i32, a2: i32, b1: i32, b2: i32| a1.max(a2).min(b1.max(b2)) - a1.min(a2).max(b1.min(b2));
        let mut toll = 0;
        for (&line, segs) in along.range(on - TRACK + 1..on + TRACK) {
            let apart = (on - line).abs();
            for &(lo, hi, _) in segs.iter().filter(|x| x.2 != me) {
                let shared = overlap(first, last, lo, hi);
                let end = apart == 0 && (last == lo || last == hi);
                toll += if end || (shared > 0) || (apart == 0 && shared >= 0) { STACK } else { 0 };
            }
        }
        if let Some(segs) = across.get(&last) {
            for &(lo, hi, _) in segs.iter().filter(|x| x.2 != me) {
                toll += if on == lo || on == hi {
                    STACK
                } else if on > lo.min(hi) && on < lo.max(hi) {
                    CROSS
                } else {
                    0
                };
            }
        }
        toll
    }
}

/// One A* at a time over a graph, with its per-state tables kept between searches.
pub(crate) struct Search<'g> {
    g: &'g Graph,
    best: Vec<i64>,
    came: Vec<usize>,
    touched: Vec<usize>,
    /// Each move's cost against the taken segments, by its end state, and the generation it was computed in. The
    /// side options of one edge search the same moves again and again; a new owner or a new segment starts a new
    /// generation.
    costs: Vec<(u32, i64)>,
    generation: (u32, usize, usize),
    /// The states popped over every search: the work a layout bounds.
    pub(crate) pops: usize,
}

impl<'g> Search<'g> {
    pub(crate) fn new(g: &'g Graph) -> Self {
        let states = g.xs.len() * g.ys.len() * 4;
        Self {
            g,
            best: vec![i64::MAX; states],
            came: vec![usize::MAX; states],
            touched: Vec::new(),
            costs: vec![(0, 0); states],
            generation: (0, usize::MAX, usize::MAX),
            pops: 0,
        }
    }

    /// A* from the stub `from` (leaving in its side) to the stub `to` (arriving to enter in its side). Returns the cost
    /// and the stub-to-stub points, or none when boxes wall the target off or no route costs less than `limit`: a
    /// caller that already holds a route of that cost stops the search there instead of exploring the whole graph.
    pub(crate) fn run(
        &mut self,
        taken: &Taken,
        me: usize,
        (from, out): (Point, Side),
        (to, inward): (Point, Side),
        limit: i64,
    ) -> Option<(i64, Vec<Point>)> {
        if (self.generation.1, self.generation.2) != (taken.adds, me) {
            self.generation = (self.generation.0 + 1, taken.adds, me);
        }
        let generation = self.generation.0;
        for s in self.touched.drain(..) {
            self.best[s] = i64::MAX;
            self.came[s] = usize::MAX;
        }
        let g = self.g;
        let (start, goal) = (g.vertex(from), g.vertex(to));
        let dir_index = |d: Side| DIRS.iter().position(|x| *x == d).expect("a side is a direction");
        // The length still to go, and a bend when the goal lies off the current line or behind it.
        let guess = |vertex: usize, dir: Side| {
            let at = g.point(vertex);
            let (dx, dy) = (to.0 - at.0, to.1 - at.1);
            let u = crate::route::unit(dir);
            let ahead = (dx == 0 || dx.signum() == u.0) && (dy == 0 || dy.signum() == u.1) && (dx == 0 || dy == 0);
            i64::from(dx.abs() + dy.abs()) + if (dx, dy) != (0, 0) && !ahead { BEND } else { 0 }
        };
        let mut heap = BinaryHeap::new();
        let first = start * 4 + dir_index(out);
        self.best[first] = 0;
        self.touched.push(first);
        heap.push(Reverse((guess(start, out), 0_i64, first)));
        while let Some(Reverse((f, cost, state))) = heap.pop() {
            self.pops += 1;
            if f >= limit {
                return None;
            }
            if cost > self.best[state] {
                continue;
            }
            let (vertex, dir) = (state / 4, DIRS[state % 4]);
            if vertex == goal {
                let end = cost + if dir == inward { 0 } else { BEND };
                let mut path = vec![g.point(vertex)];
                let mut back = state;
                while back != first {
                    back = self.came[back];
                    path.push(g.point(back / 4));
                }
                path.reverse();
                return Some((end, path));
            }
            for turn in DIRS {
                if turn == opposite(dir) {
                    continue;
                }
                let Some((next_vertex, soft)) = g.step(vertex, turn) else { continue };
                let (here, there) = (g.point(vertex), g.point(next_vertex));
                let len = i64::from((here.0 - there.0).abs() + (here.1 - there.1).abs());
                let next_state = next_vertex * 4 + dir_index(turn);
                let known = self.costs[next_state];
                let toll = if known.0 == generation {
                    known.1
                } else {
                    let toll = if soft == NONE || soft == me { 0 } else { STACK } + taken.cost(here, there, me);
                    self.costs[next_state] = (generation, toll);
                    toll
                };
                let next = cost + len + if turn == dir { 0 } else { BEND } + toll;
                if next < self.best[next_state] {
                    if self.best[next_state] == i64::MAX {
                        self.touched.push(next_state);
                    }
                    self.best[next_state] = next;
                    self.came[next_state] = state;
                    heap.push(Reverse((next + guess(next_vertex, turn), next, next_state)));
                }
            }
        }
        None
    }
}
