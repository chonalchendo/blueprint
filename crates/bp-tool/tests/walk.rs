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
    let want =
        serde_json::json!({ "id": SEED, "for": "complete", "done": false, "forge_age": null, "contract_version": 1 });
    assert_eq!(v, want, "no snapshot is in view, so its age is null");
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
    let issue = |section: &str| {
        format!("---\nkind: issue\ntitle: C\nstatus: open\nunder: {n}\n---\n\n## {section}\n\n{}", block("fin"))
    };
    write(d.path(), "projects/bp/issues/0003.notes.md", &issue("Notes"));
    assert_eq!(derived(d.path()), false, "a block outside the issue's Check covers nothing");
    write(d.path(), "projects/bp/issues/0004.covers.md", &issue("Check"));
    assert_eq!(derived(d.path()), true, "an issue's block covers the finish line");
    std::fs::remove_file(d.path().join("projects/bp/milestones/m.md")).unwrap();
    let (_, v) = walk(d.path(), &["projects/bp/designs/system", "--for", "shape", "--done"]);
    assert_eq!(v["done"], false, "no milestone is planned: {v}");
}

/// `walk <seed> --for <action> --done`'s answer, with the forge snapshot at `forge` when one is given.
fn done(root: &Path, forge: Option<&Path>, seed: &str, action: &str) -> Value {
    // A record added by hand under a node shows at the next commit (12, Index), and this vault has no git: drop the
    // index so each answer reads the files as they are.
    let _ = std::fs::remove_dir_all(root.join(".bp"));
    let mut cmd = Command::cargo_bin("bp").unwrap();
    cmd.current_dir(root).env_remove("BP_VAULT").env("BP_NOW", "2026-10-18T00:00:00Z").env_remove("BP_FORGE_SNAPSHOT");
    if let Some(f) = forge {
        cmd.env("BP_FORGE_SNAPSHOT", f);
    }
    let out = cmd.args(["walk", seed, "--for", action, "--done", "--json"]).output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(out.status.success(), "{v}");
    v["done"].clone()
}

#[test]
fn deliberate_needs_the_thread_closed_and_a_decision_citing_it() {
    let d = vault();
    let thread = |slug: &str, status: &str| {
        let id = format!("projects/bp/threads/{slug}");
        write(d.path(), &format!("{id}.md"), &format!("---\nkind: thread\ntitle: T\nstatus: {status}\n---\n\nWhy?\n"));
        id
    };
    let decide = |slug: &str, thread: &str| {
        let text = format!("---\nkind: decision\ntitle: D\nstatus: accepted\n---\n\nFrom [[{thread}]].\n");
        write(d.path(), &format!("projects/bp/decisions/{slug}.md"), &text);
    };
    let open = thread("open", "open");
    decide("early", &open);
    assert_eq!(done(d.path(), None, &open, "deliberate"), false, "an open thread is not done, cited or not");
    let closed = thread("closed", "closed");
    assert_eq!(done(d.path(), None, &closed, "deliberate"), false, "no decision cites it yet");
    decide("because", &closed);
    assert_eq!(done(d.path(), None, &closed, "deliberate"), true);
    let (code, v) = walk(d.path(), &[&closed, "--for", "deliberate"]);
    assert_eq!((code, v["budget"].as_u64()), (Some(0), Some(8000)), "{v}");
    assert!(ids(&v["bodies"]).contains(&"projects/bp/decisions/because"), "a decision is a body: {v}");
}

#[test]
fn complete_is_done_when_the_latest_claim_is_released_escalated() {
    let d = vault();
    let claim = format!(
        "---\nkind: claim\nunder: {SEED}\nfrom: 2026-10-16T00:00:00Z\n---\n\n## Released\n\nto: 2026-10-16T01:00:00Z\nreason: escalated\n"
    );
    write(d.path(), &format!("{SEED}/claims/c.md"), &claim);
    assert_eq!(done(d.path(), None, SEED, "complete"), true);
}

#[test]
fn review_and_respond_read_the_open_pull_request_head() {
    let d = vault();
    let forge = tempfile::tempdir().unwrap();
    let head = |h: &str| {
        let dir = forge.path().join(h);
        let pr = format!(r#"[{{"number":1,"headRefName":"bp/0001.seed","headRefOid":"{h}"}}]"#);
        write(&dir, "open-prs.json", &pr);
        dir
    };
    let (one, two) = (head("aaaa"), head("bbbb"));
    write(
        d.path(),
        &format!("{SEED}/claims/c.md"),
        &format!("---\nkind: claim\nunder: {SEED}\nfrom: 2026-10-16T00:00:00Z\n---\n"),
    );
    assert_eq!(done(d.path(), Some(&one), SEED, "review"), false, "no review at the head");
    let table = "| severity | finding |\n|---|---|\n| major | one |\n| minor | two |\n";
    let review = format!("{SEED}/reviews/aaaa");
    write(d.path(), &format!("{review}.md"), &format!("---\nkind: review\nunder: {SEED}\nhead: aaaa\n---\n\n{table}"));
    assert_eq!(done(d.path(), Some(&one), SEED, "review"), true);
    assert_eq!(done(d.path(), Some(&two), SEED, "review"), false, "the review is at an older head");
    let answer = |n: u64, how: &str| {
        let text = format!("---\nkind: disposition\nunder: {review}\nfinding: {n}\nas: {how}\n---\n");
        write(d.path(), &format!("{review}/dispositions/{n}.md"), &text);
    };
    answer(1, "declined");
    answer(99, "declined");
    assert_eq!(done(d.path(), Some(&one), SEED, "respond"), false, "finding 2 has no disposition; 99 is no finding");
    answer(2, "fixed");
    assert_eq!(done(d.path(), Some(&one), SEED, "respond"), false, "a fix moves the head");
    assert_eq!(done(d.path(), Some(&two), SEED, "respond"), true);
}

#[test]
fn a_stop_reads_the_vault_snapshot_and_never_calls_gh() {
    let d = vault();
    let claim = format!("---\nkind: claim\nunder: {SEED}\nfrom: 2026-10-16T00:00:00Z\n---\n");
    write(d.path(), &format!("{SEED}/claims/c.md"), &claim);
    write(
        d.path(),
        &format!("{SEED}/reviews/aaaa.md"),
        &format!("---\nkind: review\nunder: {SEED}\nhead: aaaa\n---\n"),
    );
    // A `gh` that leaves a mark and fails: no stop may call it.
    let guard = tempfile::tempdir().unwrap();
    let mark = guard.path().join("called");
    write(guard.path(), "gh", &format!("#!/bin/sh\ntouch '{}'\nexit 1\n", mark.display()));
    std::fs::set_permissions(guard.path().join("gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", guard.path().display(), std::env::var("PATH").unwrap());
    let stop = || {
        let mut cmd = Command::cargo_bin("bp").unwrap();
        cmd.current_dir(d.path()).env_remove("BP_VAULT").env_remove("BP_FORGE_SNAPSHOT").env("PATH", &path);
        let out = cmd.args(["walk", SEED, "--for", "review", "--done", "--json"]).output().unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(out.status.success(), "{v}");
        (v["done"].clone(), v["forge_age"].clone())
    };
    assert_eq!(stop(), (Value::Bool(false), Value::Null), "with no snapshot no pull request is in view");
    write(d.path(), ".bp/forge/bp/open-prs.json", r#"[{"number":1,"headRefName":"bp/0001.seed","headRefOid":"aaaa"}]"#);
    // Another project's bad file is out of view: the stop reads its own project's snapshot only.
    write(d.path(), ".bp/forge/zzz/open-prs.json", "garbage");
    let (done, age) = stop();
    assert!(done == true && age.is_i64(), "the review is at the snapshot's head, read {age} seconds ago");
    assert!(!mark.exists(), "no stop called gh");
}

#[test]
fn triage_drift_needs_a_citation_for_each_failed_drift_run() {
    let d = vault();
    let s = "projects/bp/designs/system";
    let run = |key: &str, kind: &str| {
        let id = format!("{s}/runs/{key}-aaaa-1");
        let text = format!(
            "---\nkind: run\nunder: {s}\ncheck: {key}\ncheck_kind: {kind}\ncommit: aaaa\nattempt: 1\nverdict: fail\n---\n"
        );
        write(d.path(), &format!("{id}.md"), &text);
        id
    };
    run("vault", "integrity");
    assert_eq!(done(d.path(), None, s, "triage-drift"), true, "an integrity run is no drift run");
    let drift = run("drift", "shell");
    assert_eq!(done(d.path(), None, s, "triage-drift"), false);
    let thread = format!("---\nkind: thread\ntitle: T\nstatus: open\n---\n\nWhy did [[{drift}]] fail?\n");
    write(d.path(), "projects/bp/threads/drift.md", &thread);
    assert_eq!(done(d.path(), None, s, "triage-drift"), true);
}

#[test]
fn distil_needs_a_citing_learning_that_search_finds_by_its_cue() {
    let d = vault();
    let learning = |slug: &str, cue: &str| {
        let text = format!(
            "---\nkind: learning\ntitle: Quokkas nap\nstatus: draft\n{cue}cites: [{SEED}]\n---\n\nQuokkas nap at noon.\n"
        );
        write(d.path(), &format!("shared/learnings/{slug}.md"), &text);
    };
    learning("bare", "");
    assert_eq!(done(d.path(), None, SEED, "distil"), false, "a learning with no cue reads false, title or not");
    learning("cued", "cue: quokkas nap at noon\n");
    assert_eq!(done(d.path(), None, SEED, "distil"), true);
}

#[test]
fn respond_holds_the_latest_review_by_its_time_not_its_id() {
    let d = vault();
    let review = |head: &str, at: &str, finding: &str| {
        let text = format!(
            "---\nkind: review\nunder: {SEED}\nhead: {head}\nat: {at}\n---\n\n| severity | finding |\n|---|---|\n| major | {finding} |\n"
        );
        write(d.path(), &format!("{SEED}/reviews/{head}.md"), &text);
    };
    review("ffff", "2026-10-16T00:00:00Z", "older");
    review("aaaa", "2026-10-17T00:00:00Z", "newer");
    let (code, v) = walk(d.path(), &[SEED, "--for", "respond"]);
    assert_eq!(code, Some(0), "{v}");
    let first = &v["bodies"][0];
    assert_eq!(
        (first["id"].as_str(), first["kind"].as_str()),
        (Some(&*format!("{SEED}/reviews/aaaa")), Some("review"))
    );
    assert!(first["body"].as_str().unwrap().contains("| major | newer |") && first["header"]["head"] == "aaaa", "{v}");
    assert_eq!(ids(&v["bodies"]).iter().filter(|i| i.contains("/reviews/")).count(), 1, "only the latest: {v}");
    let (_, v) = walk(d.path(), &[SEED, "--for", "respond", "--ids"]);
    assert_eq!(v["ids"][0]["id"], format!("{SEED}/reviews/aaaa"), "{v}");
    let (_, v) = walk(d.path(), &[SEED, "--for", "complete"]);
    assert!(!ids(&v["bodies"]).iter().any(|i| i.contains("/reviews/")), "complete answers no review: {v}");
}
