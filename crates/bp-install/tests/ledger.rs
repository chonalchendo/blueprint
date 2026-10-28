//! The context ledger (17, check 9; 29, design change 4): the fixed load of each dispatched action and of each design
//! route, held under a cap. The cap guards growth. It trims nothing, and `install` never refuses on load.
//!
//! A token is four bytes, the rule `bp-tool/tests/bundle.rs` uses for a skill body's cap. A file counts whole,
//! front matter included, because the harness loads it whole.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Files = BTreeMap<String, String>;

/// Choices from 29. The role cap sits above today's worst dispatched action. The route cap guards growth with about
/// 7 % headroom over the worst route, `revise-container`; a cap with none would trip on every edit.
const ROLE_CAP: usize = 1_500;
const ROUTE_CAP: usize = 10_500;

/// The nominal routes of `bp-design`'s question 1, `gates` last every time, plus the editorial delta and the
/// `diagram` add-on, whose row is its own cost and not a load any session has alone. `route_loads` adds a
/// `revise-<kind>` row to each kind: question 2 loads `revision` with it. Every conditional of questions 4 to 6 is
/// out, except question 5 and 6 for system and container. The route is free text in `SKILL.md`, so the tool cannot
/// derive it and this table is the ledger's copy.
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

/// The skill directories a role file preloads, as `skills/<name>/`: its front matter `skills:` line.
fn preloads(role: &str) -> Vec<String> {
    let line = role.lines().skip(1).take_while(|l| *l != "---").find_map(|l| l.strip_prefix("skills:"));
    line.map_or_else(Vec::new, |s| s.split(',').map(|n| format!("skills/{}/", n.trim())).collect())
}

/// Every skill directory that some role file preloads.
fn preloaded(files: &Files) -> Vec<String> {
    files
        .iter()
        .filter(|(p, _)| p.strip_prefix("roles/").is_some_and(|r| !r.contains('/')))
        .flat_map(|(_, t)| preloads(t))
        .collect()
}

/// One row per dispatch template: its role file, the template, and each preloaded skill's body and references. A
/// role with two templates loads the same preloads under each. The body discloses a reference only as the work needs
/// it, which saves runtime context but not cap, so the row counts each reference (31, design change 1).
fn role_loads(files: &Files) -> Vec<(String, usize)> {
    files
        .keys()
        .filter_map(|p| {
            let rest = p.strip_prefix("roles/")?;
            let (role, action) = rest.split_once('/')?;
            let mut paths = vec![format!("roles/{role}.md"), p.clone()];
            for dir in preloads(&files[&format!("roles/{role}.md")]) {
                paths.push(format!("{dir}SKILL.md"));
                let refs = files.keys().filter(|f| f.strip_prefix(&dir).is_some_and(|r| r.starts_with("references/")));
                paths.extend(refs.cloned());
            }
            Some((format!("{role} {}", action.trim_end_matches(".md")), tokens(files, &paths)))
        })
        .collect()
}

fn reference(name: &str) -> String {
    format!("skills/bp-design/references/{name}.md")
}

/// One row per route and one `revise-<kind>` row per kind: the `bp-design` body and the references the row names.
fn route_loads(files: &Files) -> Vec<(String, usize)> {
    let row = |name: String, names: &[&str]| {
        let mut paths = vec!["skills/bp-design/SKILL.md".to_string()];
        paths.extend(names.iter().map(|n| reference(n)));
        (name, tokens(files, &paths))
    };
    ROUTES
        .iter()
        .flat_map(|(route, names)| {
            let mut rows = vec![row((*route).to_string(), names)];
            if !matches!(*route, "diagram" | "editorial-delta") {
                rows.push(row(format!("revise-{route}"), &[names, &["revision"][..]].concat()));
            }
            rows
        })
        .collect()
}

/// A row over its cap, or a reference that no route names and no role preloads, as a message that names the row.
fn failures(files: &Files) -> Vec<String> {
    let over = |rows: Vec<(String, usize)>, cap, what: &str| {
        rows.into_iter()
            .filter(|(_, n)| *n > cap)
            .map(|(name, n)| format!("{what} {name}: {n} tokens, over the cap of {cap}"))
            .collect::<Vec<_>>()
    };
    let mut out = over(role_loads(files), ROLE_CAP, "role");
    out.extend(over(route_loads(files), ROUTE_CAP, "route"));
    let loaded = preloaded(files);
    for path in files.keys().filter(|p| p.contains("/references/")) {
        let routed = ROUTES.iter().any(|(_, names)| names.iter().any(|n| reference(n) == *path));
        if !routed && !loaded.iter().any(|d| path.starts_with(&format!("{d}references/"))) {
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
fn over_cap_reference_of_a_preload_counts_against_the_role() {
    let files = with(&read(), "skills/bp-research/references/fan-out.md", ROLE_CAP * 4);
    let bad = failures(&files);
    assert_eq!(bad.len(), 1, "only bp-researcher preloads bp-research: {bad:#?}");
    assert!(bad[0].starts_with("role bp-researcher research: "), "{bad:#?}");
}

#[test]
fn over_cap_route_names_the_route() {
    let files = with(&read(), &reference("gates"), ROUTE_CAP * 4);
    let bad = failures(&files);
    assert_eq!(bad.len(), route_loads(&files).len(), "every row ends in `gates`: {bad:#?}");
    assert!(bad.iter().all(|m| m.starts_with("route ")));
}

#[test]
fn over_cap_body_counts_in_every_route() {
    let files = with(&read(), "skills/bp-design/SKILL.md", ROUTE_CAP * 4);
    assert_eq!(failures(&files).len(), route_loads(&files).len());
}

/// Catches a reference added to or dropped from the route text. It does not catch one moved between routes.
#[test]
fn the_table_names_the_references_of_the_route_text() {
    let text = &read()["skills/bp-design/SKILL.md"];
    let named: Vec<&str> =
        text.split("references/").skip(1).filter_map(|t| t.split_once(".md").map(|(n, _)| n)).collect();
    let table: Vec<&str> = ROUTES.iter().flat_map(|(_, names)| names.iter().copied()).collect();
    for n in &named {
        assert!(table.contains(n), "SKILL.md names references/{n}.md, which no row holds");
    }
    for n in &table {
        assert!(named.contains(n), "a row holds {n}, which SKILL.md does not name");
    }
}

#[test]
fn a_reference_in_no_route_is_found() {
    let mut files = read();
    files.insert(reference("orphan"), "x".into());
    assert_eq!(failures(&files), [format!("{}: in no route of the ledger", reference("orphan"))]);
    let mut files = read();
    files.insert("skills/bp-research/references/b.md".into(), "x".into());
    assert!(failures(&files).is_empty(), "a reference of a preloaded skill is routed by its role");
    files.insert("skills/bp-capture/references/x.md".into(), "x".into());
    assert_eq!(
        failures(&files),
        ["skills/bp-capture/references/x.md: in no route of the ledger"],
        "no role preloads it"
    );
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
