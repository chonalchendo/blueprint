//! Large data models (28, issue 5): the budget of 8 entities warns and does not cap, so a model of 30 or 48
//! entities with links across the grid renders, and a refusal comes fast. The fixtures come from a generator at
//! master `fc5178f`: a grid of entities with neighbour links and seeded random cross-links, which then refused
//! `label/collision` (30 entities) or `route/touches` after 29 s (48 entities). `er-30-x40` has 40 cross-links: a
//! round leaves as many failing as the round before, so the layout stops there and refuses.

use std::time::{Duration, Instant};

use bp_diagram::{Severity, render};

fn spec(path: &str) -> String {
    std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn errors(path: &str) -> Vec<String> {
    match render(&spec(path), None, None) {
        Ok(r) => r.findings.iter().filter(|f| f.severity == Severity::Error).map(|f| format!("{f:?}")).collect(),
        Err(f) => f.iter().filter(|f| f.severity == Severity::Error).map(|f| format!("{} {}", f.rule, f.id)).collect(),
    }
}

#[test]
fn thirty_entities_with_cross_links_render() {
    for path in ["tests/large/er-30-x3.yaml", "tests/large/er-30.yaml"] {
        assert_eq!(errors(path), Vec::<String>::new(), "{path}");
    }
}

#[test]
fn forty_eight_entities_with_random_links_render_within_two_seconds() {
    let start = Instant::now();
    assert_eq!(errors("tests/large/er-48.yaml"), Vec::<String>::new());
    assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
}

#[test]
fn a_hopeless_model_refuses_within_two_seconds() {
    let start = Instant::now();
    assert!(!errors("tests/large/er-30-x40.yaml").is_empty());
    assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
}

#[test]
fn the_whole_logical_model_renders_with_budget_warnings_only() {
    let r = render(&spec("examples/bp-lm-whole.yaml"), None, None).unwrap_or_else(|f| panic!("{f:#?}"));
    assert!(r.findings.iter().all(|f| f.rule.starts_with("budget/")), "{:?}", r.findings);
    assert_eq!(r.html.matches("data-term=").count() + r.html.matches("data-noun=").count(), 23);
}
