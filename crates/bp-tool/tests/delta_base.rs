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
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
        ("shared/threads/u.md", "---\nkind: thread\ntitle: U\nstatus: open\n---\n"),
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
    let (exit, b) = record(root, B, "## One\n\nB's.\n");
    assert_eq!((exit, &b["code"]), (Some(0), &serde_json::Value::Null), "{b}");
    let stale = b["id"].as_str().unwrap().to_string();
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

#[test]
fn a_dirty_spec_refuses_path_dirty_before_base_moved() {
    let spec = "type: architecture\nlevel: container\ntitle: One\nlede: One box.\nnodes:\n  - {id: o, noun: One, at: [0, 0]}\n";
    let path = format!("{S}/diagrams/one.yaml");
    let v = vault(&[(&path, spec)]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    assert_eq!(bp(root, &["transition", B, "approved"]).0, Some(0));
    assert_eq!(record(root, B, "## One\n\nB's.\n").0, Some(0));
    std::fs::write(root.join(&path), spec.replace("One box.", "Dirty.")).unwrap();
    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", B]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("path_dirty")), "{v}");
}

#[test]
fn a_stale_delta_of_another_cause_still_refuses_delta_pending() {
    let v = vault(&[]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    for m in [A, B] {
        assert_eq!(bp(root, &["transition", m, "approved"]).0, Some(0));
    }
    let (t, u) = ("cause=shared/threads/t", "cause=shared/threads/u");
    let (_, stale) = record_with(root, B, "## One\n\nT's.\n", &["--field", t]);
    let stale = stale["id"].as_str().unwrap().to_string();
    assert_eq!(record(root, A, "## One\n\nA's.\n").0, Some(0));
    assert_eq!(bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", A]).0, Some(0));
    // Another cause cannot take the stale delta's place: its change would be lost without a word.
    let (exit, v) = record_with(root, B, "## One\n\nU's.\n", &["--field", u]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("delta_pending")), "{v}");
    let fix = v["fix"].as_str().unwrap();
    assert!(fix.starts_with(&format!("{stale} can never be stamped")) && fix.contains(t), "{fix}");
    assert_eq!(bp(root, &["show", &stale]).1["state"], "proposed");
    let (exit, v) = record_with(root, B, "## One\n\nA's, then T's.\n", &["--field", t]);
    assert_eq!((exit, v["rejected"].as_str()), (Some(0), Some(stale.as_str())), "the same cause: {v}");
}

#[test]
fn a_header_change_leaves_the_base() {
    let v = vault(&[]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    assert_eq!(bp(root, &["transition", B, "approved"]).0, Some(0));
    assert_eq!(record(root, B, "## One\n\nB's.\n").0, Some(0));
    assert_eq!(bp(root, &["set", S, "title", "S two"]).0, Some(0));
    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", B]);
    assert_eq!(exit, Some(0), "the stamp rewrites the body only, so a title changes nothing it reverts: {v}");
}

const W: (&str, &str) = ("shared/threads/w.md", "---\nkind: thread\ntitle: W\nstatus: open\n---\n");

/// The `base` a recorded delta holds.
fn base(root: &Path, delta: &str) -> serde_json::Value {
    bp(root, &["show", delta, "--no-body"]).1["base"].clone()
}

/// The id `record` returned, after it exited 0.
fn id((exit, v): (Option<i32>, serde_json::Value)) -> String {
    assert_eq!(exit, Some(0), "{v}");
    v["id"].as_str().unwrap().to_string()
}

#[test]
fn delta_chains_within_milestone() {
    let v = vault(&[W]);
    let root = v.path();
    assert_eq!(bp(root, &["transition", B, "approved"]).0, Some(0));
    let (t, u, w) = ("cause=shared/threads/t", "cause=shared/threads/u", "cause=shared/threads/w");
    let first = id(record_with(root, B, "## One\n\nT's.\n", &["--field", t]));
    // Chaining needs the field: the record names the tip to read.
    let (exit, v) = record_with(root, B, "## One\n\nT's, then U's.\n", &["--field", u]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("delta_pending")), "{v}");
    let fix = v["fix"].as_str().unwrap();
    assert!(
        fix.contains(&format!("bp show {first} --json")) && fix.contains(&format!("--field chains={first}")),
        "{fix}"
    );
    let on = format!("chains={first}");
    let (exit, v) = record_with(root, B, "## One\n\nT's, then U's.\n", &["--field", u, "--field", &on]);
    assert_eq!((exit, v["chains"].as_str()), (Some(0), Some(first.as_str())), "the result says it chained: {v}");
    let second = v["id"].as_str().unwrap().to_string();
    let shown = bp(root, &["show", &second, "--no-body"]).1;
    assert_eq!(shown["chains"].as_str(), Some(first.as_str()), "{shown}");
    let (_, listed) = bp(root, &["list", "delta", "--under", S]);
    let row = listed["rows"].as_array().unwrap().iter().find(|r| r["id"] == second.as_str()).unwrap().clone();
    assert_eq!((row["milestone"].as_str(), row["cause"].as_str()), (Some(B), Some("shared/threads/u")), "{row}");
    assert_eq!(row["chains"].as_str(), Some(first.as_str()), "{row}");

    // The second's base is the hash of the design as the first leaves it: a delta's base on a design with that body.
    let on = vault(&[]);
    let design = "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nT's.\n";
    std::fs::write(on.path().join(format!("{S}.md")), design).unwrap();
    common::git(on.path(), &["commit", "-qam", "T's"]);
    assert_eq!(bp(on.path(), &["transition", B, "approved"]).0, Some(0));
    let plain = id(record(on.path(), B, "## One\n\nAny.\n"));
    assert_eq!(base(root, &second), base(on.path(), &plain));
    assert_ne!(base(root, &second), base(root, &first));

    let (exit, v) = record_with(root, B, "## One\n\nT's again.\n", &["--field", t]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("delta_pending")), "the first cause: {v}");
    let fork = format!("chains={first}");
    let (exit, v) = record_with(root, B, "## One\n\nT's, then W's.\n", &["--field", w, "--field", &fork]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("delta_pending")), "a fork: {v}");
    assert!(v["fix"].as_str().unwrap().contains(&second), "the fix names the tip: {v}");
    let tip = format!("chains={second}");
    let third = id(record_with(root, B, "## One\n\nT's, U's, then W's.\n", &["--field", w, "--field", &tip]));
    assert_eq!(bp(root, &["show", &third, "--no-body"]).1["chains"].as_str(), Some(second.as_str()));

    // An editorial delta never chains.
    let body = root.join(".bp/editorial.md");
    std::fs::write(&body, "## One\n\nOld,\n").unwrap();
    let (exit, v) =
        bp(root, &["record", "delta", S, "--editorial", "--field", &fork, "--body-file", body.to_str().unwrap()]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("delta_pending")), "{v}");
}

#[test]
fn stamp_accepts_chain_in_order() {
    let spec = "type: architecture\nlevel: container\ntitle: One\nlede: One box.\nnodes:\n  - {id: o, noun: One, at: [0, 0]}\n";
    let path = format!("{S}/diagrams/one.yaml");
    let v = vault(&[(&path, spec)]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    assert_eq!(bp(root, &["transition", B, "approved"]).0, Some(0));
    let file = root.join(".bp/one.yaml");
    let changed = spec.replace("lede: One box.", "lede: T's box.");
    std::fs::write(&file, &changed).unwrap();
    let flag = format!("one={}", file.display());
    let t = ["--field", "cause=shared/threads/t", "--diagram", &flag];
    let first = id(record_with(root, B, "## One\n\nT's.\n", &t));
    let on = format!("chains={first}");
    let u = ["--field", "cause=shared/threads/u", "--field", &on];
    let second = id(record_with(root, B, "## One\n\nT's, then U's.\n", &u));

    let (exit, v) = bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", B]);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!(v["deltas"], serde_json::json!([first, second]), "in chain order: {v}");
    assert_eq!(bp(root, &["show", S]).1["body"], "## One\n\nT's, then U's.");
    assert_eq!(std::fs::read_to_string(root.join(&path)).unwrap(), changed, "the first's spec holds");
    for d in [&first, &second] {
        assert_eq!(bp(root, &["show", d]).1["state"], "accepted");
    }
}

#[test]
fn a_moved_link_refuses_and_its_cause_records_it_again() {
    let v = vault(&[]);
    let code = common::vault(&[("marker.txt", "here\n")]);
    let (root, code) = (v.path(), code.path());
    let stamp = |m: &str| bp_env(code, &[("BP_VAULT", root.to_str().unwrap())], &["stamp", m]);
    for m in [A, B] {
        assert_eq!(bp(root, &["transition", m, "approved"]).0, Some(0));
    }
    let (t, u) = (["--field", "cause=shared/threads/t"], ["--field", "cause=shared/threads/u"]);
    let first = id(record_with(root, B, "## One\n\nT's.\n", &t));
    let on = format!("chains={first}");
    let second = id(record_with(root, B, "## One\n\nT's, then U's.\n", &[&u[..], &["--field", &on]].concat()));
    assert_eq!(record(root, A, "## One\n\nA's.\n").0, Some(0));
    assert_eq!(stamp(A).0, Some(0));
    let (exit, v) = stamp(B);
    assert_eq!((exit, v["code"].as_str(), v["subject"].as_str()), (Some(2), Some("base_moved"), Some(first.as_str())));

    // The first's cause proposes it again on the new design; the second now chains on a rejected delta.
    let (exit, v) = record_with(root, B, "## One\n\nA's, then T's.\n", &t);
    assert_eq!((exit, v["rejected"].as_str()), (Some(0), Some(first.as_str())), "{v}");
    let renewed = v["id"].as_str().unwrap().to_string();
    let (exit, v) = stamp(B);
    assert_eq!((exit, v["code"].as_str(), v["subject"].as_str()), (Some(2), Some("base_moved"), Some(second.as_str())));
    // The fix names the link before it now, not the design.
    let fix = v["fix"].as_str().unwrap();
    assert!(
        fix.contains(&format!("bp show {renewed} --json")) && fix.contains(&format!("--field chains={renewed}")),
        "{fix}"
    );
    let on = format!("chains={renewed}");
    let (exit, v) = record_with(root, B, "## One\n\nA's, T's, then U's.\n", &[&u[..], &["--field", &on]].concat());
    assert_eq!((exit, v["rejected"].as_str()), (Some(0), Some(second.as_str())), "{v}");
    assert_eq!(stamp(B).0, Some(0));
    assert_eq!(bp(root, &["show", S]).1["body"], "## One\n\nA's, T's, then U's.");
}

#[test]
fn a_chained_body_that_drops_the_chains_change_refuses_chain_reverts() {
    let v = vault(&[W]);
    let root = v.path();
    assert_eq!(bp(root, &["transition", B, "approved"]).0, Some(0));
    let t = ["--field", "cause=shared/threads/t"];
    let first = id(record_with(root, B, "## One\n\nOld, and T's line.\n\n## T\n\nT's section.\n", &t));
    let on = format!("chains={first}");
    let u = ["--field", "cause=shared/threads/u", "--field", &on];

    // Built on the design, not the tip: the section T added is gone.
    let (exit, v) = record_with(root, B, "## One\n\nOld, and T's line.\n\n## U\n\nU's.\n", &u);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("chain_reverts")), "{v}");
    assert_eq!(v["subject"].as_str(), Some(format!("{first}#T").as_str()), "{v}");
    let fix = v["fix"].as_str().unwrap();
    assert!(fix.contains("adds the section") && fix.contains(&format!("bp show {first} --json")), "{fix}");
    // The section T changed reads as committed again.
    let (exit, v) = record_with(root, B, "## One\n\nOld.\n\n## T\n\nT's section.\n", &u);
    assert_eq!(
        (exit, v["code"].as_str(), v["subject"].as_str()),
        (Some(2), Some("chain_reverts"), Some(format!("{first}#One").as_str()))
    );
    assert!(v["fix"].as_str().unwrap().contains("puts back the committed text"), "{v}");
    // On top of the tip, a further change to T's section is the new delta's to make.
    let body = "## One\n\nOld, and T's line, and U's.\n\n## T\n\nT's section, and U's.\n";
    assert_eq!(record_with(root, B, body, &u).0, Some(0));
}
