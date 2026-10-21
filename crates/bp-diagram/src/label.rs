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
    pub(crate) boxes: &'a [Rect],
    pub(crate) routes: &'a [Vec<Point>],
    pub(crate) labels: Vec<Rect>,
}

impl Clear<'_> {
    /// Whether `rect` is clear of everything but route `own`, and at least [`GAP`] from `own` except on segment
    /// `skip`, the one it cuts.
    fn ok(&self, rect: &Rect, own: usize, skip: Option<usize>) -> bool {
        let mask = rect.inflate(4);
        let seg = |a: &Point, b: &Point| Rect::of_segment(*a, *b);
        !self.boxes.iter().any(|b| mask.meets(b))
            && !self.labels.iter().any(|l| mask.meets(l))
            && self.routes.iter().enumerate().all(|(i, r)| {
                r.windows(2).enumerate().all(|(k, w)| {
                    let d2 = seg(&w[0], &w[1]).gap2(rect);
                    if i == own { Some(k) == skip || d2 >= i64::from(GAP * GAP) } else { d2 >= 16 }
                })
            })
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
            if clear.ok(&rect, own, Some(k)) {
                return Some(Spot { rect, cut: Some((k, c - along / 2 - GAP, c + along / 2 + GAP)) });
            }
        }
    }
    for &(_, a, b) in &segs {
        for f in SLIDE {
            let c = ((a.0 + (b.0 - a.0) * f / 100) / 4 * 4, (a.1 + (b.1 - a.1) * f / 100) / 4 * 4);
            let beside = if a.1 == b.1 {
                [Rect::new(c.0 - w / 2, c.1 - GAP - h, w, h), Rect::new(c.0 - w / 2, c.1 + GAP, w, h)]
            } else {
                [Rect::new(c.0 + GAP, c.1 - h / 2, w, h), Rect::new(c.0 - GAP - w, c.1 - h / 2, w, h)]
            };
            if let Some(rect) = beside.into_iter().find(|r| clear.ok(r, own, None)) {
                return Some(Spot { rect, cut: None });
            }
        }
    }
    None
}

/// A cardinality beside the end of a route: 12 px out from the port, 8 px off the line, on the first clear side,
/// and whether that side was clear. `w` is a multiple of 4.
pub(crate) fn card(pts: &[Point], at_start: bool, own: usize, w: i32, clear: &Clear<'_>) -> (Rect, bool) {
    let (end, next) = if at_start { (pts[0], pts[1]) } else { (pts[pts.len() - 1], pts[pts.len() - 2]) };
    let tall = 12;
    let out = ((next.0 - end.0).signum(), (next.1 - end.1).signum());
    let tries = if out.1 == 0 {
        let left = if out.0 > 0 { end.0 + 12 } else { end.0 - 12 - w };
        [Rect::new(left, end.1 - GAP - tall, w, tall), Rect::new(left, end.1 + GAP, w, tall)]
    } else {
        let top = if out.1 > 0 { end.1 + 12 } else { end.1 - 12 - tall };
        [Rect::new(end.0 + GAP, top, w, tall), Rect::new(end.0 - GAP - w, top, w, tall)]
    };
    tries.into_iter().find(|r| clear.ok(r, own, None)).map_or((tries[0], false), |r| (r, true))
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
