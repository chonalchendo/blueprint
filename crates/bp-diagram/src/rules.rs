//! The geometric hard rules (28, design change 4). Each reads only the [`Geometry`], so a layout bug in `bp` and a
//! defect in a hand-drawn SVG fail the same way. Each finding names its rule and the element.

use crate::Finding;
use crate::geom::{Geometry, Point, Rect, Route};

/// Two routes closer than this over a shared stretch are stacked; two ends on one side closer than this are one.
const TRACK: i32 = 12;
/// A label's mask keeps this far from its own stroke.
const LABEL_GAP: i64 = 6;
/// An end keeps this far from its box's corner.
const CORNER: i32 = 8;

pub fn check(g: &Geometry) -> Vec<Finding> {
    let mut out = Vec::new();
    for r in &g.routes {
        for (a, b) in r.segments() {
            if a.0 != b.0 && a.1 != b.1 {
                out.push(Finding::error("route/diagonal", &r.id, format!("segment {a:?} to {b:?} is diagonal")));
            }
            let seg = Rect::of_segment(a, b);
            for (id, rect) in &g.boxes {
                if *id != r.from && *id != r.to && seg.meets(rect) {
                    out.push(Finding::error("route/through-box", &r.id, format!("the route passes through `{id}`")));
                }
            }
        }
    }
    pairs(g, &mut out);
    attach(g, &mut out);
    labels(g, &mut out);
    for t in &g.texts {
        if !t.within.holds(&t.rect) {
            out.push(Finding::error("text/overflow", &t.id, format!("text {:?} does not fit {:?}", t.rect, t.within)));
        }
    }
    let off = |v: i32| v % 4 != 0;
    for (id, b) in &g.boxes {
        if off(b.x) || off(b.y) || off(b.w) || off(b.h) {
            out.push(Finding::error("grid/4px", id, format!("box {b:?} is off the 4 px grid")));
        }
    }
    for r in &g.routes {
        if let Some(p) = r.pieces.iter().flatten().chain(&r.bridges).find(|p| off(p.0) || off(p.1)) {
            out.push(Finding::error("grid/4px", &r.id, format!("point {p:?} is off the 4 px grid")));
        }
    }
    out
}

/// Stacked routes and unbridged crossings: each pair of routes once.
fn pairs(g: &Geometry, out: &mut Vec<Finding>) {
    for (i, a) in g.routes.iter().enumerate() {
        for b in &g.routes[i + 1..] {
            let mut stacked = false;
            let mut unbridged = None;
            for (p, q) in a.segments() {
                for (s, t) in b.segments() {
                    stacked |= parallel_overlap((p, q), (s, t));
                    if let Some(x) = crossing((p, q), (s, t))
                        && !a.bridges.contains(&x)
                        && !b.bridges.contains(&x)
                    {
                        unbridged.get_or_insert(x);
                    }
                }
            }
            if stacked {
                out.push(Finding::error("route/stacked", &b.id, format!("runs within {TRACK} px of `{}`", a.id)));
            }
            if let Some(x) = unbridged {
                out.push(Finding::error(
                    "route/crossing",
                    &b.id,
                    format!("crosses `{}` at {x:?} with no bridge", a.id),
                ));
            }
        }
    }
}

fn parallel_overlap((p, q): (Point, Point), (s, t): (Point, Point)) -> bool {
    let overlap = |a1: i32, a2: i32, b1: i32, b2: i32| a1.max(a2).min(b1.max(b2)) - a1.min(a2).max(b1.min(b2)) > 0;
    if p.1 == q.1 && s.1 == t.1 && p.0 != q.0 && s.0 != t.0 {
        (p.1 - s.1).abs() < TRACK && overlap(p.0, q.0, s.0, t.0)
    } else if p.0 == q.0 && s.0 == t.0 && p.1 != q.1 && s.1 != t.1 {
        (p.0 - s.0).abs() < TRACK && overlap(p.1, q.1, s.1, t.1)
    } else {
        false
    }
}

/// The point where a horizontal and a vertical segment cross inside both.
fn crossing(one: (Point, Point), two: (Point, Point)) -> Option<Point> {
    let inside = |v: i32, a: i32, b: i32| v > a.min(b) && v < a.max(b);
    let flat = |s: (Point, Point)| s.0.1 == s.1.1;
    let (across, down) = match (flat(one), flat(two)) {
        (true, false) => (one, two),
        (false, true) => (two, one),
        _ => return None,
    };
    (inside(down.0.0, across.0.0, across.1.0) && inside(across.0.1, down.0.1, down.1.1))
        .then_some((down.0.0, across.0.1))
}

/// Ends on one side of a box sit at least [`TRACK`] apart and [`CORNER`] from a corner.
fn attach(g: &Geometry, out: &mut Vec<Finding>) {
    let mut ends: Vec<(&str, &str, i32, &str)> = Vec::new();
    for r in &g.routes {
        let first = r.pieces.first().and_then(|p| p.first());
        let last = r.pieces.last().and_then(|p| p.last());
        for (end, owner) in [(first, &r.from), (last, &r.to)] {
            let Some(&(x, y)) = end else { continue };
            let Some((_, b)) = g.boxes.iter().find(|(id, _)| id == owner) else { continue };
            let side = if x == b.x || x == b.right() {
                (if x == b.x { "left" } else { "right" }, y, b.y, b.bottom())
            } else if y == b.y || y == b.bottom() {
                (if y == b.y { "top" } else { "bottom" }, x, b.x, b.right())
            } else {
                continue;
            };
            if side.1 - side.2 < CORNER || side.3 - side.1 < CORNER {
                out.push(Finding::error(
                    "route/attach",
                    &r.id,
                    format!("an end sits within {CORNER} px of a corner of `{owner}`"),
                ));
            }
            ends.push((owner, side.0, side.1, &r.id));
        }
    }
    ends.sort_unstable();
    for w in ends.windows(2) {
        if w[0].0 == w[1].0 && w[0].1 == w[1].1 && w[1].2 - w[0].2 < TRACK {
            out.push(Finding::error(
                "route/attach",
                w[1].3,
                format!("its end on the {} side of `{}` is within {TRACK} px of `{}`", w[1].1, w[1].0, w[0].3),
            ));
        }
    }
}

fn labels(g: &Geometry, out: &mut Vec<Finding>) {
    let near = |r: &Route, rect: &Rect, d2: i64| r.segments().any(|(a, b)| Rect::of_segment(a, b).gap2(rect) < d2);
    for (i, l) in g.labels.iter().enumerate() {
        if let Some(own) = g.routes.iter().find(|r| r.id == l.of)
            && near(own, &l.rect, LABEL_GAP * LABEL_GAP)
        {
            out.push(Finding::error(
                "label/gap",
                &l.id,
                format!("the mask sits within {LABEL_GAP} px of its own stroke"),
            ));
        }
        let mask = l.rect.inflate(2);
        let hit = g
            .boxes
            .iter()
            .find(|(_, b)| mask.meets(b))
            .map(|(id, _)| id.as_str())
            .or_else(|| {
                g.labels.iter().enumerate().find(|(j, o)| *j != i && mask.meets(&o.rect)).map(|(_, o)| o.id.as_str())
            })
            .or_else(|| g.routes.iter().find(|r| r.id != l.of && near(r, &mask, 1)).map(|r| r.id.as_str()));
        if let Some(hit) = hit {
            out.push(Finding::error("label/collision", &l.id, format!("the mask touches `{hit}`")));
        }
    }
}
