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

/// A state machine whose entry, exit and branch name nothing: each state is a noun of its design.
const MACHINE: &str = "type: state\ntitle: An issue\nlede: An issue moves from open to resolved.\nnodes:\n  - {id: in, \
                       role: start}\n  - {id: open, noun: open}\n  - {id: which, role: choice, label: landed?}\n  - \
                       {id: resolved, noun: resolved}\n  - {id: out, role: end}\nedges:\n  - {from: in, to: open}\n  \
                       - {from: open, to: which, label: land}\n  - {from: which, to: resolved, label: yes}\n  - {from: \
                       which, to: open, label: no}\n  - {from: resolved, to: out}\n";

#[test]
fn a_start_end_or_branch_needs_no_link() {
    let known = |l: Link<'_>| matches!(l, Link::Noun("open" | "resolved"));
    let r = render(MACHINE, None, Some(&known)).expect("start, end and choice name nothing");
    assert!(r.findings.iter().all(|f| f.severity != Severity::Error), "{:?}", r.findings);
    let flow = "type: workflow\ntitle: Review\nlede: A change is reviewed.\nlanes:\n  - {id: h, label: Human}\nnodes:\n  \
                - {id: in, role: start, lane: h}\n  - {id: ok, role: decision, lane: h, label: approve?}\n  - {id: \
                out, role: end, lane: h}\nedges:\n  - {from: in, to: ok}\n  - {from: ok, to: out, label: yes}\n";
    render(flow, None, None).expect("start, end and decision name nothing");
    // A link given anyway is still resolved.
    let linked = MACHINE.replace("role: choice,", "role: choice, noun: verdict,");
    assert!(names(&render(&linked, None, Some(&known)).expect_err("no verdict noun"), "nodes/resolve", "which"));
}

#[test]
fn refuses_an_unlinked_state() {
    let f = refused(&MACHINE.replace("{id: open, noun: open}", "{id: open}"));
    assert!(names(&f, "nodes/resolve", "open") && f.len() == 1, "{f:?}");
    let both = MACHINE.replace("role: start}", "role: start, term: issue, noun: start}");
    assert!(names(&refused(&both), "nodes/resolve", "in"), "a start that gives two links");
}

#[test]
fn a_state_or_step_draws_its_note() {
    let noted = MACHINE.replace("{id: open, noun: open}", "{id: open, noun: open, note: Waits for a worker.}");
    let known = |_: Link<'_>| true;
    let html = render(&noted, None, Some(&known)).expect("a state takes a note").html;
    assert!(html.contains(">Waits for a worker.</text>"), "the note is drawn, not dropped");
    let flow = "type: workflow\ntitle: Review\nlede: A change is reviewed.\nlanes:\n  - {id: h, label: Human}\nnodes:\n  - {id: r, noun: \
                review, lane: h, note: By the owner.}\n";
    assert!(render(flow, None, None).expect("a step takes a note").html.contains(">By the owner.</text>"));
}

#[test]
fn refuses_a_note_on_a_start_end_or_branch() {
    let f = refused(&MACHINE.replace("role: choice,", "role: choice, note: Did it land?,"));
    assert!(
        f.iter().any(|f| f.rule == "spec/schema" && f.id == "which" && f.detail.contains("draws no `note`")),
        "{f:?}"
    );
    // A dot draws no text, so the fix names no `label`; the finding names the key the author wrote.
    let f = refused(&MACHINE.replace("role: start}", "role: start, sub: Filed.}"));
    let detail = &f.iter().find(|f| f.id == "in").expect("the start refuses").detail;
    assert_eq!(detail, "a `start` node draws no `sub`; say it in the design");
}

#[test]
fn refuses_a_label_on_a_start_or_end() {
    for (spec, id, role) in [
        (MACHINE.replace("role: start}", "role: start, label: Filed}"), "in", "start"),
        (MACHINE.replace("role: end}", "role: end, label: Done}"), "out", "end"),
    ] {
        let f = refused(&spec);
        let detail = &f.iter().find(|f| f.id == id).expect("the node refuses").detail;
        assert_eq!(detail, &format!("a `{role}` node draws no `label`; say it in the design"), "{f:?}");
    }
    // A choice draws its label.
    render(MACHINE, None, Some(&|_: Link<'_>| true)).expect("a choice keeps its label");
}

#[test]
fn a_crowded_side_gives_way_to_another_for_the_cards() {
    // Two cards of different edges would end at the left of `b`, a side that holds one card, not two. The diagonal
    // neighbour once took that side for its route and the second card found no spot.
    let spec = "type: er\nlevel: logical\ntitle: T\nlede: One line.\nnodes:\n  - {id: b, noun: B, role: record, at: [1, 1]}\n  - \
                {id: left, noun: Left, role: record, at: [0, 1]}\n  - {id: corner, noun: Corner, role: record, at: [0, 0]}\n  - {id: up, noun: Up, role: record, at: [1, 0]}\n\
                edges:\n  - {from: left, to: b, label: l, card: [\"1\", \"N\"]}\n  - {from: corner, to: b, label: c, card: \
                [\"1\", \"N\"]}\n";
    let r = render(spec, None, None).unwrap_or_else(|f| panic!("refused: {f:#?}"));
    assert!(r.findings.iter().all(|f| f.severity != Severity::Error), "{:?}", r.findings);
}

#[test]
fn a_side_without_cards_keeps_its_straight_route() {
    // A state box is too short to hold two ends 16 px apart, but without cards nothing needs that room: the line from
    // `a` to `b` keeps the route it had at master, from `a`'s right side to `b`'s left, whatever else ends at `a`'s right.
    let spec = "type: state\ntitle: T\nlede: One line.\nnodes:\n  - {id: a, noun: a}\n  - {id: b, noun: b}\nedges:\n  - {from: a, to: b, label: go}\n  - {from: a, to: a, label: again}\n";
    let page = render(spec, None, Some(&|_: Link<'_>| true)).expect("a self loop and a line render").html;
    let at = page.find("data-id=\"a--b\"").unwrap();
    let path = &page[at..];
    let path = &path[path.find(" d=\"").unwrap() + 4..];
    let path = &path[..path.find('"').unwrap()];
    assert!(
        path.starts_with("M128,84") && path.ends_with("L200,92"),
        "a--b leaves a's right side and enters b's left: {path}"
    );
}

#[test]
fn refuses_a_note_on_an_entity() {
    let spec = "type: er\nlevel: logical\ntitle: T\nlede: One entity.\nnodes:\n  - {id: a, noun: A, at: [0, 0], note: Dropped.}\n";
    let f = refused(spec);
    assert!(f.iter().any(|f| f.rule == "spec/schema" && f.id == "a" && f.detail.contains("draws no `note`")), "{f:?}");
    let f = refused(&spec.replace("note:", "sub:"));
    assert!(f.iter().any(|f| f.detail == "an `er` node draws no `sub`; use its `label`"), "{f:?}");
}

#[test]
fn refuses_a_legend_item_without_a_label() {
    let spec = format!("{ONE}legend:\n  - {{role: application}}\nnodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n");
    let f = refused(&spec);
    assert!(
        f.iter().any(|f| f.rule == "spec/schema" && f.id == "legend" && f.detail.contains("needs a `label`")),
        "{f:?}"
    );
    let empty = format!("{ONE}legend:\n  - {{}}\nnodes:\n  - {{id: a, noun: A, at: [0, 0]}}\n");
    let f = refused(&empty);
    assert!(f.iter().any(|f| f.detail.starts_with("legend item 1 needs a `role` or a `style`")), "{f:?}");
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
