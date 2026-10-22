//! Edge labels (`m5-diagram-tools.md` §5.3, step 6). A label is a pill on its route's longest free straight run,
//! nearest the middle first; the stroke breaks 8 px before the pill. When no spot on the line is clear, it tries
//! beside the line, 8 px off it. Every mask sits on the 4 px grid. Each spot must keep clear of every box, every other label and every other route.

use crate::font::Font;
use crate::geom::{Point, Rect};
use crate::scene::{Run, SLabel};

const SLIDE: [i32; 7] = [50, 40, 60, 30, 70, 20, 80];
/// The stroke stays this far from a pill: 6 px, the rule's gap, rounded up to the grid.
pub(crate) const GAP: i32 = 8;
/// A cut keeps this far from a segment's ends: the arc of a corner, or an arrowhead.
const CLEAR: i32 = 12;

/// What a label must keep clear of.
pub(crate) struct Clear<'a> {
    /// The drawing above the legend: a label beside a route in the outer channel stays inside it.
    pub(crate) frame: Rect,
    pub(crate) boxes: &'a [Rect],
    /// The group frames: a pill never sits on a frame's edge, as it would read as the frame's own label.
    pub(crate) frames: &'a [Rect],
    pub(crate) routes: &'a [Vec<Point>],
    pub(crate) labels: Vec<Rect>,
}

impl Clear<'_> {
    /// Whether `rect` is clear of everything but route `own`, and at least [`GAP`] from `own` except on segment
    /// `skip`, the one it cuts.
    fn ok(&self, rect: &Rect, own: usize, skip: Option<usize>) -> bool {
        let mask = rect.inflate(4);
        let seg = |a: &Point, b: &Point| Rect::of_segment(*a, *b);
        self.frame.holds(rect)
            && !self.boxes.iter().any(|b| mask.meets(b))
            && !self.labels.iter().any(|l| mask.meets(l))
            && self.routes[own]
                .windows(2)
                .enumerate()
                .all(|(k, w)| Some(k) == skip || seg(&w[0], &w[1]).gap2(rect) >= i64::from(GAP * GAP))
            && self.apart(rect, own, 16)
    }

    /// Whether `rect`, with its mask, lies wholly inside every frame it meets or wholly outside it.
    fn off_frames(&self, rect: &Rect) -> bool {
        let mask = rect.inflate(4);
        self.frames.iter().all(|f| !mask.meets(&f.inflate(1)) || f.inflate(-1).holds(&mask))
    }

    /// Whether every route but `own` keeps a squared distance of at least `d2` from `rect`.
    fn apart(&self, rect: &Rect, own: usize, d2: i64) -> bool {
        self.routes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != own)
            .all(|(_, r)| r.windows(2).all(|w| Rect::of_segment(w[0], w[1]).gap2(rect) >= d2))
    }
}

/// A placed label: its mask and, when it sits on the line, the segment it cuts and the cut's span along it.
pub(crate) struct Spot {
    pub(crate) rect: Rect,
    pub(crate) cut: Option<(usize, i32, i32)>,
}

/// The pill for `text`: Inter 12 px with at least 8 px of padding, 24 px tall. Its width is a multiple of 8, so with
/// its centre on a grid line its edges, and the stroke's cut ends beyond them, land on the grid too.
pub(crate) fn pill(id: &str, text: &str) -> (SLabel, i32) {
    let run = Run::new(Font::Body, 12, 0, "elabel", text);
    let w = (run.w + 16 + 7) / 8 * 8;
    (SLabel { id: format!("{id}:label"), rect: Rect::new(0, 0, w, PILL_H), runs: vec![run], pill: true }, w)
}

/// A pill's height: half of it is a multiple of 4, so a pill centred on a line sits on the grid.
pub(crate) const PILL_H: i32 = 24;

/// The first clear spot for a `w`×`h` label on route `own`, or none.
pub(crate) fn place(pts: &[Point], own: usize, w: i32, h: i32, clear: &Clear<'_>) -> Option<Spot> {
    let mut segs: Vec<(usize, Point, Point)> = pts.windows(2).enumerate().map(|(k, s)| (k, s[0], s[1])).collect();
    let len = |a: Point, b: Point| (a.0 - b.0).abs() + (a.1 - b.1).abs();
    segs.sort_by_key(|&(k, a, b)| (-len(a, b), k));
    // On the line first, on any segment; then beside it.
    for &(k, a, b) in &segs {
        let horizontal = a.1 == b.1;
        let (lo, hi) = if horizontal { (a.0.min(b.0), a.0.max(b.0)) } else { (a.1.min(b.1), a.1.max(b.1)) };
        let along = if horizontal { w } else { h };
        let room = hi - lo - 2 * CLEAR - 2 * GAP - along;
        if room < 0 {
            continue;
        }
        for f in SLIDE {
            let c = (lo + CLEAR + GAP + along / 2 + room * f / 100) / 4 * 4;
            let rect = if horizontal {
                Rect::new(c - w / 2, a.1 - h / 2, w, h)
            } else {
                Rect::new(a.0 - w / 2, c - h / 2, w, h)
            };
            let (lo, hi) = (c - along / 2 - GAP, c + along / 2 + GAP);
            // The cut's ends keep a hop's width from any other route, so a route never seems to end at a gap.
            let gap = if horizontal { Rect::new(lo, a.1, hi - lo, 0) } else { Rect::new(a.0, lo, 0, hi - lo) };
            if clear.ok(&rect, own, Some(k)) && clear.off_frames(&rect) && clear.apart(&gap, own, i64::from(GAP * GAP))
            {
                return Some(Spot { rect, cut: Some((k, lo, hi)) });
            }
        }
    }
    for &(_, a, b) in &segs {
        let flat = a.1 == b.1;
        let line = if flat { a.1 } else { a.0 };
        // A frame beside the line may leave no room inside it: the pill then clears the frame's edge by its mask, so
        // those offsets come after the plain gap, which every slide position tries first.
        let mut offs = vec![GAP];
        for f in clear.frames {
            let edges = if flat { [f.y, f.bottom()] } else { [f.x, f.right()] };
            offs.extend(
                edges.map(|e| ((e - line).abs() + 8) / 4 * 4).into_iter().filter(|d| (GAP..=GAP + 24).contains(d)),
            );
        }
        offs.sort_unstable();
        offs.dedup();
        for d in offs {
            for f in SLIDE {
                let c = ((a.0 + (b.0 - a.0) * f / 100) / 4 * 4, (a.1 + (b.1 - a.1) * f / 100) / 4 * 4);
                let beside = if flat {
                    [Rect::new(c.0 - w / 2, c.1 - d - h, w, h), Rect::new(c.0 - w / 2, c.1 + d, w, h)]
                } else {
                    [Rect::new(c.0 + d, c.1 - h / 2, w, h), Rect::new(c.0 - d - w, c.1 - h / 2, w, h)]
                };
                if let Some(rect) = beside.into_iter().find(|r| clear.ok(r, own, None) && clear.off_frames(r)) {
                    return Some(Spot { rect, cut: None });
                }
            }
        }
    }
    None
}

/// A cardinality beside the end of a route: 12 px out from the port, 8 px off the line, on the first clear side,
/// and how many of the two sides were clear. A side that faces another end on the same side of the box comes last:
/// the narrow stretch between two ends holds one card, so a neighbour's card may need it. `w` is a multiple of 4.
pub(crate) fn card(pts: &[Point], at_start: bool, own: usize, w: i32, clear: &Clear<'_>) -> (Rect, usize) {
    let (end, next) = if at_start { (pts[0], pts[1]) } else { (pts[pts.len() - 1], pts[pts.len() - 2]) };
    let out = ((next.0 - end.0).signum(), (next.1 - end.1).signum());
    let tries = beside(end, out, w);
    let ends: Vec<Point> =
        clear.routes.iter().enumerate().filter(|(i, _)| *i != own).flat_map(|(_, r)| [r[0], r[r.len() - 1]]).collect();
    // The offset of a neighbour's end across the line: positive toward the first try's side.
    let toward =
        |p: &Point| if out.1 == 0 { (p.0 == end.0).then(|| end.1 - p.1) } else { (p.1 == end.1).then(|| p.0 - end.0) };
    let faces = |first: bool| {
        // Two cards between the lines need their gaps, their extents across the line, and a mask between them.
        let reach = 2 * (GAP + if out.1 == 0 { 12 } else { w }) + 4;
        ends.iter().filter_map(toward).any(|d| if first { d > 0 && d < reach } else { d < 0 && -d < reach })
    };
    let mut free: Vec<(bool, usize, Rect)> = tries
        .into_iter()
        .enumerate()
        .filter(|(_, r)| clear.ok(r, own, None))
        .map(|(k, r)| (faces(k == 0), k, r))
        .collect();
    free.sort_unstable_by_key(|x| (x.0, x.1));
    (free.first().map_or(tries[0], |x| x.2), free.len())
}

/// The two spots a `w` wide cardinality may take at the port `end`, whose route leaves in direction `out`. The
/// router makes other routes pay to cross either, so a card mostly finds one clear.
pub(crate) fn beside(end: Point, out: Point, w: i32) -> [Rect; 2] {
    let tall = 12;
    if out.1 == 0 {
        let left = if out.0 > 0 { end.0 + 12 } else { end.0 - 12 - w };
        [Rect::new(left, end.1 - GAP - tall, w, tall), Rect::new(left, end.1 + GAP, w, tall)]
    } else {
        let top = if out.1 > 0 { end.1 + 12 } else { end.1 - 12 - tall };
        [Rect::new(end.0 + GAP, top, w, tall), Rect::new(end.0 - GAP - w, top, w, tall)]
    }
}

/// The route's drawn pieces: whole, or in two with the cut left out.
pub(crate) fn cut(pts: &[Point], cut: Option<(usize, i32, i32)>) -> Vec<Vec<Point>> {
    let Some((k, lo, hi)) = cut else { return vec![pts.to_vec()] };
    let (a, b) = (pts[k], pts[k + 1]);
    let forward = if a.1 == b.1 { b.0 > a.0 } else { b.1 > a.1 };
    let at = |v: i32| if a.1 == b.1 { (v, a.1) } else { (a.0, v) };
    let (first_end, second_start) = if forward { (at(lo), at(hi)) } else { (at(hi), at(lo)) };
    let mut one = pts[..=k].to_vec();
    one.push(first_end);
    let mut two = vec![second_start];
    two.extend_from_slice(&pts[k + 1..]);
    vec![one, two]
}
