//! The context ledger (17, check 9; 29, design change 4): the fixed load of each dispatched action and of each design
//! route, held under a cap. The cap guards growth. It trims nothing, and `install` never refuses on load.
//!
//! A token is four bytes, the rule `bp-tool/tests/bundle.rs` uses for a skill body's cap. A file counts whole,
//! front matter included, because the harness loads it whole.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Files = BTreeMap<String, String>;

/// Choices from 29: today's worst dispatched action is about 1,110 tokens, and today's worst route about 8,060.
const ROLE_CAP: usize = 1_500;
const ROUTE_CAP: usize = 8_500;

/// Each route of `bp-design`'s `## Route` (question 1) with the references it names, `gates` last every time. The
/// route is free text in `SKILL.md`, so the tool cannot derive it and this table is the ledger's copy.
const ROUTES: [(&str, &[&str]); 10] = [
    ("product", &["product", "gates"]),
    ("glossary", &["system", "gates"]),
    ("system", &["system", "quality", "strategic", "risk-threat", "gates"]),
    ("container", &["container", "quality", "strategic", "risk-threat", "gates"]),
    ("component", &["component", "discovery", "gates"]),
    ("data-model", &["data-model", "discovery", "gates"]),
    ("workflow-model", &["workflow-model", "discovery", "gates"]),
    ("decision", &["decision", "gates"]),
    ("diagram", &["diagrams", "gates"]),
    ("editorial-delta", &["revision", "gates"]),
];

fn bundle() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bundle")
}

fn walk(base: &Path, dir: &Path, out: &mut Files) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(base, &path, out);
        } else {
            let rel = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            out.insert(rel, String::from_utf8_lossy(&std::fs::read(&path).unwrap()).into_owned());
        }
    }
}

fn read() -> Files {
    let base = bundle();
    let mut files = Files::new();
    walk(&base, &base, &mut files);
    files
}

fn tokens(files: &Files, paths: &[String]) -> usize {
    paths
        .iter()
        .map(|p| files.get(p).unwrap_or_else(|| panic!("the ledger names {p}, which the bundle lacks")).len())
        .sum::<usize>()
        / 4
}

/// The skills a role file preloads: its front matter `skills:` line.
fn preloads(role: &str) -> Vec<String> {
    let line = role.lines().skip(1).take_while(|l| *l != "---").find_map(|l| l.strip_prefix("skills:"));
    line.map_or_else(Vec::new, |s| s.split(',').map(|n| format!("skills/{}/SKILL.md", n.trim())).collect())
}

/// One row per dispatch template: its role file, the role's preloaded bodies, and the template. A role with two
/// templates loads the same preloads under each.
fn role_loads(files: &Files) -> Vec<(String, usize)> {
    files
        .keys()
        .filter_map(|p| {
            let rest = p.strip_prefix("roles/")?;
            let (role, action) = rest.split_once('/')?;
            let mut paths = vec![format!("roles/{role}.md"), p.clone()];
            paths.extend(preloads(&files[&format!("roles/{role}.md")]));
            Some((format!("{role} {}", action.trim_end_matches(".md")), tokens(files, &paths)))
        })
        .collect()
}

fn reference(name: &str) -> String {
    format!("skills/bp-design/references/{name}.md")
}

/// One row per route: the `bp-design` body and the references the route names.
fn route_loads(files: &Files) -> Vec<(String, usize)> {
    ROUTES
        .iter()
        .map(|(route, names)| {
            let mut paths = vec!["skills/bp-design/SKILL.md".to_string()];
            paths.extend(names.iter().map(|n| reference(n)));
            ((*route).to_string(), tokens(files, &paths))
        })
        .collect()
}

/// A row over its cap, or a reference no route names, as a message that names the row.
fn failures(files: &Files) -> Vec<String> {
    let over = |rows: Vec<(String, usize)>, cap, what: &str| {
        rows.into_iter()
            .filter(|(_, n)| *n > cap)
            .map(|(name, n)| format!("{what} {name}: {n} tokens, over the cap of {cap}"))
            .collect::<Vec<_>>()
    };
    let mut out = over(role_loads(files), ROLE_CAP, "role");
    out.extend(over(route_loads(files), ROUTE_CAP, "route"));
    for path in files.keys().filter(|p| p.contains("/references/")) {
        if !ROUTES.iter().any(|(_, names)| names.iter().any(|n| reference(n) == *path)) {
            out.push(format!("{path}: in no route of the ledger"));
        }
    }
    out
}

fn with(files: &Files, path: &str, extra: usize) -> Files {
    let mut copy = files.clone();
    copy.get_mut(path).unwrap_or_else(|| panic!("no {path}")).push_str(&"q".repeat(extra));
    copy
}

#[test]
fn the_bundle_is_under_every_cap_and_every_reference_is_routed() {
    let bad = failures(&read());
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn over_cap_role_names_the_role() {
    let files = with(&read(), "roles/bp-worker.md", ROLE_CAP * 4);
    let bad = failures(&files);
    assert!(bad.iter().any(|m| m.starts_with("role bp-worker complete: ")), "{bad:#?}");
    assert!(bad.iter().any(|m| m.starts_with("role bp-worker respond: ")), "{bad:#?}");
    assert!(!bad.iter().any(|m| m.contains("bp-reviewer")), "{bad:#?}");
}

#[test]
fn over_cap_preload_counts_against_the_role() {
    let files = with(&read(), "skills/bp-review/SKILL.md", ROLE_CAP * 4);
    assert_eq!(failures(&files).len(), 1, "only bp-reviewer preloads bp-review");
    assert!(failures(&files)[0].starts_with("role bp-reviewer review: "));
}

#[test]
fn over_cap_route_names_the_route() {
    let files = with(&read(), &reference("gates"), ROUTE_CAP * 4);
    let bad = failures(&files);
    assert_eq!(bad.len(), ROUTES.len(), "every route ends in `gates`: {bad:#?}");
    assert!(bad.iter().all(|m| m.starts_with("route ")));
}

#[test]
fn over_cap_body_counts_in_every_route() {
    let files = with(&read(), "skills/bp-design/SKILL.md", ROUTE_CAP * 4);
    assert_eq!(failures(&files).len(), ROUTES.len());
}

#[test]
fn a_reference_in_no_route_is_found() {
    let mut files = read();
    files.insert(reference("orphan"), "x".into());
    assert_eq!(failures(&files), [format!("{}: in no route of the ledger", reference("orphan"))]);
}

/// Prints the figures the `design-route-load` block reads, and every row for the owner.
#[test]
#[ignore = "prints the ledger for the finish-line block"]
fn print_max_route() {
    let files = read();
    for (name, n) in role_loads(&files) {
        println!("role {name} {n}");
    }
    let routes = route_loads(&files);
    for (name, n) in &routes {
        println!("route {name} {n}");
    }
    println!("max_route {}", routes.iter().map(|(_, n)| *n).max().unwrap());
}
