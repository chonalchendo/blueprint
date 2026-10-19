//! `bp walk` through the binary: the order, the fill, the conventions tail and the `context_incomplete` refusal.

use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

const SEED: &str = "projects/bp/issues/0001.seed";

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn vault() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    write(r, "projects/bp/project.md", "---\nkind: project\ntitle: P\n---\n\nFollows [[shared/conventions/style]].\n");
    write(r, "shared/conventions/style.md", "---\nkind: convention\ntitle: Style\n---\n\nShort sentences.\n");
    write(r, "shared/conventions/naming.md", "---\nkind: convention\ntitle: Naming\n---\n\nNobody cites this.\n");
    write(r, "projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n\nThe goal.\n");
    write(r, "projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\nThe system.\n");
    let issue = |title: &str| {
        format!(
            "---\nkind: issue\ntitle: {title}\nstatus: open\nunder: projects/bp/milestones/m\ncites: [projects/bp/designs/system]\n---\n\nThe problem.\n"
        )
    };
    write(r, &format!("{SEED}.md"), &issue("Seed"));
    write(r, "projects/bp/issues/0002.sibling.md", &issue("Sibling"));
    write(r, "projects/bp/inbox/note.md", &format!("---\nkind: inbox\ntitle: N\n---\n\nAbout [[{SEED}]].\n"));
    d
}

fn walk(root: &Path, args: &[&str]) -> (Option<i32>, Value) {
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(root)
        .env_remove("BP_VAULT")
        .env("BP_NOW", "2026-10-18T00:00:00Z")
        .env_remove("BP_FORGE_SNAPSHOT")
        .arg("walk")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap())
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect()
}

#[test]
fn the_seed_leads_and_the_cited_conventions_close_the_id_tail() {
    let d = vault();
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete"]);
    assert_eq!(code, Some(0), "{v}");
    let bodies = ids(&v["bodies"]);
    assert_eq!(bodies[0], SEED);
    for want in ["projects/bp/milestones/m", "projects/bp/designs/system", "projects/bp/issues/0002.sibling"] {
        assert!(bodies.contains(&want), "{want} is a body: {bodies:?}");
    }
    assert_eq!(ids(&v["ids"]).last(), Some(&"shared/conventions/style"), "{v}");
    let all = [bodies, ids(&v["ids"])].concat();
    assert!(!all.contains(&"projects/bp/inbox/note"), "an inbox entry is never walked");
    assert!(!all.contains(&"shared/conventions/naming"), "an uncited, unreached convention stays out");
    assert_eq!(v["budget"], 15000);
    assert!(v["intents"].is_array());
}

#[test]
fn a_small_budget_cuts_bodies_to_id_lines() {
    let d = vault();
    let long = format!("---\nkind: design\ntitle: S\nstatus: active\n---\n\n{}\n", "word ".repeat(200));
    write(d.path(), "projects/bp/designs/system.md", &long);
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete", "--budget", "60"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(ids(&v["bodies"])[0], SEED, "the seed is always a body");
    assert!(!ids(&v["bodies"]).contains(&"projects/bp/designs/system"), "{v}");
    assert!(v["cut"].as_array().unwrap().contains(&"projects/bp/designs/system".into()), "{v}");
    assert!(v["used"].as_u64().unwrap() <= 60, "{v}");
}

#[test]
fn ids_lists_every_reached_node_seed_first() {
    let d = vault();
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete", "--ids"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(ids(&v["ids"])[0], SEED);
    assert!(v["ids"][0]["rank"].as_f64().unwrap() > 0.0);
}

#[test]
fn a_missing_or_empty_required_design_refuses_context_incomplete() {
    let d = vault();
    std::fs::remove_file(d.path().join("projects/bp/designs/system.md")).unwrap();
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("context_incomplete")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("absent"), "{v}");

    write(d.path(), "projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\n  \n");
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("context_incomplete")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("empty"), "{v}");
}

#[test]
fn the_seed_must_fit_the_action() {
    let d = vault();
    let (code, v) = walk(d.path(), &["projects/bp/designs/system", "--for", "complete"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("unknown_value")), "{v}");
    let (code, v) = walk(d.path(), &[SEED, "--for", "nonsense"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("unknown_value")), "{v}");
}
