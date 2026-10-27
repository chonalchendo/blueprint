//! `cargo xtask stage` through the binary (32, design change 11): the staged copy holds every intent the view must
//! show, and `list issue` on it prints sorted keys and no `null` at any level (the pin of `bp-tool/tests/show.rs`,
//! on live rows). It copies the converted vault, so it needs `BP_VAULT` and `target/release/bp`:
//! `BP_VAULT=/tmp/vault-target-6 cargo test -p xtask --test stage -- --ignored`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "needs BP_VAULT and target/release/bp"]
fn stage_makes_one_row_of_each_intent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xtask")).arg("stage").current_dir(root).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: BTreeMap<&str, &str> = text.lines().map(|l| l.split_once('=').unwrap()).collect();
    assert_eq!(lines.keys().copied().collect::<Vec<_>>(), ["CLONE", "FORGE", "VAULT"], "{text}");
    assert_ne!(
        Path::new(lines["VAULT"]).canonicalize().unwrap(),
        Path::new(&std::env::var("BP_VAULT").unwrap()).canonicalize().unwrap()
    );

    let list = |kind: &str| {
        let out = Command::new(root.join("target/release/bp"))
            .args(["list", kind, "--project", "bp", "--limit", "100000", "--json"])
            .env("BP_VAULT", lines["VAULT"])
            .output()
            .unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains(":null"), "a skipped field printed null: list {kind}");
        // The envelope appends `contract_version` after the result's keys; the result's own keys print sorted at
        // every level, which re-serialising through a `Value` gives.
        let result = format!("{}}}", text.trim_end().rsplit_once(",\"contract_version\":").unwrap().0);
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(result, serde_json::to_string(&value).unwrap(), "unsorted keys: list {kind}");
        value
    };
    let issues = list("issue");
    assert_eq!(issues["forge"], "snapshot", "the repo verbs wrote the snapshot");
    let milestones = list("milestone");
    let intents: Vec<&str> = [&issues, &milestones]
        .iter()
        .flat_map(|v| v["rows"].as_array().unwrap())
        .filter_map(|r| r["intent"].as_str())
        .collect();
    for want in ["ready", "working", "needs-landing", "waiting", "blocked", "needs-milestone-approval"] {
        assert!(intents.contains(&want), "no {want} row on the staged copy");
    }
}
