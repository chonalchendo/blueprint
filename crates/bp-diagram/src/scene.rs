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

impl Scene {
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
