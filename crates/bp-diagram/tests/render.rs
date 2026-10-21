//! The renderer's contract: the examples render clean, twice to the same bytes, with the SVG contract the view
//! reads (28, design change 12); a diff marks what changed; state and workflow render from order.

use std::collections::BTreeSet;

use bp_diagram::{Link, render, render_with};

fn example(name: &str) -> String {
    std::fs::read_to_string(format!("{}/examples/{name}.yaml", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn html(spec: &str, base: Option<&str>) -> String {
    match render(spec, base, None) {
        Ok(r) => r.html,
        Err(f) => panic!("refused: {f:#?}"),
    }
}

/// The ids every `data-id` names, and the ids the metadata index holds.
fn ids(html: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let attrs = html.split("data-id=\"").skip(1).map(|s| s[..s.find('"').unwrap()].to_string()).collect();
    let json = &html[html.find("<metadata").unwrap()..];
    let json = &json[json.find('>').unwrap() + 1..json.find("</metadata>").unwrap()];
    let meta: serde_json::Value = serde_json::from_str(json).unwrap();
    (attrs, meta["ids"].as_object().unwrap().keys().cloned().collect())
}

#[test]
fn the_three_render_clean_and_identically() {
    for name in ["bp-containers", "bp-lm-design", "bp-seq-issue-to-landed"] {
        let spec = example(name);
        let one = render(&spec, None, None).unwrap_or_else(|f| panic!("{name}: {f:#?}"));
        assert!(one.findings.iter().all(|f| f.rule.starts_with("budget/")), "{name}: {:?}", one.findings);
        assert_eq!(one.html, html(&spec, None), "{name} renders the same bytes twice");
        let lower = one.html.to_lowercase();
        assert!(
            !lower.contains("http://") && !lower.contains("https://") && !lower.contains("src="),
            "{name} loads nothing"
        );
        assert_eq!(lower.matches("<script>").count(), 1, "{name} inlines one viewer");
        let (attrs, index) = ids(&one.html);
        assert!(!attrs.is_empty());
        assert_eq!(attrs, index, "{name}: every data-id is in the index and back");
        assert!(one.html.contains("prefers-color-scheme: dark") && one.html.contains("var(--"));
    }
}

#[test]
fn nodes_carry_their_link_and_role() {
    let page = html(&example("bp-containers"), None);
    assert!(page.contains("data-id=\"tool\" data-noun=\"Tool\" data-role=\"application\""));
    assert!(page.contains("data-id=\"harness\" data-term=\"harness\" data-role=\"external\""));
    let seq = html(&example("bp-seq-issue-to-landed"), None);
    assert!(seq.contains("data-step=\"1\"") && seq.contains("\"steps\":[\"walk\",\"walk.back\""));
}

#[test]
fn the_diff_marks_added_and_removed_and_keeps_the_ghost() {
    let page = html(&example("bp-containers-changed"), Some(&example("bp-containers")));
    assert!(page.contains("class=\"node role-external added\" data-id=\"forge\""), "forge is added");
    assert!(
        page.contains("class=\"node role-external removed\" data-id=\"repository\""),
        "repository stays as a ghost"
    );
    assert!(page.contains("class=\"edge removed\" data-id=\"tool--repository\""));
    assert!(page.contains("class=\"bp bp-architecture diff mode-diff\""));
    // The legend lists the states the drawing uses, and no other (`legend/truthful`).
    let keys: Vec<&str> = ["added", "removed", "changed", "moved"]
        .into_iter()
        .filter(|s| page.contains(&format!("swatch key-{s}")))
        .collect();
    assert_eq!(keys, ["added", "removed"]);
    let note = example("bp-containers").replace("note: Skills and roles", "note: Skills and all roles");
    let changed = html(&note, Some(&example("bp-containers")));
    let keys: Vec<&str> = ["added", "removed", "changed", "moved"]
        .into_iter()
        .filter(|s| changed.contains(&format!("swatch key-{s}")))
        .collect();
    assert_eq!(keys, ["changed"], "a changed note draws the dot and lists only `changed`");
}

/// The drill badge is the caller's input, never the spec's (28, design change 12): the preview draws none, and a
/// count from the vault draws one with the count in the index.
#[test]
fn a_drill_badge_comes_from_the_caller() {
    let spec = example("bp-containers");
    assert!(!html(&spec, None).contains("<g class=\"badge\">"));
    let counts = |l: Link<'_>| if l == Link::Noun("Tool") { 2 } else { 0 };
    let page = render_with(&spec, None, None, &counts).unwrap_or_else(|f| panic!("refused: {f:#?}")).html;
    assert!(page.contains("class=\"node role-application focus drill\" data-id=\"tool\""));
    assert_eq!(page.matches("<g class=\"badge\">").count(), 1);
    assert!(
        page.contains("\"children\":2") && page.contains("\"badge\":["),
        "the index holds the count and the badge box"
    );
}

#[test]
fn state_and_workflow_render_from_order() {
    let state = "type: state\ntitle: An issue\nlede: An issue moves from open to resolved.\nfocus: [resolved]\nnodes:\n  - {id: start, noun: start, role: start}\n  - {id: open, term: issue, label: open}\n  - {id: in-progress, noun: in-progress, label: in progress}\n  - {id: resolved, noun: resolved}\n  - {id: escalated, noun: escalated}\n  - {id: end, noun: end, role: end}\nedges:\n  - {from: start, to: open}\n  - {from: open, to: in-progress, label: cut}\n  - {from: in-progress, to: resolved, label: land}\n  - {from: in-progress, to: escalated, label: escalate}\n  - {from: escalated, to: open, label: reopen}\n  - {from: resolved, to: end}\n";
    html(state, None);
    let flow = "type: workflow\ntitle: Review\nlede: A pull request goes from opened to landed.\nlanes:\n  - {id: worker, label: Worker}\n  - {id: human, label: Human}\nnodes:\n  - {id: open, noun: open, lane: worker, label: open pull request}\n  - {id: review, noun: review, lane: human, label: review}\n  - {id: respond, noun: respond, lane: worker}\n  - {id: land, noun: land, lane: human}\nedges:\n  - {from: open, to: review}\n  - {from: review, to: respond, label: findings}\n  - {from: respond, to: land}\n";
    html(flow, None);
}

#[test]
fn a_sequence_diff_keeps_removed_steps_and_parties_as_ghosts() {
    let head = example("bp-seq-issue-to-landed");
    let base = head.replace(
        "  - {id: forge, term: forge, label: Forge, sub: github}\n",
        "  - {id: forge, term: forge, label: Forge, sub: github}\n  - {id: ci, noun: CI, sub: runner}\n",
    );
    let base = base.replace("  - {id: build,", "  - {id: lint, from: worker, to: worker, say: lint}\n  - {id: build,");
    let page = html(&head, Some(&base));
    assert!(page.contains("removed\" data-id=\"ci\""), "the removed party stays as a ghost");
    assert!(page.contains("class=\"edge call removed\" data-id=\"lint\""), "the removed step stays in its place");
    assert!(page.contains("\"steps\":[\"walk\",\"walk.back\",\"dispatch\",\"cut\",\"lint\",\"build\""));
}

/// The review of PR 42 planted two routes that met end to end under a long group label; they now route apart, and
/// the render's own rules (`route/touches`, the label as an obstacle) passed it.
#[test]
fn routes_keep_apart_and_off_a_group_label() {
    let spec = "type: architecture\nlevel: container\ntitle: T\nlede: One line.\ngroups:\n  - {id: g, label: A rather long group label that spans the top, members: [m1, m2]}\nnodes:\n  - {id: x, noun: X, role: external, at: [0, 0]}\n  - {id: m1, noun: M1, at: [0, 1]}\n  - {id: m2, noun: M2, at: [1, 1]}\n  - {id: y, noun: Y, role: external, at: [1, 0]}\nedges:\n  - {from: x, to: m2, label: calls, exit: bottom, enter: top}\n  - {from: y, to: m1, label: reads, exit: bottom, enter: top}\n";
    let page = html(spec, None);
    let json = &page[page.find("<metadata").unwrap()..];
    let json = &json[json.find('>').unwrap() + 1..json.find("</metadata>").unwrap()];
    let meta: serde_json::Value = serde_json::from_str(json).unwrap();
    let points = |id: &str| -> BTreeSet<(i64, i64)> {
        let pieces = meta["ids"][id]["pieces"].as_array().unwrap();
        let pts = pieces
            .iter()
            .flat_map(|p| p.as_array().unwrap().iter().map(|q| (q[0].as_i64().unwrap(), q[1].as_i64().unwrap())));
        pts.collect()
    };
    assert!(points("x--m2").is_disjoint(&points("y--m1")), "no corner or end is shared");
}

/// FNV-1a: a fixed hash of the bytes, so a platform that renders other bytes fails here (28, design change 3).
#[test]
fn the_bytes_are_the_same_on_every_platform() {
    let fnv =
        |s: &str| s.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let diff = html(&example("bp-containers-changed"), Some(&example("bp-containers")));
    let got: Vec<String> = ["bp-containers", "bp-lm-design", "bp-seq-issue-to-landed", "bp-lm-whole"]
        .iter()
        .map(|n| html(&example(n), None))
        .chain([diff])
        .map(|page| format!("{:016x}", fnv(&page)))
        .collect();
    // A deliberate change to the renderer, the style or the viewer changes these; update them in the same commit.
    assert_eq!(
        got,
        ["f91d94207fd3f9ec", "f117ac4b16d01f14", "12bdf6dc9aecf17e", "a704b3b602d4d9c6", "9331afa85300ad81"]
    );
}
