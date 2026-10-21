//! A delta has a base (28, design change 8) through the binary: `record delta` stores the design's hash, `stamp`
//! refuses `base_moved` when the design changed since, and the fix, a delta on the new base, closes the stale one.

mod common;

use std::path::Path;

use common::{bp, bp_env, commits};

const A: &str = "projects/bp/milestones/a";
const B: &str = "projects/bp/milestones/b";
const S: &str = "projects/bp/designs/system";

fn milestone(title: &str) -> String {
    format!(
        "---\nkind: milestone\ntitle: {title}\nstatus: planned\n---\n\n## Finish line\n\n~~~check shell {{name=p expect=0}}\ntrue\n~~~\n"
    )
}

/// `bp record delta <S>` under `milestone` with `body` and `extra` flags.
fn record_with(root: &Path, milestone: &str, body: &str, extra: &[&str]) -> (Option<i32>, serde_json::Value) {
    let file = root.join(".bp/body.md");
    std::fs::write(&file, body).unwrap();
    let ms = format!("milestone={milestone}");
    bp(root, &[&["record", "delta", S, "--field", &ms, "--body-file", file.to_str().unwrap()], extra].concat())
}

fn record(root: &Path, milestone: &str, body: &str) -> (Option<i32>, serde_json::Value) {
    record_with(root, milestone, body, &[])
}

/// A project, the active design [`S`] with `extra` files beside it, and the planned milestones [`A`] and [`B`].
fn vault(extra: &[(&str, &str)]) -> tempfile::TempDir {
    let (s, a, b) = (format!("{S}.md"), format!("{A}.md"), format!("{B}.md"));
    let files = [
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        (s.as_str(), "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n"),
        (a.as_str(), &milestone("A")),
        (b.as_str(), &milestone("B")),
        ("shared/.keep", ""),
    ];
    common::vault(&[&files[..], extra].concat())
}

#[test]
fn refuses_base_moved() {
    let v = vault(&[]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let stamp = |m: &str| bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", m]);
    for m in [A, B] {
        assert_eq!(bp(root, &["transition", m, "approved"]).0, Some(0));
    }
    // With no spec, the base is the design file's hash.
    let base = bp(root, &["show", S]).1["hash"].clone();
    let (exit, b) = record(root, B, "## One\n\nB's.\n");
    assert_eq!((exit, &b["code"]), (Some(0), &serde_json::Value::Null), "{b}");
    let stale = b["id"].as_str().unwrap().to_string();
    assert_eq!(bp(root, &["show", &stale]).1["base"], base);
    let (exit, v) = record(root, B, "## One\n\nAgain.\n");
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("delta_pending")), "the base holds: {v}");
    assert_eq!(record(root, A, "## One\n\nA's.\n").0, Some(0));
    assert_eq!(stamp(A).0, Some(0), "A's delta is on the design as it is");

    // B's delta was proposed on the design before A's change: writing it would revert A's without a word.
    let before = commits(root);
    let (exit, v) = stamp(B);
    assert_eq!(
        (exit, v["code"].as_str(), v["subject"].as_str()),
        (Some(2), Some("base_moved"), Some(stale.as_str())),
        "{v}"
    );
    let fix = v["fix"].as_str().unwrap();
    assert!(fix.contains(&format!("bp record delta {S} --field milestone={B} --body-file <f>")), "{fix}");
    assert_eq!(commits(root), before, "nothing is committed");
    assert_eq!(bp(root, &["show", S]).1["body"], "## One\n\nA's.");

    // The fix: a delta on the new base, which closes the stale one as rejected.
    let (exit, v) = record(root, B, "## One\n\nA's, then B's.\n");
    assert_eq!((exit, v["rejected"].as_str()), (Some(0), Some(stale.as_str())), "{v}");
    assert_eq!(bp(root, &["show", &stale]).1["state"], "rejected");
    let (exit, v) = stamp(B);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!(v["deltas"].as_array().map(Vec::len), Some(1), "only the delta on the new base: {v}");
    assert_eq!(bp(root, &["show", S]).1["body"], "## One\n\nA's, then B's.");
}

#[test]
fn refuses_base_moved_after_a_spec_change() {
    let spec = "type: architecture\nlevel: container\ntitle: One\nlede: One box.\nnodes:\n  - {id: o, noun: One, at: [0, 0]}\n";
    let v = vault(&[(&format!("{S}/diagrams/one.yaml"), spec)]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let stamp = |m: &str| bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", m]);
    for m in [A, B] {
        assert_eq!(bp(root, &["transition", m, "approved"]).0, Some(0));
    }
    let (exit, b) = record(root, B, "## One\n\nB's.\n");
    assert_eq!(exit, Some(0), "{b}");
    // A's delta keeps the body and changes only the spec, so the design file stays as it was.
    let file = root.join(".bp/one.yaml");
    std::fs::write(&file, spec.replace("lede: One box.", "lede: The one box.")).unwrap();
    let flag = format!("one={}", file.display());
    assert_eq!(record_with(root, A, "## One\n\nOld.\n", &["--diagram", &flag]).0, Some(0));
    let text = std::fs::read_to_string(root.join(format!("{S}.md"))).unwrap();
    assert_eq!(stamp(A).0, Some(0));
    assert_eq!(std::fs::read_to_string(root.join(format!("{S}.md"))).unwrap(), text, "the design file is unchanged");

    let (exit, v) = stamp(B);
    assert_eq!(
        (exit, v["code"].as_str(), v["subject"].as_str()),
        (Some(2), Some("base_moved"), b["id"].as_str()),
        "{v}"
    );
}
