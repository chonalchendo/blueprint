//! The scene as SVG, the view's contract (28, design change 12). Every node, group, edge, label and note carries
//! `data-id`; nodes carry `data-term` or `data-noun` and `data-role`; a `<metadata>` JSON index holds each id's
//! boxes, ports and label boxes. Colours are CSS variables; states are classes, so a view toggles and never redraws.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::geom::{Point, Rect};
use crate::scene::{Run, SLabel, SNode, Scene, Shape};

pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn run(out: &mut String, r: &Run) {
    let anchor = if r.mid { " text-anchor=\"middle\"" } else { "" };
    let track = if r.track > 0 { format!(" letter-spacing=\"{}\"", r.track) } else { String::new() };
    let _ = write!(
        out,
        "<text class=\"{} {}\" x=\"{}\" y=\"{}\" font-size=\"{}\"{track}{anchor}>",
        r.font.class(),
        r.class,
        r.x,
        r.y,
        r.size
    );
    // A field's leading marks (`#`, `→`) draw muted, so the name reads first.
    let mark = if r.class.starts_with("field") {
        r.text.chars().take_while(|c| matches!(c, '#' | '→' | ' ')).count()
    } else {
        0
    };
    if mark > 0 {
        let (m, rest) = r.text.split_at(r.text.char_indices().nth(mark).map_or(r.text.len(), |(i, _)| i));
        let _ = write!(out, "<tspan class=\"mark\">{}</tspan>{}", esc(m), esc(rest));
    } else {
        out.push_str(&esc(&r.text));
    }
    out.push_str("</text>");
}

fn rect(out: &mut String, class: &str, r: &Rect, rx: i32) {
    let _ = write!(
        out,
        "<rect class=\"{class}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{rx}\"/>",
        r.x, r.y, r.w, r.h
    );
}

/// A polyline as a path: quarter arcs of radius 8 at each corner, and a 6 px hop at each bridge.
fn path(pts: &[Point], bridges: &[Point]) -> String {
    let mut d = format!("M{},{}", pts[0].0, pts[0].1);
    let dir = |from: Point, to: Point| ((to.0 - from.0).signum(), (to.1 - from.1).signum());
    let len = |from: Point, to: Point| (from.0 - to.0).abs() + (from.1 - to.1).abs();
    let mut at = pts[0];
    for i in 1..pts.len() {
        let (start, corner) = (pts[i - 1], pts[i]);
        let u = dir(start, corner);
        let radius = if i + 1 < pts.len() { 8.min(len(start, corner) / 2).min(len(corner, pts[i + 1]) / 2) } else { 0 };
        let end = (corner.0 - u.0 * radius, corner.1 - u.1 * radius);
        let mut hops: Vec<Point> = bridges
            .iter()
            .copied()
            .filter(|hop| len(at, *hop) + len(*hop, end) == len(at, end) && len(at, *hop) >= 8 && len(*hop, end) >= 8)
            .collect();
        hops.sort_by_key(|hop| len(at, *hop));
        for hop in hops {
            let sweep = i32::from(u.0 > 0 || u.1 > 0);
            let _ = write!(
                d,
                "L{},{}A6,6 0 0 {sweep} {},{}",
                hop.0 - u.0 * 6,
                hop.1 - u.1 * 6,
                hop.0 + u.0 * 6,
                hop.1 + u.1 * 6
            );
        }
        let _ = write!(d, "L{},{}", end.0, end.1);
        at = end;
        if radius > 0 {
            let turn = dir(corner, pts[i + 1]);
            let sweep = i32::from(u.0 * turn.1 - u.1 * turn.0 > 0);
            at = (corner.0 + turn.0 * radius, corner.1 + turn.1 * radius);
            let _ = write!(d, "A{radius},{radius} 0 0 {sweep} {},{}", at.0, at.1);
        }
    }
    d
}

/// A stealth arrowhead: 8 px long, 6 px wide, notched.
fn arrow(tip: Point, u: Point) -> String {
    let (bx, by) = (tip.0 - u.0 * 8, tip.1 - u.1 * 8);
    let (nx, ny) = (tip.0 - u.0 * 6, tip.1 - u.1 * 6);
    let (px, py) = (-u.1 * 3, u.0 * 3);
    format!("M{},{}L{},{}L{},{}L{},{}Z", tip.0, tip.1, bx + px, by + py, nx, ny, bx - px, by - py)
}

/// The drill badge's box: at the top right, on the name's row.
fn drill_box(n: &SNode) -> Rect {
    let w = crate::nodes::drill_w(n.children);
    Rect::new(n.rect.right() - 12 - w, n.rect.y + 14, w, 18)
}

fn node(out: &mut String, n: &SNode, diff: &[&str]) {
    let mut class = vec!["node".to_string(), format!("role-{}", n.role)];
    class.extend(n.class.iter().chain(diff).map(|c| (*c).to_string()));
    if n.children > 0 {
        class.push("drill".into());
    }
    let link = match (&n.term, &n.noun) {
        (Some(t), _) => format!(" data-term=\"{}\"", esc(t)),
        (None, Some(noun)) => format!(" data-noun=\"{}\"", esc(noun)),
        _ => String::new(),
    };
    let _ = write!(
        out,
        "<g class=\"{}\" data-id=\"{}\"{link} data-role=\"{}\">",
        class.join(" "),
        esc(&n.id),
        esc(&n.role)
    );
    let r = n.rect;
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    match n.shape {
        Shape::Box => rect(out, "box", &r, 8),
        Shape::Pill => rect(out, "box", &r, r.h / 2),
        Shape::Dot => {
            let _ = write!(out, "<circle class=\"box solid\" cx=\"{cx}\" cy=\"{cy}\" r=\"{}\"/>", r.w / 2 - 2);
        }
        Shape::Ring => {
            let _ = write!(
                out,
                "<circle class=\"box\" cx=\"{cx}\" cy=\"{cy}\" r=\"{}\"/><circle class=\"solid\" cx=\"{cx}\" cy=\"{cy}\" r=\"6\"/>",
                r.w / 2
            );
        }
        Shape::Diamond => {
            let _ = write!(
                out,
                "<path class=\"box\" d=\"M{cx},{}L{},{cy}L{cx},{}L{},{cy}Z\"/>",
                r.y,
                r.right(),
                r.bottom(),
                r.x
            );
        }
    }
    for y in &n.rules {
        let _ = write!(out, "<path class=\"rule\" d=\"M{},{y}H{}\"/>", r.x, r.right());
    }
    for t in &n.runs {
        // The tag's pill reaches 6 px past its text; `nodes::title` counts that reach in the box width.
        if t.class == "tag" {
            rect(out, "tagbox", &Rect::new(t.x - 6, t.y - 13, t.w + 12, 18), 9);
        }
        run(out, t);
    }
    if n.children > 0 {
        let Rect { x: left, y: top, w: width, .. } = drill_box(n);
        let _ = write!(
            out,
            "<g class=\"badge\"><rect x=\"{left}\" y=\"{top}\" width=\"{width}\" height=\"18\" rx=\"9\"/><circle cx=\"{}\" cy=\"{}\" r=\"3\"/><path d=\"M{},{}L{},{}\"/><text class=\"t-mono\" x=\"{}\" y=\"{}\" font-size=\"10\">{}</text></g>",
            left + 10,
            top + 8,
            left + 12,
            top + 10,
            left + 15,
            top + 13,
            left + 18,
            top + 13,
            n.children
        );
    }
    if diff.contains(&"changed") {
        // On the name row in the room `nodes::title` kept; a box without that room takes it in its top right corner,
        // with a smaller dot 6 px clear of a centred name.
        let (cx, cy, radius) = n.dot.map_or((r.right() - 9, r.y + 8, 3), |dx| (r.x + dx, r.y + 23, 4));
        let _ = write!(out, "<circle class=\"dot\" cx=\"{cx}\" cy=\"{cy}\" r=\"{radius}\"/>");
    }
    out.push_str("</g>");
}

fn label(out: &mut String, l: &SLabel, of: &str, class: &[&str]) {
    let extra = class.iter().fold(String::new(), |mut s, c| {
        s.push(' ');
        s.push_str(c);
        s
    });
    let _ = write!(out, "<g class=\"label{extra}\" data-id=\"{}\" data-of=\"{}\">", esc(&l.id), esc(of));
    if l.pill {
        rect(out, "pill", &l.rect, 12);
    } else {
        rect(out, "mask", &l.rect, 2);
    }
    // A step number sits in a grey chip, the height of its label.
    for r in l.runs.iter().filter(|r| r.class == "chip") {
        rect(out, "chipbox", &Rect::new(r.x - 4, l.rect.y, r.w + 8, l.rect.h), 4);
    }
    for r in &l.runs {
        run(out, r);
    }
    out.push_str("</g>");
}

/// The SVG and its index. `classes` gives each id its diff classes.
pub(crate) fn svg(scene: &Scene, head: &crate::Head, classes: &dyn Fn(&str) -> Vec<&'static str>) -> String {
    let mut out = String::new();
    let kind = head.ty;
    let p = &head.prefix;
    let diff = if head.diff { " diff mode-diff" } else { "" };
    let _ = write!(
        out,
        "<svg class=\"bp bp-{kind}{diff}\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\" aria-labelledby=\"{p}-title {p}-desc\" data-type=\"{kind}\"{level}>",
        w = scene.w,
        h = scene.h,
        level = head.level.as_ref().map_or_else(String::new, |l| format!(" data-level=\"{l}\""))
    );
    let _ = write!(
        out,
        "<title id=\"{p}-title\">{}</title><desc id=\"{p}-desc\">{}</desc>",
        esc(&head.title),
        esc(&head.lede)
    );
    // Text content needs only `&`, `<` and `>` escaped, so the index stays JSON a plain reader can parse.
    let json = index(scene, head).to_string().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let _ = write!(out, "<metadata id=\"{p}-index\">{json}</metadata>");
    let _ = write!(
        out,
        "<rect class=\"bg\" width=\"{}\" height=\"{}\" rx=\"12\"/><g class=\"viewport\">",
        scene.w, scene.h
    );
    for g in &scene.groups {
        let lane = if g.lane { " lane" } else { "" };
        let _ = write!(out, "<g class=\"group{lane}\" data-id=\"{}\">", esc(&g.id));
        rect(&mut out, "zone", &g.rect, if g.lane { 6 } else { 10 });
        for r in &g.runs {
            run(&mut out, r);
        }
        out.push_str("</g>");
    }
    for n in scene.nodes.iter().filter(|n| n.lifeline.is_some()) {
        let (x, a, b) = n.lifeline.expect("filtered on lifeline");
        let _ = write!(
            out,
            "<g class=\"life\" data-id=\"{0}.life\" data-of=\"{0}\"><path class=\"lifeline\" d=\"M{x},{a}V{b}\"/>",
            esc(&n.id)
        );
        for bar in &n.bars {
            rect(&mut out, "bar", bar, 1);
        }
        out.push_str("</g>");
    }
    for e in &scene.edges {
        let mut class = vec!["edge"];
        class.extend(&e.class);
        if e.dashed {
            class.push("dashed");
        }
        class.extend(classes(&e.id));
        let step = e.step.map_or_else(String::new, |s| format!(" data-step=\"{s}\""));
        let _ = write!(
            out,
            "<g class=\"{}\" data-id=\"{}\" data-from=\"{}\" data-to=\"{}\"{step}>",
            class.join(" "),
            esc(&e.id),
            esc(&e.from),
            esc(&e.to)
        );
        for piece in &e.pieces {
            let _ = write!(out, "<path class=\"stroke\" d=\"{}\"/>", path(piece, &e.bridges));
        }
        for (tip, u) in &e.heads {
            let _ = write!(out, "<path class=\"head\" d=\"{}\"/>", arrow(*tip, *u));
        }
        out.push_str("</g>");
    }
    for e in &scene.edges {
        let mut class: Vec<&str> = e.class.clone();
        class.extend(classes(&e.id));
        for l in &e.labels {
            label(&mut out, l, &e.id, &class);
        }
    }
    for n in &scene.nodes {
        node(&mut out, n, &classes(&n.id));
    }
    for n in &scene.notes {
        let _ = write!(out, "<g class=\"note\" data-id=\"{}\" data-of=\"{}\">", esc(&n.id), esc(&n.of));
        rect(&mut out, "notebox", &n.rect, 6);
        for r in &n.runs {
            run(&mut out, r);
        }
        out.push_str("</g>");
    }
    out.push_str(&crate::legend::draw(scene));
    out.push_str("</g></svg>");
    out
}

fn bx(r: &Rect) -> Value {
    json!([r.x, r.y, r.w, r.h])
}

/// The JSON index: each id with its box, ports and label boxes, and the step order for a sequence.
fn index(scene: &Scene, head: &crate::Head) -> Value {
    // serde_json's `Map` is a `BTreeMap` while no crate turns on its `preserve_order` feature: the index's key order,
    // and so the page's bytes, then never follow insertion order or a hash seed.
    let mut ids = serde_json::Map::new();
    for g in &scene.groups {
        ids.insert(g.id.clone(), json!({"kind": "group", "box": bx(&g.rect), "members": g.members}));
    }
    for n in &scene.nodes {
        let ports: Vec<Value> = scene
            .edges
            .iter()
            .flat_map(|e| {
                let first = e.pieces.first().and_then(|p| p.first()).filter(|_| e.from == n.id);
                let last = e.pieces.last().and_then(|p| p.last()).filter(|_| e.to == n.id);
                first.into_iter().chain(last).map(move |p| json!([p.0, p.1, e.id]))
            })
            .collect();
        let mut v = json!({"kind": "node", "box": bx(&n.rect), "role": n.role, "label": n.label, "ports": ports});
        if n.children > 0 {
            v["children"] = n.children.into();
            v["badge"] = bx(&drill_box(n));
        }
        if let Some(t) = &n.term {
            v["term"] = t.clone().into();
        }
        if let Some(noun) = &n.noun {
            v["noun"] = noun.clone().into();
        }
        ids.insert(n.id.clone(), v);
        if n.lifeline.is_some() {
            ids.insert(format!("{}.life", n.id), json!({"kind": "life", "of": n.id}));
        }
    }
    for e in &scene.edges {
        let pts: Vec<Value> =
            e.pieces.iter().map(|p| json!(p.iter().map(|q| [q.0, q.1]).collect::<Vec<_>>())).collect();
        let labels: Vec<&str> = e.labels.iter().map(|l| l.id.as_str()).collect();
        let mut v = json!({"kind": "edge", "from": e.from, "to": e.to, "pieces": pts, "labels": labels});
        if let Some(s) = e.step {
            v["step"] = s.into();
        }
        ids.insert(e.id.clone(), v);
        for l in &e.labels {
            ids.insert(l.id.clone(), json!({"kind": "label", "of": e.id, "box": bx(&l.rect)}));
        }
    }
    for n in &scene.notes {
        ids.insert(n.id.clone(), json!({"kind": "note", "of": n.of, "box": bx(&n.rect), "step": n.step}));
    }
    json!({"type": head.ty, "level": head.level, "focus": head.focus, "steps": scene.steps, "ids": ids, "size": [scene.w, scene.h]})
}
