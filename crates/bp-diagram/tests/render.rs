//! The renderer's contract: the examples render clean, twice to the same bytes, with the SVG contract the view
//! reads (28, design change 12); a diff marks what changed; state and workflow render from order.

use std::collections::BTreeSet;

use bp_diagram::render;

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
    assert!(page.contains("class=\"node role-external added dashed\" data-id=\"forge\""), "forge is added");
    assert!(
        page.contains("class=\"node role-external removed dashed\" data-id=\"repository\""),
        "repository stays as a ghost"
    );
    assert!(page.contains("class=\"edge removed\" data-id=\"tool--repository\""));
    assert!(page.contains("class=\"bp bp-architecture diff mode-diff\""));
}

#[test]
fn state_and_workflow_render_from_order() {
    let state = "type: state\ntitle: An issue\nlede: An issue moves from open to resolved.\nfocus: [resolved]\nnodes:\n  - {id: start, noun: start, role: start}\n  - {id: open, term: issue, label: open}\n  - {id: in-progress, noun: in-progress, label: in progress}\n  - {id: resolved, noun: resolved}\n  - {id: escalated, noun: escalated}\n  - {id: end, noun: end, role: end}\nedges:\n  - {from: start, to: open}\n  - {from: open, to: in-progress, label: cut}\n  - {from: in-progress, to: resolved, label: land}\n  - {from: in-progress, to: escalated, label: escalate}\n  - {from: escalated, to: open, label: reopen}\n  - {from: resolved, to: end}\n";
    html(state, None);
    let flow = "type: workflow\ntitle: Review\nlede: A pull request goes from opened to landed.\nlanes:\n  - {id: worker, label: Worker}\n  - {id: human, label: Human}\nnodes:\n  - {id: open, noun: open, lane: worker, label: open pull request}\n  - {id: review, noun: review, lane: human, label: review}\n  - {id: respond, noun: respond, lane: worker}\n  - {id: land, noun: land, lane: human}\nedges:\n  - {from: open, to: review}\n  - {from: review, to: respond, label: findings}\n  - {from: respond, to: land}\n";
    html(flow, None);
}

/// FNV-1a: a fixed hash of the bytes, so a platform that renders other bytes fails here (28, design change 3).
#[test]
fn the_bytes_are_the_same_on_every_platform() {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in html(&example("bp-lm-design"), None).bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    // A deliberate change to the renderer, the style or the viewer changes this; update it in the same commit.
    assert_eq!(format!("{h:016x}"), "34a0c9be7ceadb5b");
}
