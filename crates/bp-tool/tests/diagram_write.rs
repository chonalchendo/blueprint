//! `bp diagram <design> <name>` through the binary (28, design changes 5 and 6): the spec and its page in one commit,
//! the repeat, `--remove`, the links resolved against the vault, `design_not_delta`, the drill badge's count, and the
//! `nodes` check over what the verb wrote.

use std::path::Path;

mod common;

use common::{bp, bp_env, commits, git};

const FRONT: &str = "projects/bp/designs/containers/front";
const VAULT: &str = "projects/bp/designs/containers/vault";

const SPEC: &str = "type: architecture\nlevel: container\ntitle: Front\nlede: Two boxes.\nnodes:\n  - {id: h, term: \
                    harness, at: [0, 0]}\n  - {id: v, noun: Vault, at: [1, 0]}\n";

fn design(title: &str, status: &str, body: &str) -> String {
    format!("---\nkind: design\ntitle: {title}\nstatus: {status}\nunder: projects/bp/designs/system\n---\n\n{body}")
}

/// A project with a glossary, a draft container design whose body names `Vault`, an active one, and a code
/// repository for `check`.
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let nodes = "## Check\n\n~~~check nodes {name=diagram}\n~~~\n";
    let vault = common::vault(&[
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        (
            "projects/bp/designs/system.md",
            "---\nkind: design\ntitle: S\nstatus: active\nunder: projects/bp/project\n---\n\nThe system.\n",
        ),
        ("projects/bp/designs/glossary.md", &design("G", "active", "- `harness` — runs the agents.\n")),
        (
            &format!("{FRONT}.md"),
            &design("Front", "draft", &format!("The front.\n\n## Vault\n\nThe files.\n\n{nodes}")),
        ),
        (&format!("{VAULT}.md"), &design("Vault", "active", "The vault.\n")),
        ("shared/.keep", ""),
    ]);
    (vault, common::vault(&[("marker.txt", "here\n")]))
}

fn spec(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

#[test]
fn the_spec_and_its_page_land_in_one_commit() {
    let (vault, _code) = setup();
    let root = vault.path();
    let tmp = tempfile::tempdir().unwrap();
    let s = spec(tmp.path(), "s.yaml", SPEC);
    let (exit, v) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s]);
    assert_eq!(exit, Some(0), "{v}");
    let paths = [format!("{FRONT}/diagrams/front.yaml"), format!("{FRONT}/diagrams/front.html")];
    assert_eq!(v["written"], serde_json::json!(paths), "{v}");
    assert_eq!(std::fs::read_to_string(root.join(&paths[0])).unwrap(), SPEC);
    let html = std::fs::read_to_string(root.join(&paths[1])).unwrap();
    assert!(html.contains("data-id=\"v\" data-noun=\"Vault\""), "the page beside the spec");
    let message = git(root, &["log", "-1", "--format=%B"]);
    assert!(message.lines().any(|l| l == "Bp-Verb: diagram"), "{message}");
    assert_eq!(git(root, &["show", "--name-only", "--format=", "HEAD"]).lines().count(), 2, "both files, one commit");
    assert_eq!(git(root, &["status", "--porcelain"]), "", "nothing left behind");
    let before = commits(root);
    let (exit, v) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s]);
    assert_eq!((exit, v["id"].as_str()), (Some(3), Some(FRONT)), "a repeat writes nothing: {v}");
    assert_eq!(commits(root), before);

    let (exit, v) = bp(root, &["diagram", FRONT, "front", "--remove"]);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!(v["removed"], serde_json::json!(paths));
    assert!(!root.join(FRONT).join("diagrams").exists(), "the emptied directory goes too");
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: diagram"));
    let (exit, _) = bp(root, &["diagram", FRONT, "front", "--remove"]);
    assert_eq!(exit, Some(3), "nothing left to remove");
}

#[test]
fn a_link_that_does_not_resolve_refuses_and_writes_nothing() {
    let (vault, _code) = setup();
    let root = vault.path();
    let tmp = tempfile::tempdir().unwrap();
    let before = commits(root);
    for (from, to) in [("term: harness", "term: no-such-term"), ("noun: Vault", "noun: Front door")] {
        let s = spec(tmp.path(), "bad.yaml", &SPEC.replace(from, to));
        let (exit, v) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s]);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{to}: {v}");
        assert_eq!(v["findings"][0]["rule"], "nodes/resolve", "{v}");
        assert!(v["fix"].as_str().unwrap().contains(&format!("bp diagram {FRONT} front --spec-file")), "{v}");
    }
    assert_eq!(commits(root), before);
    assert!(!root.join(FRONT).join("diagrams").exists());
}

#[test]
fn a_design_past_draft_refuses_design_not_delta() {
    let (vault, _code) = setup();
    let root = vault.path();
    let tmp = tempfile::tempdir().unwrap();
    let s = spec(tmp.path(), "s.yaml", SPEC);
    let (exit, v) = bp(root, &["diagram", VAULT, "front", "--spec-file", &s]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("design_not_delta")), "{v}");
    assert!(v["fix"].as_str().unwrap().starts_with(&format!("bp record delta {VAULT}")), "{v}");
    let (exit, v) = bp(root, &["diagram", VAULT, "front", "--remove"]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("design_not_delta")), "{v}");
    for (design, name, want) in [
        (FRONT, "a.b", (4, "usage")),
        (FRONT, "Front", (4, "usage")),
        ("projects/bp/project", "front", (2, "schema_invalid")),
        ("projects/bp/designs/containers/none", "front", (2, "no_node")),
    ] {
        let (exit, v) = bp(root, &["diagram", design, name, "--spec-file", &s]);
        assert_eq!((exit, v["code"].as_str()), (Some(want.0), Some(want.1)), "{design} {name}: {v}");
    }
    let (_, v) = bp(root, &["diagram", "projects/bp/project", "front", "--spec-file", &s]);
    assert_eq!(v["fix"], "a diagram belongs to a design; projects/bp/project is kind project", "{v}");
    let (exit, _) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s, "--out", "/tmp/x.html"]);
    assert_eq!(exit, Some(4), "the preview's --out takes no design");
}

#[test]
fn a_container_node_counts_the_component_views_it_drills_to() {
    let (vault, _code) = setup();
    let root = vault.path();
    let component = "type: architecture\nlevel: component\ntitle: Inside\nlede: One box.\nnodes:\n  - {id: i, noun: \
                     Index, at: [0, 0]}\n";
    for (name, text) in [("inside.yaml", component), ("old.nodes.yaml", component), ("ctx.yaml", SPEC)] {
        std::fs::create_dir_all(root.join(VAULT).join("diagrams")).unwrap();
        std::fs::write(root.join(VAULT).join("diagrams").join(name), text).unwrap();
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "views"]);
    let tmp = tempfile::tempdir().unwrap();
    let s = spec(tmp.path(), "s.yaml", SPEC);
    let (exit, v) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s]);
    assert_eq!(exit, Some(0), "{v}");
    let html = std::fs::read_to_string(root.join(FRONT).join("diagrams/front.html")).unwrap();
    assert_eq!(html.matches("<g class=\"badge\">").count(), 1, "only the vault box drills");
    assert!(html.contains("\"children\":1"), "one component view; the node list and the container view do not count");
}

#[test]
fn the_nodes_check_runs_over_the_specs() {
    let (vault, code) = setup();
    let (root, code) = (vault.path(), code.path());
    let check = || bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["check", FRONT]);
    let (exit, v) = check();
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!((v["runs"][0]["kind"].as_str(), v["runs"][0]["verdict"].as_str()), (Some("nodes"), Some("pass")));
    assert_eq!(v["runs"][0]["note"], format!("{FRONT} has no diagram spec"));
    let tmp = tempfile::tempdir().unwrap();
    let s = spec(tmp.path(), "s.yaml", SPEC);
    assert_eq!(bp(root, &["diagram", FRONT, "front", "--spec-file", &s]).0, Some(0));
    let (exit, v) = check();
    assert_eq!((exit, v["runs"][0]["verdict"].as_str()), (Some(0), Some("pass")), "{v}");
    // A human edit that breaks a link, committed: the next run fails and names the node.
    let path = root.join(FRONT).join("diagrams/front.yaml");
    std::fs::write(&path, SPEC.replace("noun: Vault", "noun: Nowhere")).unwrap();
    git(root, &["commit", "-qam", "hand edit"]);
    let (exit, v) = check();
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("check_failed")), "{v}");
    assert_eq!(v["runs"][0]["note"], format!("1 finding in 1 of 1 diagram file of {FRONT}"));
    let (_, run) = bp(root, &["show", v["runs"][0]["id"].as_str().unwrap()]);
    let body = run["body"].as_str().unwrap_or_default();
    assert!(body.contains("nodes/resolve: front#v: noun `Nowhere` is not named by the home design"), "{run}");
    // A file `bp` cannot have written fails the run, so no spec hides from it; a converted node list does not.
    let dir = root.join(FRONT).join("diagrams");
    std::fs::write(dir.join("front.yaml"), SPEC).unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    for name in ["evil.v2.yaml", "evil.yml", "sub/evil.yaml", "old.nodes.yaml"] {
        std::fs::write(dir.join(name), SPEC.replace("term: harness", "term: no-such-term")).unwrap();
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "planted"]);
    let (exit, v) = check();
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("check_failed")), "{v}");
    assert_eq!(v["runs"][0]["note"], format!("3 findings in 3 of 4 diagram files of {FRONT}"));
    let (_, run) = bp(root, &["show", v["runs"][0]["id"].as_str().unwrap()]);
    let body = run["body"].as_str().unwrap_or_default();
    for name in ["evil.v2.yaml", "evil.yml", "sub"] {
        assert!(body.contains(&format!("nodes/name: diagrams/{name}: not a diagram spec")), "{name}: {run}");
    }
    assert!(!body.contains("old.nodes.yaml"), "{run}");
    // An owner with no named block exits 3 and lists no run.
    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["check", VAULT]);
    assert_eq!((exit, v["runs"].clone()), (Some(3), serde_json::json!([])), "{v}");
}

#[test]
fn the_body_and_the_glossary_it_resolves_against_are_committed() {
    let (vault, _code) = setup();
    let root = vault.path();
    let tmp = tempfile::tempdir().unwrap();
    let s = spec(tmp.path(), "s.yaml", &SPEC.replace("noun: Vault", "noun: Ghost"));
    for (file, extra) in
        [(format!("{FRONT}.md"), "\n## Ghost\n"), ("projects/bp/designs/glossary.md".to_string(), "- `ghost` — a.\n")]
    {
        let path = root.join(&file);
        let committed = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, committed.clone() + extra).unwrap();
        let (exit, v) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s]);
        assert_eq!(
            (exit, v["code"].as_str(), v["subject"].as_str()),
            (Some(2), Some("path_dirty"), Some(&*file)),
            "{v}"
        );
        std::fs::write(&path, committed).unwrap();
    }
    assert!(!root.join(FRONT).join("diagrams").exists());
}

#[test]
fn with_no_glossary_a_term_says_how_to_make_one() {
    let (vault, _code) = setup();
    let root = vault.path();
    git(root, &["rm", "-q", "projects/bp/designs/glossary.md"]);
    git(root, &["commit", "-qm", "no glossary"]);
    let tmp = tempfile::tempdir().unwrap();
    let s = spec(tmp.path(), "s.yaml", SPEC);
    let (exit, v) = bp(root, &["diagram", FRONT, "front", "--spec-file", &s]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{v}");
    let detail = v["findings"][0]["detail"].as_str().unwrap();
    assert!(
        detail.starts_with("term `harness` is no glossary slug: projects/bp/designs/glossary does not exist"),
        "{v}"
    );
    assert!(
        v["fix"].as_str().unwrap().contains("bp create design --project bp --title Glossary --slug glossary"),
        "{v}"
    );
}
