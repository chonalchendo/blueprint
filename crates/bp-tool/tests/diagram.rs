//! `bp diagram` in its preview form through the binary: the result and its warnings, the `diagram_invalid` envelope
//! with every finding, the diff render, and nothing written but the file it was asked for.

use std::fmt::Write as _;
use std::path::Path;

/// `bp <args> --json` in `dir`, outside any vault: the exit code and the parsed result.
fn bp(dir: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    bp_in(dir, None, args)
}

/// The same, with `BP_VAULT` set to `vault` when one is given.
fn bp_in(dir: &Path, vault: Option<&Path>, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let mut cmd = assert_cmd::Command::cargo_bin("bp").unwrap();
    cmd.current_dir(dir).env_remove("BP_VAULT");
    if let Some(v) = vault {
        cmd.env("BP_VAULT", v);
    }
    let out = cmd.args(args).arg("--json").output().unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

const SPEC: &str = "type: architecture\nlevel: container\ntitle: Quokka\nlede: One box.\nnodes:\n  - {id: v, term: vault, at: [0, 0]}\n";

fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

#[test]
fn the_preview_writes_its_file_and_lists_the_warnings() {
    let dir = dir_with(&[("s.yaml", SPEC)]);
    let (code, v) = bp(dir.path(), &["diagram", "--spec-file", "s.yaml", "--out", "s.html"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["out"], "s.html");
    assert_eq!(v["findings"].as_array().unwrap().len(), 1, "one warning: no vault, so no term was checked: {v}");
    assert_eq!(
        (v["findings"][0]["rule"].as_str(), v["findings"][0]["severity"].as_str()),
        (Some("nodes/resolve"), Some("warn"))
    );
    let html = std::fs::read_to_string(dir.path().join("s.html")).unwrap();
    assert_eq!(v["bytes"], html.len());
    assert!(html.contains("data-id=\"v\" data-term=\"vault\""));
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 2, "the spec and the one output: {names:?}");
}

#[test]
fn a_budget_is_a_warning_with_its_severity() {
    let mut spec = SPEC.replace("nodes:\n  - {id: v, term: vault, at: [0, 0]}\n", "nodes:\n");
    for i in 0..10 {
        let _ = writeln!(spec, "  - {{id: n{i}, noun: N{i}, at: [{}, {}]}}", i % 4, i / 4);
    }
    let dir = dir_with(&[("s.yaml", &spec)]);
    let (code, v) = bp(dir.path(), &["diagram", "--spec-file", "s.yaml", "--out", "s.html"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(v["findings"][0]["rule"], "budget/architecture");
    assert_eq!(v["findings"][0]["severity"], "warn");
}

#[test]
fn a_broken_rule_refuses_diagram_invalid_with_every_finding() {
    let spec = SPEC.replace("lede: One box.\n", "focus: [v, w, x]\n") + "  - {id: w, noun: W, at: [0, 0]}\n";
    let dir = dir_with(&[("s.yaml", &spec)]);
    let (code, v) = bp(dir.path(), &["diagram", "--spec-file", "s.yaml", "--out", "s.html"]);
    assert_eq!(code, Some(2), "{v}");
    assert_eq!(v["code"], "diagram_invalid");
    assert_eq!(v["exit"], 2);
    let rules: Vec<&str> = v["findings"].as_array().unwrap().iter().map(|f| f["rule"].as_str().unwrap()).collect();
    for rule in ["focus/count", "a11y/title", "layout/overlap"] {
        assert!(rules.contains(&rule), "{rule} in {rules:?}");
    }
    assert!(v["findings"].as_array().unwrap().iter().all(|f| f["severity"] == "error" && f["id"].is_string()));
    assert!(v["fix"].as_str().unwrap().contains("bp diagram --spec-file s.yaml"));
    assert!(!dir.path().join("s.html").exists(), "a refusal writes nothing");
}

#[test]
fn the_diff_render_marks_the_change() {
    let head = SPEC.to_string() + "  - {id: w, noun: W, at: [1, 0]}\n";
    let dir = dir_with(&[("base.yaml", SPEC), ("head.yaml", &head)]);
    let (code, v) = bp(dir.path(), &["diagram", "--base", "base.yaml", "--spec-file", "head.yaml", "--out", "d.html"]);
    assert_eq!(code, Some(0), "{v}");
    let html = std::fs::read_to_string(dir.path().join("d.html")).unwrap();
    assert!(html.contains("added\" data-id=\"w\""));
}

#[test]
fn an_unreadable_spec_could_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let (code, v) = bp(dir.path(), &["diagram", "--spec-file", "none.yaml", "--out", "x.html"]);
    assert_eq!((code, v["code"].as_str()), (Some(1), Some("could_not_run")));
}

#[test]
fn with_a_vault_a_term_resolves_and_the_page_stays_outside() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(vault.path().join("projects/bp/designs")).unwrap();
    std::fs::create_dir_all(vault.path().join("shared")).unwrap();
    std::fs::write(vault.path().join("projects/bp/designs/glossary.md"), "# Glossary\n\n- `vault` — the files.\n")
        .unwrap();
    let dir = dir_with(&[("s.yaml", SPEC), ("bad.yaml", &SPEC.replace("term: vault", "term: no-such-term"))]);
    let (code, v) = bp_in(dir.path(), Some(vault.path()), &["diagram", "--spec-file", "s.yaml", "--out", "s.html"]);
    assert_eq!((code, v["findings"].clone()), (Some(0), serde_json::json!([])), "{v}");
    let (code, v) = bp_in(dir.path(), Some(vault.path()), &["diagram", "--spec-file", "bad.yaml", "--out", "b.html"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("diagram_invalid")), "{v}");
    assert_eq!(v["findings"][0]["rule"], "nodes/resolve");
    let inside = vault.path().join("projects/x.html");
    let (code, v) =
        bp_in(dir.path(), Some(vault.path()), &["diagram", "--spec-file", "s.yaml", "--out", inside.to_str().unwrap()]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("usage")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("such as /tmp/x.html"), "{v}");
    assert!(!inside.exists(), "a refusal writes nothing");
}
