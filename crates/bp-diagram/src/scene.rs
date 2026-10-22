//! The laid-out diagram: every mark with its place, before it becomes SVG. The rules read its [`Geometry`].

use crate::font::Font;
use crate::geom::{Geometry, Label, Point, Rect, Route, Text};

/// One run of text. `y` is the baseline; a centred run has `x` at its middle.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) w: i32,
    pub(crate) font: Font,
    pub(crate) size: i32,
    pub(crate) track: i32,
    pub(crate) class: &'static str,
    pub(crate) text: String,
    pub(crate) mid: bool,
}

impl Run {
    pub(crate) fn new(font: Font, size: i32, track: i32, class: &'static str, text: impl Into<String>) -> Self {
        let text = text.into();
        Self { x: 0, y: 0, w: font.width(&text, size, track), font, size, track, class, text, mid: false }
    }

    pub(crate) fn at(mut self, x: i32, y: i32) -> Self {
        (self.x, self.y) = (x, y);
        self
    }

    pub(crate) fn centred(mut self, x: i32, y: i32) -> Self {
        (self.x, self.y, self.mid) = (x, y, true);
        self
    }

    /// The extent the overflow rule reads: cap height above the baseline, a quarter em below.
    pub(crate) fn rect(&self) -> Rect {
        let left = if self.mid { self.x - self.w / 2 } else { self.x };
        let cap = self.font.cap(self.size);
        Rect::new(left, self.y - cap, self.w, cap + self.size / 4)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Box,
    Pill,
    /// A box with the one-line pill's corner radius, for a state too tall to be a pill.
    Rounded,
    Dot,
    Ring,
    Diamond,
}

#[derive(Clone, Debug)]
pub(crate) struct SNode {
    pub(crate) id: String,
    pub(crate) role: String,
    pub(crate) term: Option<String>,
    pub(crate) noun: Option<String>,
    pub(crate) label: String,
    pub(crate) rect: Rect,
    pub(crate) shape: Shape,
    pub(crate) runs: Vec<Run>,
    /// Horizontal rules inside the box, as `y`.
    pub(crate) rules: Vec<i32>,
    /// A sequence party's lifeline, `(x, top, bottom)`, and its activation bars.
    pub(crate) lifeline: Option<(i32, i32, i32)>,
    pub(crate) bars: Vec<Rect>,
    pub(crate) class: Vec<&'static str>,
    /// The drill badge's count: the lower diagrams the caller named. Only architecture boxes and entities keep room
    /// for it at the top right.
    pub(crate) children: usize,
    /// The changed dot's centre `x`, from the box's left edge, in the room the name row keeps for it. None puts the
    /// dot in the top right corner.
    pub(crate) dot: Option<i32>,
}

/// A label: its mask and its runs. A pill draws its mask; a bare label masks with the canvas only.
#[derive(Clone, Debug)]
pub(crate) struct SLabel {
    pub(crate) id: String,
    pub(crate) rect: Rect,
    pub(crate) runs: Vec<Run>,
    pub(crate) pill: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SEdge {
    pub(crate) id: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) dashed: bool,
    /// Arrowheads: the tip and the unit direction it points.
    pub(crate) heads: Vec<(Point, Point)>,
    pub(crate) pieces: Vec<Vec<Point>>,
    pub(crate) bridges: Vec<Point>,
    pub(crate) labels: Vec<SLabel>,
    pub(crate) class: Vec<&'static str>,
    pub(crate) step: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct SGroup {
    pub(crate) id: String,
    pub(crate) rect: Rect,
    pub(crate) runs: Vec<Run>,
    pub(crate) members: Vec<String>,
    pub(crate) lane: bool,
}

impl SGroup {
    /// The label's extent, grown out to the 4 px grid: routes and edge labels keep off it, as off a box.
    pub(crate) fn label_box(&self) -> Rect {
        let r = self.runs.iter().map(Run::rect).reduce(|a, b| {
            let (x, y) = (a.x.min(b.x), a.y.min(b.y));
            Rect::new(x, y, a.right().max(b.right()) - x, a.bottom().max(b.bottom()) - y)
        });
        let r = r.unwrap_or_default();
        let (x, y) = (r.x.div_euclid(4) * 4, r.y.div_euclid(4) * 4);
        Rect::new(x, y, (r.right() - x + 3) / 4 * 4, (r.bottom() - y + 3) / 4 * 4)
    }
}

/// A sequence note: a box beside a lifeline that names a state effect.
#[derive(Clone, Debug)]
pub(crate) struct SNote {
    pub(crate) id: String,
    pub(crate) of: String,
    pub(crate) rect: Rect,
    pub(crate) runs: Vec<Run>,
    pub(crate) step: usize,
}

/// One legend entry: the swatch's kind, the tag its swatch shows when the role tags its nodes, and its words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) kind: String,
    pub(crate) tag: Option<String>,
    pub(crate) label: String,
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Scene {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) groups: Vec<SGroup>,
    pub(crate) edges: Vec<SEdge>,
    pub(crate) nodes: Vec<SNode>,
    pub(crate) notes: Vec<SNote>,
    pub(crate) legend: Vec<Key>,
    /// The top of the legend strip.
    pub(crate) legend_y: i32,
    /// Message ids in time order, for step-through.
    pub(crate) steps: Vec<String>,
}

/// The room between the drawing's outermost mark and its frame, the same on all four sides. The legend's hairline
/// and right edge keep the same distance.
pub(crate) const MARGIN: i32 = 24;

impl Scene {
    /// Sets the bounds to what the drawing covers: every box, group, lifeline, route piece and label mask, plus
    /// [`MARGIN`] on each side, and moves the marks so the drawing sits centred. Layout leaves an outer channel on
    /// each side that a route may or may not use, so only the marks say how far the drawing reaches. Nothing moves
    /// relative to anything else; every offset is a multiple of 4 because every mark's edge is.
    pub(crate) fn fit(&mut self) {
        let rects = self.marks();
        let (left, top) = rects.iter().fold((i32::MAX, i32::MAX), |(x, y), r| (x.min(r.x), y.min(r.y)));
        let (right, bottom) =
            rects.iter().fold((i32::MIN, i32::MIN), |(x, y), r| (x.max(r.right()), y.max(r.bottom())));
        let (dx, dy) = (MARGIN - left, MARGIN - top);
        for g in &mut self.groups {
            g.rect = move_rect(g.rect, dx, dy);
            g.runs.iter_mut().for_each(|r| move_run(r, dx, dy));
        }
        for n in &mut self.notes {
            n.rect = move_rect(n.rect, dx, dy);
            n.runs.iter_mut().for_each(|r| move_run(r, dx, dy));
        }
        for n in &mut self.nodes {
            crate::nodes::shift(n, dx, dy);
            n.lifeline = n.lifeline.map(|(x, a, b)| (x + dx, a + dy, b + dy));
            n.bars.iter_mut().for_each(|b| *b = move_rect(*b, dx, dy));
        }
        let at = |p: &mut Point| *p = (p.0 + dx, p.1 + dy);
        for e in &mut self.edges {
            e.pieces.iter_mut().flatten().for_each(at);
            e.bridges.iter_mut().for_each(at);
            for (tip, _) in &mut e.heads {
                at(tip);
            }
            for l in &mut e.labels {
                l.rect = move_rect(l.rect, dx, dy);
                l.runs.iter_mut().for_each(|r| move_run(r, dx, dy));
            }
        }
        (self.w, self.h) = (right - left + 2 * MARGIN, bottom - top + 2 * MARGIN);
        self.legend_y = self.h;
    }

    /// The extent of every mark that must sit inside the bounds: boxes, groups, lifelines, bars, route points and
    /// label masks.
    fn marks(&self) -> Vec<Rect> {
        let mut rects: Vec<Rect> =
            self.groups.iter().map(|g| g.rect).chain(self.notes.iter().map(|n| n.rect)).collect();
        for n in &self.nodes {
            rects.push(n.rect);
            rects.extend(n.lifeline.map(|(x, a, b)| Rect::of_segment((x, a), (x, b))));
            rects.extend(n.bars.iter().copied());
        }
        for e in &self.edges {
            rects.extend(e.labels.iter().map(|l| l.rect));
            rects.extend(e.pieces.iter().flatten().map(|p| Rect::new(p.0, p.1, 0, 0)));
        }
        rects
    }

    pub(crate) fn geometry(&self) -> Geometry {
        let mut g = Geometry::default();
        for n in &self.nodes {
            g.boxes.push((n.id.clone(), n.rect));
            let within = n.rect.inflate(-4);
            g.texts.extend(n.runs.iter().map(|r| Text { id: n.id.clone(), rect: r.rect(), within }));
        }
        for gr in self.groups.iter().filter(|gr| !gr.runs.is_empty()) {
            g.boxes.push((format!("{}:label", gr.id), gr.label_box()));
        }
        if !self.legend.is_empty() {
            g.boxes.push(("legend".into(), Rect::new(0, self.legend_y, self.w, self.h - self.legend_y)));
        }
        for n in &self.notes {
            g.boxes.push((n.id.clone(), n.rect));
            g.texts.extend(n.runs.iter().map(|r| Text { id: n.id.clone(), rect: r.rect(), within: n.rect }));
        }
        for e in &self.edges {
            g.routes.push(Route {
                id: e.id.clone(),
                from: e.from.clone(),
                to: e.to.clone(),
                pieces: e.pieces.clone(),
                bridges: e.bridges.clone(),
            });
            g.labels.extend(e.labels.iter().map(|l| Label { id: l.id.clone(), of: e.id.clone(), rect: l.rect }));
        }
        g
    }
}

fn move_rect(r: Rect, dx: i32, dy: i32) -> Rect {
    Rect::new(r.x + dx, r.y + dy, r.w, r.h)
}

fn move_run(r: &mut Run, dx: i32, dy: i32) {
    r.x += dx;
    r.y += dy;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::spec::{self, Type};

    /// `bp-wf-milestone`: two routes loop above the states, and their pills sat on the frame's top edge.
    const LOOPING_STATE: &str = "type: state\ntitle: Milestone\nlede: The human approves a shaped milestone.\nnodes:\n  - {id: start, noun: start, role: start}\n  - {id: planned, noun: planned}\n  - {id: approved, noun: approved}\n  - {id: done, noun: done}\nedges:\n  - {from: start, to: planned, label: \"shape · session proposes, human decides\"}\n  - {from: planned, to: approved, label: \"approve · human\"}\n  - {from: approved, to: planned, label: \"edit · end goal or finish line, the approval lapses\"}\n  - {id: stamp, from: approved, to: done, label: \"stamp · human, passes or waived\"}\n  - {id: again, from: approved, to: approved, label: \"stamp · human, finish line fails, run recorded\"}\n";

    /// Every example and the looping state: each mark lies inside the bounds, and the outermost marks sit exactly
    /// [`MARGIN`] from all four sides, so the drawing is centred.
    #[test]
    fn the_bounds_hold_every_mark_with_one_margin() {
        let dir = format!("{}/examples", env!("CARGO_MANIFEST_DIR"));
        let mut specs: Vec<(String, String)> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "yaml"))
            .map(|p| (p.display().to_string(), std::fs::read_to_string(&p).unwrap()))
            .collect();
        assert!(specs.len() >= 5, "the examples are found");
        specs.push(("looping state".into(), LOOPING_STATE.into()));
        for (name, text) in specs {
            let spec = spec::parse(&text).unwrap_or_else(|f| panic!("{name}: {f:?}"));
            let mut scene = match spec.ty {
                Type::Sequence => crate::seq::lay_out(&spec),
                _ => crate::layout::lay_out(&spec, &BTreeSet::new()).unwrap_or_else(|_| panic!("{name}: no layout")),
            };
            scene.fit();
            let inner = Rect::new(MARGIN, MARGIN, scene.w - 2 * MARGIN, scene.h - 2 * MARGIN);
            let marks = scene.marks();
            assert!(!marks.is_empty(), "{name}");
            for m in &marks {
                assert!(inner.holds(m), "{name}: {m:?} is outside {inner:?}");
            }
            let edge = |f: fn(&Rect) -> i32| marks.iter().map(f).collect::<Vec<_>>();
            assert_eq!(edge(|r| r.x).into_iter().min(), Some(MARGIN), "{name}: left");
            assert_eq!(edge(|r| r.y).into_iter().min(), Some(MARGIN), "{name}: top");
            assert_eq!(edge(Rect::right).into_iter().max(), Some(scene.w - MARGIN), "{name}: right");
            assert_eq!(edge(Rect::bottom).into_iter().max(), Some(scene.h - MARGIN), "{name}: bottom");
            assert_eq!(scene.legend_y, scene.h, "{name}: the legend starts below the drawing");
        }
    }
}
