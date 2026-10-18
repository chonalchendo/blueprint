//! `bp list` intents and `--ready` through the binary, with a forge snapshot and a fixed clock.

use std::path::Path;

use assert_cmd::Command;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn issue(status: &str, depends: &str) -> String {
    format!("---\nkind: issue\ntitle: T\nstatus: {status}\ndepends: [{depends}]\n---\n\nBody.\n")
}

fn vault() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    write(r, "projects/bp/issues/0001.ready.md", &issue("open", ""));
    write(r, "projects/bp/issues/0002.waits.md", &issue("open", "\"projects/bp/issues/0003.held\""));
    write(r, "projects/bp/issues/0003.held.md", &issue("in-progress", ""));
    write(
        r,
        "projects/bp/issues/0003.held/claims/s1.md",
        "---\nkind: claim\nunder: projects/bp/issues/0003.held\nfrom: \"2026-10-17T00:00:00Z\"\n---\n",
    );
    write(r, "projects/bp/issues/0004.done.md", &issue("resolved", ""));
    // A released claim no longer holds the issue; an escalated release blocks it (13, 21).
    let claim = |under: &str, reason: &str| {
        format!(
            "---\nkind: claim\nunder: {under}\nfrom: 2026-10-16T00:00:00Z\n---\n\n## Released\n\nto: 2026-10-16T01:00:00Z\nreason: {reason}\n"
        )
    };
    write(r, "projects/bp/issues/0005.dropped.md", &issue("open", ""));
    write(r, "projects/bp/issues/0005.dropped/claims/c.md", &claim("projects/bp/issues/0005.dropped", "dropped"));
    write(r, "projects/bp/issues/0006.escalated.md", &issue("open", ""));
    write(r, "projects/bp/issues/0006.escalated/claims/c.md", &claim("projects/bp/issues/0006.escalated", "escalated"));
    write(r, "projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n");
    write(r, "forge/open-prs.json", r#"[{"number":9,"headRefName":"bp/0003.held","headRefOid":"h"}]"#);
    d
}

fn list(root: &Path, args: &[&str], forge: bool) -> serde_json::Value {
    let mut cmd = Command::cargo_bin("bp").unwrap();
    cmd.current_dir(root).env_remove("BP_VAULT").env("BP_NOW", "2026-10-18T00:00:00Z").env_remove("BP_FORGE_SNAPSHOT");
    if forge {
        cmd.env("BP_FORGE_SNAPSHOT", root.join("forge"));
    }
    let out = cmd.arg("list").args(args).arg("--json").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn intent<'a>(v: &'a serde_json::Value, id: &str) -> (&'a str, &'a str) {
    let row = v["rows"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap();
    (row["intent"].as_str().unwrap_or("-"), row["note"].as_str().unwrap_or("-"))
}

#[test]
fn every_live_row_carries_its_intent() {
    let d = vault();
    let v = list(d.path(), &["issue"], false);
    assert_eq!(v["forge"], "not read");
    assert_eq!(intent(&v, "projects/bp/issues/0001.ready"), ("ready", ""));
    assert_eq!(intent(&v, "projects/bp/issues/0002.waits"), ("waiting", "waits for projects/bp/issues/0003.held"));
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("working", "stale"));
    assert_eq!(intent(&v, "projects/bp/issues/0004.done"), ("-", "-"));
    assert_eq!(intent(&v, "projects/bp/issues/0005.dropped"), ("ready", ""));
    assert_eq!(intent(&v, "projects/bp/issues/0006.escalated"), ("blocked", "escalated"));
    let v = list(d.path(), &["milestone"], false);
    assert_eq!(intent(&v, "projects/bp/milestones/m"), ("needs-milestone-approval", ""));
}

#[test]
fn the_forge_snapshot_moves_the_review_path_and_unblocks_dependents() {
    let d = vault();
    let v = list(d.path(), &["issue"], true);
    assert_eq!(v["forge"], "snapshot");
    assert_eq!(intent(&v, "projects/bp/issues/0003.held"), ("needs-review", ""));
    assert_eq!(
        intent(&v, "projects/bp/issues/0002.waits"),
        ("ready", ""),
        "an open pull request on the target is enough"
    );
}

#[test]
fn ready_filters_before_the_limit() {
    let d = vault();
    let v = list(d.path(), &["issue", "--ready"], false);
    let ids: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["projects/bp/issues/0001.ready", "projects/bp/issues/0005.dropped"]);
    assert_eq!((v["total"].as_u64(), v["truncated"].as_bool()), (Some(2), Some(false)));
    let v = list(d.path(), &["issue", "--ready", "--limit", "0"], true);
    assert_eq!((v["total"].as_u64(), v["truncated"].as_bool()), (Some(3), Some(true)));
}

#[test]
fn a_bad_clock_or_snapshot_cannot_run() {
    let d = vault();
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(d.path())
        .env("BP_NOW", "yesterday")
        .args(["list", "issue", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["code"], "could_not_run");
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(d.path())
        .env_remove("BP_NOW")
        .env("BP_FORGE_SNAPSHOT", d.path().join("none"))
        .args(["list", "issue", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
