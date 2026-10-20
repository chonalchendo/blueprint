//! `bp stamp` through the binary: the gate's refusals, a failed block's runs committed alone, then the acceptance in
//! one commit with its trailers and tag, the delta's body in its design and its derived state, and a repeat.

use std::path::Path;

mod common;

use common::{bp, bp_env, commits, git};

const M: &str = "projects/bp/milestones/m";
const S: &str = "projects/bp/designs/system";
const P: &str = "projects/bp/designs/product";

fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let milestone = "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n\n## Finish line\n\n\
                     ~~~check shell {name=p expect=0}\ntrue\n~~~\n\n~~~check shell {name=f expect=0}\nfalse\n~~~\n";
    let vault = common::vault(&[
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\n## One\n\nOld.\n"),
        ("projects/bp/designs/product.md", "---\nkind: design\ntitle: P\nstatus: active\n---\n\nSame.\n"),
        ("projects/bp/milestones/m.md", milestone),
        ("projects/bp/issues/i.md", "---\nkind: issue\ntitle: I\nstatus: open\nunder: projects/bp/milestones/m\n---\n"),
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
    ]);
    (vault, common::vault(&[("marker.txt", "here\n")]))
}

fn stamp(vault: &Path, code: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    bp_env(code, &[("BP_VAULT", vault.to_str().unwrap())], &[&["stamp", M], args].concat())
}

#[test]
fn a_stamp_accepts_the_milestone_in_one_commit() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let sha = git(code, &["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(stamp(vault, code, &[]).1["code"], "illegal_transition");
    let (exit, v) = bp(vault, &["transition", M, "approved"]);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!(stamp(vault, code, &["--waive", "nope", "--reason", "x"]).1["code"], "unknown_value");
    assert_eq!(stamp(vault, code, &["--waive", "f"]).1["code"], "usage");
    assert_eq!(stamp(vault, code, &[]).1["code"], "issues_open");
    assert_eq!(bp(vault, &["transition", "projects/bp/issues/i", "abandoned"]).0, Some(0));
    // `.bp/` is ignored, so the body file dirties no tree.
    let body = vault.join(".bp/body.md");
    std::fs::write(&body, "## One\n\nNew.\n").unwrap();
    let (ms, file) = (format!("milestone={M}"), body.to_str().unwrap());
    let (_, v) = bp(vault, &["record", "delta", S, "--field", &ms, "--field", "change=one line", "--body-file", file]);
    let delta = v["id"].as_str().unwrap().to_string();
    assert_eq!(bp(vault, &["show", &delta]).1["state"], "proposed");
    let before: u32 = commits(vault).parse().unwrap();

    let (exit, v) = stamp(vault, code, &[]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("check_failed")), "{v}");
    assert_eq!(v["subject"], format!("{M}/runs/f-{sha}-1"));
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before + 1, "the runs are committed");
    assert_eq!(bp(vault, &["show", M]).1["status"], "approved");
    assert_eq!(git(vault, &["tag"]), "");

    let (exit, v) = stamp(vault, code, &["--waive", "f", "--reason", "f waived"]);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!((v["version"].as_u64(), v["tag"].as_str()), (Some(1), Some("blueprint/bp/1")));
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before + 2, "one commit");
    let message = git(vault, &["log", "-1", "--format=%B"]);
    for line in ["Bp-Verb: stamp", "Bp-Version: 1", &format!("Bp-Delta: {delta}")] {
        assert!(message.lines().any(|l| l == line), "{line}: {message}");
    }
    assert_eq!(git(vault, &["rev-parse", "blueprint/bp/1^{commit}"]), git(vault, &["rev-parse", "HEAD"]));
    assert_eq!(bp(vault, &["show", M]).1["status"], "done");
    assert_eq!(bp(vault, &["show", &format!("{M}/runs/p-{sha}-2")]).1["verdict"], "pass");
    let waiver = bp(vault, &["show", &format!("{M}/waivers/f")]).1;
    assert_eq!((waiver["reason"].as_str(), waiver.get("valid_until")), (Some("f waived"), None));
    let delta = bp(vault, &["show", &delta]).1;
    assert_eq!((delta["state"].as_str(), delta["sections"].as_array().map(Vec::len)), (Some("accepted"), Some(2)));
    let design = bp(vault, &["show", S]).1;
    assert_eq!(design["body"], "## One\n\nNew.");
    let version = bp(vault, &["show", "projects/bp/versions/1"]).1;
    assert_eq!((version["milestone"].as_str(), version["n"].as_u64()), (Some(M), Some(1)));
    assert_eq!(version["manifest"][S], design["hash"]);
    assert_eq!(version["manifest"][P], bp(vault, &["show", P]).1["hash"]);
    assert!(git(vault, &["status", "--porcelain"]).is_empty());

    let (exit, v) = stamp(vault, code, &[]);
    assert_eq!((exit, v["status"].as_str()), (Some(3), Some("done")), "a repeat: {v}");
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before + 2);
}

#[test]
fn record_names_stamp_for_its_kinds() {
    let (vault, _code) = setup();
    for kind in ["waiver", "blueprint-version"] {
        let (exit, v) = bp(vault.path(), &["record", kind, M]);
        assert_eq!((exit, v["code"].as_str()), (Some(4), Some("usage")), "{kind}");
        assert!(v["fix"].as_str().unwrap().starts_with("bp stamp <milestone>"), "{kind}: {v}");
    }
}

/// Replaces the milestone's finish line with `blocks`, committed by hand, and makes the milestone stampable: its issue
/// abandoned, the milestone approved.
fn ready(vault: &Path, blocks: &str) {
    let text = format!("---\nkind: milestone\ntitle: M\nstatus: planned\n---\n\n## Finish line\n\n{blocks}");
    std::fs::write(vault.join(format!("{M}.md")), text).unwrap();
    git(vault, &["commit", "-qam", "finish line"]);
    assert_eq!(bp(vault, &["transition", "projects/bp/issues/i", "abandoned"]).0, Some(0));
    assert_eq!(bp(vault, &["transition", M, "approved"]).0, Some(0));
}

#[test]
fn a_file_of_the_project_that_does_not_parse_refuses() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    ready(vault, "~~~check shell {name=p expect=0}\ntrue\n~~~\n");
    let broken = "---\nkind: issue\ntitle: B\nstatus: open\nunder: projects/bp/milestones/m\nbad: [line\n---\n";
    std::fs::write(vault.join("projects/bp/issues/broken.md"), broken).unwrap();
    git(vault, &["add", "-A"]);
    git(vault, &["commit", "-qm", "a broken issue"]);
    let (exit, v) = stamp(vault, code, &[]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("schema_invalid")), "{v}");
    assert_eq!(v["subject"], "projects/bp/issues/broken.md");
    assert_eq!(bp(vault, &["show", M]).1["status"], "approved");
}

#[test]
fn refusals_come_before_the_blocks_run_and_a_repeat_makes_a_missing_tag() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    ready(vault, "~~~check shell {name=p expect=0}\ntouch ran\n~~~\n");
    let body = vault.join(".bp/body.md");
    std::fs::write(&body, "## One\n\nNew.\n").unwrap();
    let (ms, file) = (format!("milestone={M}"), body.to_str().unwrap());
    let (_, v) = bp(vault, &["record", "delta", S, "--field", &ms, "--field", "change=one line", "--body-file", file]);
    let delta = v["id"].as_str().unwrap().to_string();
    let before: u32 = commits(vault).parse().unwrap();

    std::fs::write(vault.join(format!("{S}.md")), "---\nkind: design\ntitle: S\nstatus: active\n---\n\nDirty.\n")
        .unwrap();
    assert_eq!(stamp(vault, code, &[]).1["code"], "path_dirty", "a design the delta rewrites");
    git(vault, &["checkout", "--", &format!("{S}.md")]);
    git(vault, &["tag", "blueprint/bp/1"]);
    assert_eq!(stamp(vault, code, &[]).1["code"], "id_taken");
    git(vault, &["tag", "-d", "blueprint/bp/1"]);
    assert!(!code.join("ran").exists(), "no block ran");
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before);

    // A file where the tag's directory goes makes `git tag` fail after the commit.
    let refs = vault.join(".git/refs/tags/blueprint");
    std::fs::create_dir_all(&refs).unwrap();
    std::fs::write(refs.join("bp"), "").unwrap();
    let (exit, v) = stamp(vault, code, &[]);
    assert_eq!((exit, v["code"].as_str()), (Some(1), Some("could_not_run")), "{v}");
    assert!(code.join("ran").exists());
    assert_eq!(bp(vault, &["show", M]).1["status"], "done");
    std::fs::remove_file(refs.join("bp")).unwrap();
    let (exit, v) = stamp(vault, code, &[]);
    assert_eq!((exit, v["tag"].as_str()), (Some(0), Some("blueprint/bp/1")), "the repeat makes the tag: {v}");
    let head = git(vault, &["rev-parse", "HEAD"]);
    assert_eq!(git(vault, &["rev-parse", "blueprint/bp/1^{commit}"]), head);
    assert_eq!(v["commit"].as_str(), Some(head.trim()));
    assert_eq!(stamp(vault, code, &[]).0, Some(3), "the tag exists");

    let text = std::fs::read_to_string(vault.join(format!("{delta}.md"))).unwrap();
    assert!(text.ends_with("version: [[projects/bp/versions/1]]\n"), "a wikilink, so the index holds the edge: {text}");
}

#[test]
fn a_finish_line_changed_while_the_blocks_ran_refuses() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    // The block adds a failing block to its own milestone and commits it by hand, as a write during the run would.
    let late = r"printf '\n\176\176\176check shell {name=late expect=0}\nfalse\n\176\176\176\n'";
    let block = format!("cd \"$BP_VAULT\" && {late} >> {M}.md && git commit -qm late -- {M}.md");
    ready(vault, &format!("~~~check shell {{name=p expect=0}}\n{block}\n~~~\n"));
    let before: u32 = commits(vault).parse().unwrap();
    let (exit, v) = stamp(vault, code, &[]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("evidence_stale")), "{v}");
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before + 1, "only the hand commit");
    assert_eq!(bp(vault, &["show", M]).1["status"], "approved");
}
