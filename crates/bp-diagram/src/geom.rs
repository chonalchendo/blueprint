//! The geometry the hard rules read: boxes, routes, labels and text runs, in whole pixels. The renderer builds it
//! from its own scene, and `examples/lint.rs` builds it from another tool's SVG, so one rule set judges both.

/// A point in whole pixels.
pub type Point = (i32, i32);

/// An axis-aligned rectangle; `w` and `h` are never negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    /// Left.
    pub x: i32,
    /// Top.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Rect {
    /// The rectangle from its edges.
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// The degenerate rectangle a segment covers.
    pub fn of_segment(a: Point, b: Point) -> Self {
        Self::new(a.0.min(b.0), a.1.min(b.1), (a.0 - b.0).abs(), (a.1 - b.1).abs())
    }

    /// The right edge.
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    /// The bottom edge.
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// The rectangle grown by `d` on every side.
    #[must_use]
    pub const fn inflate(&self, d: i32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + 2 * d, self.h + 2 * d)
    }

    /// Whether this closed rectangle (or segment) enters the open interior of the box `o`: touching its edge does
    /// not count.
    pub const fn meets(&self, o: &Self) -> bool {
        self.x < o.right() && self.right() > o.x && self.y < o.bottom() && self.bottom() > o.y
    }

    /// Whether `o` lies inside, edges included.
    pub const fn holds(&self, o: &Self) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    /// The squared distance between the two closed rectangles; 0 when they touch or meet.
    pub fn gap2(&self, o: &Self) -> i64 {
        let dx = i64::from((o.x - self.right()).max(self.x - o.right()).max(0));
        let dy = i64::from((o.y - self.bottom()).max(self.y - o.bottom()).max(0));
        dx * dx + dy * dy
    }
}

/// One edge's drawn route: one or more orthogonal polylines (a label on the line cuts the stroke in two).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Route {
    /// The edge id.
    pub id: String,
    /// The box the route leaves.
    pub from: String,
    /// The box the route enters.
    pub to: String,
    /// The drawn polylines, in order from `from` to `to`.
    pub pieces: Vec<Vec<Point>>,
    /// The points where this route hops over another.
    pub bridges: Vec<Point>,
}

impl Route {
    /// Every drawn segment.
    pub fn segments(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.pieces.iter().flat_map(|p| p.windows(2).map(|w| (w[0], w[1])))
    }
}

/// A label's box and the element it names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Label {
    /// The label's own id.
    pub id: String,
    /// The edge it labels.
    pub of: String,
    /// Its mask: the box no other mark may enter.
    pub rect: Rect,
}

/// A run of text and the box it must fit in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    /// The element the text belongs to.
    pub id: String,
    /// The measured extent.
    pub rect: Rect,
    /// The inner box of its element.
    pub within: Rect,
}

/// Everything the geometric rules read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Geometry {
    /// Nodes and notes, by id: no route passes through one.
    pub boxes: Vec<(String, Rect)>,
    /// Edge routes.
    pub routes: Vec<Route>,
    /// Edge labels.
    pub labels: Vec<Label>,
    /// Text runs inside boxes.
    pub texts: Vec<Text>,
}
