//! The hard rules (28, design change 4): each refuses with a finding that names its rule and element. Spec rules run
//! through `render`; geometric rules run through `lint` on a hand-made geometry, since `bp`'s own layout never
//! produces most of these faults.

use bp_diagram::geom::{Geometry, Label, Point, Rect, Route, Text};
use bp_diagram::{Finding, Link, Severity, lint, render};
use std::fmt::Write as _;

const ONE: &str = "type: architecture\nlevel: container\ntitle: T\nlede: One line.\n";

fn refused(spec: &str) -> Vec<Finding> {
    render(spec, None, None).expect_err("the spec breaks a rule")
}

fn names(findings: &[Finding], rule: &str, id: &str) -> bool {
    findings.iter().any(|f| f.rule == rule && f.id == id && f.severity == Severity::Error)
}

fn route(id: &str, pieces: Vec<Vec<Point>>) -> Route {
    Route { id: id.into(), from: "a".into(), to: "b".into(), pieces, bridges: Vec::new() }
}

/// Two boxes and a clean route between them; each test breaks one thing.
fn clean() -> Geometry {
    Geometry {
        boxes: vec![("a".into(), Rect::new(0, 0, 100, 60)), ("b".into(), Rect::new(200, 0, 100, 60))],
        routes: vec![route("a--b", vec![vec![(100, 32), (200, 32)]])],
        labels: Vec::new(),
        texts: Vec::new(),
    }
}

#[test]
fn a_clean_geometry_passes() {
    assert_eq!(lint(&clean()), []);
}

#[test]
fn refuses_overlap() {
    let f = refused(&format!("{ONE}nodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n  - {{id: b, noun: B, at: [0, 0]}}\n"));
    assert!(names(&f, "layout/overlap", "b"), "{f:?}");
}

#[test]
fn refuses_group_shape() {
    let spec = format!(
        "{ONE}groups:\n  - {{id: g, label: G, members: [a, c]}}\nnodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n  - {{id: b, noun: B, at: [1, 0]}}\n  - {{id: c, noun: C, at: [2, 0]}}\n"
    );
    assert!(names(&refused(&spec), "layout/group-shape", "g"));
    let holes = format!(
        "{ONE}groups:\n  - {{id: g, label: G, members: [a, c]}}\nnodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n  - {{id: c, noun: C, at: [1, 1]}}\n"
    );
    assert!(names(&refused(&holes), "layout/group-shape", "g"));
}

#[test]
fn refuses_diagonal() {
    let mut g = clean();
    g.routes[0].pieces = vec![vec![(100, 32), (200, 40)]];
    assert!(names(&lint(&g), "route/diagonal", "a--b"));
}

#[test]
fn refuses_through_box() {
    let mut g = clean();
    g.boxes.push(("c".into(), Rect::new(140, 0, 20, 60)));
    assert!(names(&lint(&g), "route/through-box", "a--b"));
}

#[test]
fn refuses_a_route_along_a_border() {
    let mut g = clean();
    g.boxes.push(("c".into(), Rect::new(120, 32, 40, 40)));
    let f = lint(&g);
    assert!(f.iter().any(|f| f.rule == "route/through-box" && f.detail.contains("`c`")), "{f:?}");
}

#[test]
fn refuses_touching_routes() {
    let mut g = clean();
    let mut t = route("c--b", vec![vec![(152, 100), (152, 32)]]);
    t.from = "c".into();
    g.routes.push(t);
    assert!(names(&lint(&g), "route/touches", "c--b"), "an end on another route");
    g.routes[1].pieces = vec![vec![(200, 32), (240, 32)]];
    assert!(names(&lint(&g), "route/touches", "c--b"), "two collinear runs that meet end to end");
}

#[test]
fn refuses_a_route_through_a_group_label() {
    let mut g = clean();
    g.boxes.push(("g:label".into(), Rect::new(120, 24, 48, 16)));
    let f = lint(&g);
    assert!(f.iter().any(|f| f.rule == "route/through-box" && f.detail.contains("`g:label`")), "{f:?}");
}

#[test]
fn refuses_an_edge_label_over_a_group_label() {
    let mut g = clean();
    g.boxes.push(("g:label".into(), Rect::new(120, 80, 48, 16)));
    g.labels.push(Label { id: "a--b:label".into(), of: "a--b".into(), rect: Rect::new(128, 72, 48, 24) });
    let f = lint(&g);
    assert!(f.iter().any(|f| f.rule == "label/collision" && f.detail.contains("`g:label`")), "{f:?}");
}

#[test]
fn refuses_stacked() {
    let mut g = clean();
    g.routes.push(route("a--b2", vec![vec![(100, 40), (200, 40)]]));
    assert!(names(&lint(&g), "route/stacked", "a--b2"));
}

#[test]
fn refuses_attach() {
    let mut g = clean();
    g.routes.push(route("a--b2", vec![vec![(100, 40), (148, 40), (148, 80), (200, 80)]]));
    g.boxes[1].1.h = 120;
    let f = lint(&g);
    assert!(names(&f, "route/attach", "a--b2"), "{f:?}");
    let mut corner = clean();
    corner.routes[0].pieces = vec![vec![(100, 4), (200, 4)]];
    assert!(names(&lint(&corner), "route/attach", "a--b"));
}

#[test]
fn refuses_crossing() {
    let mut g = clean();
    g.boxes.push(("c".into(), Rect::new(120, 100, 40, 40)));
    let mut cross = route("c--up", vec![vec![(140, 100), (140, -40)]]);
    cross.from = "c".into();
    cross.to = "z".into();
    g.routes.push(cross);
    assert!(names(&lint(&g), "route/crossing", "c--up"));
    g.routes[1].bridges.push((140, 32));
    assert!(!lint(&g).iter().any(|f| f.rule == "route/crossing"), "a bridge answers the crossing");
}

#[test]
fn refuses_label_gap() {
    let mut g = clean();
    g.labels.push(Label { id: "a--b:label".into(), of: "a--b".into(), rect: Rect::new(130, 12, 40, 18) });
    assert!(names(&lint(&g), "label/gap", "a--b:label"));
}

#[test]
fn refuses_label_collision() {
    let mut g = clean();
    g.labels.push(Label { id: "a--b:label".into(), of: "a--b".into(), rect: Rect::new(80, 0, 40, 16) });
    assert!(names(&lint(&g), "label/collision", "a--b:label"));
}

#[test]
fn refuses_text_overflow() {
    let mut g = clean();
    g.texts.push(Text { id: "a".into(), rect: Rect::new(16, 20, 120, 12), within: Rect::new(4, 4, 92, 52) });
    assert!(names(&lint(&g), "text/overflow", "a"));
}

#[test]
fn refuses_off_grid() {
    let mut g = clean();
    g.boxes[0].1.x = 3;
    assert!(names(&lint(&g), "grid/4px", "a"));
    g.labels.push(Label { id: "a--b:label".into(), of: "a--b".into(), rect: Rect::new(130, 80, 40, 18) });
    assert!(names(&lint(&g), "grid/4px", "a--b:label"), "a label sits on the grid too");
}

#[test]
fn refuses_a_zero_or_negative_size() {
    for size in ["[0, 1]", "[1, 0]", "[-1, 1]"] {
        let f = refused(&format!("{ONE}nodes:\n  - {{id: a, noun: A, at: [0, 0], span: {size}}}\n"));
        assert!(names(&f, "spec/schema", "a"), "{size}: {f:?}");
    }
    assert!(names(&refused(&format!("{ONE}nodes:\n  - {{id: a, noun: A, at: [-1, 0]}}\n")), "spec/schema", "a"));
}

#[test]
fn refuses_a_field_it_would_otherwise_drop() {
    let edge = |extra: &str| {
        format!(
            "{ONE}nodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n  - {{id: b, noun: B, at: [1, 0]}}\nedges:\n  - {{from: a, to: b, {extra}}}\n"
        )
    };
    assert!(names(&refused(&edge("both: \"yes\"")), "spec/schema", "a--b"), "both is a boolean");
    assert!(names(&refused(&edge("card: [\"1\", \"N\", \"M\"]")), "spec/schema", "a--b"), "card is two items");
    let both = format!("{ONE}nodes:\n  - {{id: a, noun: A, at: [0, 0], note: One., sub: Two}}\n");
    assert!(names(&refused(&both), "spec/schema", "a"), "note or sub, not both");
    let id = format!("{ONE}nodes:\n  - {{id: 'a\"><b', noun: A, at: [0, 0]}}\n");
    assert!(refused(&id).iter().any(|f| f.rule == "spec/schema" && f.detail.contains("no slug")), "ids are slugs");
}

#[test]
fn refuses_focus_count() {
    let spec = format!(
        "{ONE}focus: [a, b, c]\nnodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n  - {{id: b, noun: B, at: [1, 0]}}\n  - {{id: c, noun: C, at: [2, 0]}}\n"
    );
    assert!(names(&refused(&spec), "focus/count", "focus"));
}

#[test]
fn refuses_untruthful_legend() {
    let spec =
        format!("{ONE}nodes:\n  - {{id: a, noun: A, at: [0, 0]}}\nlegend:\n  - {{role: store, label: Data store}}\n");
    let f = refused(&spec);
    assert!(names(&f, "legend/truthful", "legend"), "{f:?}");
    assert_eq!(f.iter().filter(|f| f.rule == "legend/truthful").count(), 2, "one too many and one missing");
}

#[test]
fn refuses_missing_title() {
    let spec = "type: architecture\nlevel: container\ntitle: T\nnodes:\n  - {id: a, noun: A, at: [0, 0]}\n";
    assert!(names(&refused(spec), "a11y/title", "spec"));
}

#[test]
fn refuses_unresolved_term() {
    let spec =
        format!("{ONE}nodes:\n  - {{id: a, term: vault, at: [0, 0]}}\n  - {{id: b, term: quokka, at: [1, 0]}}\n");
    let known = |l: Link<'_>| l == Link::Term("vault");
    let f = render(&spec, None, Some(&known)).expect_err("quokka is no term");
    assert!(names(&f, "nodes/resolve", "b") && !names(&f, "nodes/resolve", "a"), "{f:?}");
    let neither = format!("{ONE}nodes:\n  - {{id: a, at: [0, 0]}}\n");
    assert!(names(&refused(&neither), "nodes/resolve", "a"));
}

#[test]
fn refuses_unknown_role() {
    let spec = format!("{ONE}nodes:\n  - {{id: a, noun: A, role: database, at: [0, 0]}}\n");
    assert!(names(&refused(&spec), "spec/schema", "a"));
}

#[test]
fn refuses_missing_endpoint() {
    let spec =
        format!("{ONE}nodes:\n  - {{id: a, noun: A, at: [0, 0]}}\nedges:\n  - {{from: a, to: ghost, label: uses}}\n");
    assert!(names(&refused(&spec), "spec/schema", "a--ghost"));
}

#[test]
fn a_budget_warns_and_still_renders() {
    let mut spec = format!("{ONE}nodes:\n");
    for i in 0..10 {
        let _ = writeln!(spec, "  - {{id: n{i}, noun: N{i}, at: [{}, {}]}}", i % 4, i / 4);
    }
    let r = render(&spec, None, None).expect("a budget does not refuse");
    assert!(r.findings.iter().any(|f| f.rule == "budget/architecture" && f.severity == Severity::Warn));
}
