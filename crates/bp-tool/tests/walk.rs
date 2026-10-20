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
    for want in ["projects/bp/milestones/m", "projects/bp/designs/system"] {
        assert!(bodies.contains(&want), "{want} is a body: {bodies:?}");
    }
    assert!(ids(&v["ids"]).contains(&"projects/bp/issues/0002.sibling"), "a sibling issue is an id line: {v}");
    assert_eq!(v["cut"], 0);
    assert_eq!(ids(&v["ids"]).last(), Some(&"shared/conventions/style"), "{v}");
    let all = [bodies, ids(&v["ids"])].concat();
    assert!(!all.contains(&"projects/bp/inbox/note"), "an inbox entry is never walked");
    assert!(!all.contains(&"shared/conventions/naming"), "an uncited, unreached convention stays out");
    assert_eq!(v["budget"], 15000);
    assert!(v["intents"].is_array());
}

#[test]
fn a_body_that_does_not_fit_becomes_an_id_line() {
    let d = vault();
    let long = format!("---\nkind: design\ntitle: S\nstatus: active\n---\n\n{}\n", "word ".repeat(200));
    write(d.path(), "projects/bp/designs/system.md", &long);
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete", "--budget", "200"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(ids(&v["bodies"])[0], SEED, "the seed is always a body");
    assert!(!ids(&v["bodies"]).contains(&"projects/bp/designs/system"), "{v}");
    assert!(ids(&v["ids"]).contains(&"projects/bp/designs/system"), "{v}");
    assert!(v["used"].as_u64().unwrap() <= 200, "{v}");
}

#[test]
fn ids_lists_every_reached_node_seed_first() {
    let d = vault();
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete", "--ids"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(ids(&v["ids"])[0], SEED);
    assert_eq!(ids(&v["ids"]).last(), Some(&"shared/conventions/style"), "conventions close the map too");
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

#[test]
fn a_record_seed_and_a_deep_walk_are_refused() {
    let d = vault();
    write(
        d.path(),
        &format!("{SEED}/claims/c.md"),
        &format!("---\nkind: claim\nunder: {SEED}\nfrom: 2026-10-16T00:00:00Z\n---\n"),
    );
    let (code, v) = walk(d.path(), &[&format!("{SEED}/claims/c"), "--for", "distil"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("unknown_value")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("distil walks from: project"), "{v}");
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete", "--depth", "9"]);
    assert_eq!((code, v["code"].as_str()), (Some(4), Some("usage")), "{v}");
}

#[test]
fn done_answers_the_stop_column_with_no_slice() {
    let d = vault();
    let (code, v) = walk(d.path(), &[SEED, "--for", "complete", "--done"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v, serde_json::json!({ "id": SEED, "for": "complete", "done": false, "contract_version": 1 }));
    for (seed, action) in [("projects/bp/designs/system", "design"), ("projects/bp/designs/system", "shape")] {
        let (_, v) = walk(d.path(), &[seed, "--for", action, "--done"]);
        assert_eq!(v["done"], true, "{action}: {v}");
    }
    // No issue is under the new milestone, so the batch is short, and its finish line names a block none holds.
    let n = "projects/bp/milestones/n";
    let block = |name: &str| format!("```check shell {{name={name}}}\ntrue\n```\n");
    let fin = format!("---\nkind: milestone\ntitle: N\nstatus: approved\n---\n\n## Finish line\n\n{}", block("fin"));
    write(d.path(), &format!("{n}.md"), &fin);
    let derived = |d: &Path| walk(d, &[n, "--for", "derive", "--done"]).1["done"].clone();
    assert_eq!(derived(d.path()), false);
    let issue = format!("---\nkind: issue\ntitle: C\nstatus: open\nunder: {n}\n---\n\n## Check\n\n{}", block("fin"));
    write(d.path(), "projects/bp/issues/0003.covers.md", &issue);
    assert_eq!(derived(d.path()), true, "an issue's block covers the finish line");
    std::fs::remove_file(d.path().join("projects/bp/milestones/m.md")).unwrap();
    let (_, v) = walk(d.path(), &["projects/bp/designs/system", "--for", "shape", "--done"]);
    assert_eq!(v["done"], false, "no milestone is planned: {v}");
}

#[test]
fn deliberate_is_done_when_the_thread_is_closed_and_a_decision_cites_it() {
    let d = vault();
    let thread = "projects/bp/threads/why";
    let at = |status: &str| format!("---\nkind: thread\ntitle: Why\nstatus: {status}\n---\n\nThe question.\n");
    write(d.path(), &format!("{thread}.md"), &at("open"));
    let done = |d: &Path| walk(d, &[thread, "--for", "deliberate", "--done"]).1["done"].clone();
    assert_eq!(done(d.path()), false);
    write(d.path(), &format!("{thread}.md"), &at("closed"));
    assert_eq!(done(d.path()), false, "no decision cites it yet");
    let decision = format!("---\nkind: decision\ntitle: Because\nstatus: accepted\n---\n\nFrom [[{thread}]].\n");
    write(d.path(), "projects/bp/decisions/because.md", &decision);
    assert_eq!(done(d.path()), true);
    let (code, v) = walk(d.path(), &[thread, "--for", "deliberate"]);
    assert_eq!((code, v["budget"].as_u64()), (Some(0), Some(8000)), "{v}");
    assert!(ids(&v["bodies"]).contains(&"projects/bp/decisions/because"), "a decision is a body: {v}");
}
