//! `bp rename` through the binary, over a small vault under git: the move, every inbound link rewritten, the alias
//! the old slug leaves, the repeat and the refusals.

mod common;

use common::{bp, commits, git};

const FRONT: &str = "projects/bp/designs/containers/front";
const NEW: &str = "projects/bp/designs/containers/door";
const A: &str = "projects/bp/issues/0001.a";
const CLAIM: &str = "projects/bp/issues/0001.a/claims/2026-07-25T23-56-17Z";

fn vault() -> tempfile::TempDir {
    common::vault(&[
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\n---\n"),
        ("projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n\nThe goal.\n"),
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n"),
        (
            "projects/bp/designs/containers/front.md",
            "---\nkind: design\ntitle: Front\nstatus: active\naliases: [\"older-front\"]\n---\n\n## Why\n\n\
             Self: [[projects/bp/designs/containers/front#Why]], a [[broken.link]].\n\n\
             [d](/projects/bp/designs/containers/front/diagrams/d.html)\n",
        ),
        ("projects/bp/designs/containers/front/diagrams/d.html", "<p>d</p>\n"),
        ("projects/bp/designs/containers/front/diagrams/d.nodes.yaml", "nodes: []\n"),
        ("projects/bp/designs/containers/front-end.md", "---\nkind: design\ntitle: FE\nstatus: active\n---\n"),
        (
            "projects/bp/issues/0001.a.md",
            "---\nkind: issue\ntitle: A\nstatus: open\nunder: projects/bp/milestones/m\n\
             cites: [\"projects/bp/designs/containers/front\"]\n---\n\n\
             See [[projects/bp/designs/containers/front|the front]], [[projects/bp/designs/containers/front#Why]],\n\
             [[projects/bp/designs/containers/older-front]] and [[projects/bp/designs/containers/front-end]].\r\n\
             [d](/projects/bp/designs/containers/front/diagrams/d.html)\n",
        ),
        (
            "projects/bp/issues/0001.a/claims/2026-07-25T23-56-17Z.md",
            "---\nkind: claim\nunder: projects/bp/issues/0001.a\nfrom: 2026-07-25T23:56:17Z\nowner: x\n---\n",
        ),
        (
            "projects/bp/issues/0002.b.md",
            "---\nkind: issue\ntitle: B\nstatus: open\ndepends: [\"projects/bp/issues/0001.a\"]\n---\n",
        ),
    ])
}

fn read(root: &std::path::Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn rename_moves_the_node_and_its_directory_and_rewrites_every_link_in_one_commit() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["rename", FRONT, "door", "--dry-run"]);
    assert_eq!((code, v["id"].as_str(), v["dry_run"].as_bool()), (Some(0), Some(NEW), Some(true)), "{v}");
    assert_eq!(
        (commits(root), git(root, &["status", "--porcelain"])),
        ("1".into(), String::new()),
        "a dry run writes nothing"
    );
    let (code, v) = bp(root, &["rename", FRONT, "door"]);
    assert_eq!(code, Some(0), "{v}");
    assert_eq!(
        (v["id"].as_str(), v["was"].as_str(), v["moved"].as_u64(), v["rewritten"].as_u64()),
        (Some(NEW), Some(FRONT), Some(3), Some(1))
    );
    assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim());
    assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: rename"));
    let a = read(root, &format!("{A}.md"));
    let want = "---\nkind: issue\ntitle: A\nstatus: open\nunder: projects/bp/milestones/m\n\
                cites: [\"projects/bp/designs/containers/door\"]\n---\n\n\
                See [[projects/bp/designs/containers/door|the front]], [[projects/bp/designs/containers/door#Why]],\n\
                [[projects/bp/designs/containers/door]] and [[projects/bp/designs/containers/front-end]].\r\n\
                [d](/projects/bp/designs/containers/door/diagrams/d.html)\n";
    assert_eq!(a, want, "header link, label, section, alias, markdown link; the near miss and every other byte stay");
    let moved = read(root, &format!("{NEW}.md"));
    assert!(moved.contains("aliases: [\"older-front\", \"front\"]\n"), "{moved}");
    assert!(
        moved.contains("[[projects/bp/designs/containers/door#Why]]")
            && moved.contains("](/projects/bp/designs/containers/door/diagrams/d.html)")
    );
    assert_eq!(read(root, &format!("{NEW}/diagrams/d.html")), "<p>d</p>\n");
    assert!(!root.join(format!("{FRONT}.md")).exists() && !root.join(FRONT).exists(), "the old file and directory go");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    assert_eq!(commits(root), "2");
    let shown = bp(root, &["show", FRONT]).1;
    assert_eq!(shown["id"], NEW, "the old id is an alias of the node");
    assert_eq!(bp(root, &["show", A]).1["cites"], serde_json::json!([NEW]), "the index took the write");
    let (code, v) = bp(root, &["rename", FRONT, "door"]);
    assert_eq!(
        (code, v),
        (Some(3), serde_json::json!({ "id": NEW, "contract_version": 1 })),
        "the same slug, through the alias"
    );
    let warm = bp(root, &["list", "--count-only"]).1;
    std::fs::remove_file(root.join(".bp/index.db")).unwrap();
    assert_eq!(bp(root, &["list", "--count-only"]).1, warm, "the index after the write is a rebuild");
}

#[test]
fn a_renamed_issue_takes_its_records_and_their_parent_link() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["rename", A, "0001.renamed"]);
    assert_eq!(code, Some(0), "{v}");
    let claim = read(root, "projects/bp/issues/0001.renamed/claims/2026-07-25T23-56-17Z.md");
    assert!(claim.contains("under: projects/bp/issues/0001.renamed\n"), "{claim}");
    assert!(read(root, "projects/bp/issues/0002.b.md").contains("depends: [\"projects/bp/issues/0001.renamed\"]"));
    assert_eq!(bp(root, &["show", "projects/bp/issues/0001.renamed/claims/2026-07-25T23-56-17Z"]).0, Some(0));
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

/// Asserts a refusal with `code`, and that it moved neither the head nor the working tree.
fn refused(root: &std::path::Path, args: &[&str], code: &str) {
    let head = git(root, &["rev-parse", "HEAD"]);
    let status = git(root, &["status", "--porcelain"]);
    let (exit, v) = bp(root, args);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some(code)), "{args:?}: {v}");
    assert_eq!((git(root, &["rev-parse", "HEAD"]), git(root, &["status", "--porcelain"])), (head, status), "{args:?}");
}

#[test]
fn rename_refuses_a_taken_id_a_record_a_bad_slug_and_a_dirty_target() {
    let dir = vault();
    let root = dir.path();
    refused(root, &["rename", FRONT, "front-end"], "id_taken");
    refused(root, &["rename", "projects/bp/designs/containers/front-end", "older-front"], "id_taken");
    refused(root, &["rename", CLAIM, "later"], "schema_invalid");
    refused(root, &["rename", FRONT, "Door"], "schema_invalid");
    refused(root, &["rename", "projects/bp/designs/system", "sys"], "schema_invalid");
    refused(root, &["rename", "projects/bp/issues/nope", "x"], "no_node");
    // An inbound file and the node itself are both targets; the edit stays on disk, uncommitted.
    for target in [A, FRONT] {
        let path = root.join(format!("{target}.md"));
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{text}\nAn edit.\n")).unwrap();
        refused(root, &["rename", FRONT, "door"], "path_dirty");
        git(root, &["checkout", "--", "."]);
    }
    assert_eq!(commits(root), "1");
}

#[test]
fn a_wrong_case_id_renames_the_node_the_index_holds() {
    let dir = vault();
    let root = dir.path();
    let (code, v) = bp(root, &["rename", "projects/bp/designs/containers/FRONT", "door"]);
    assert_eq!((code, v["id"].as_str(), v["was"].as_str()), (Some(0), Some(NEW), Some(FRONT)), "{v}");
    assert_eq!(v["rewritten"], 1, "the inbound file follows: {v}");
    assert!(read(root, &format!("{A}.md")).contains("cites: [\"projects/bp/designs/containers/door\"]"));
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    let warm = bp(root, &["list", "--count-only"]).1;
    std::fs::remove_file(root.join(".bp/index.db")).unwrap();
    assert_eq!(bp(root, &["list", "--count-only"]).1, warm, "the index after the write is a rebuild");
}

#[test]
fn a_directory_already_at_the_new_path_refuses_id_taken() {
    let dir = vault();
    let root = dir.path();
    std::fs::create_dir_all(root.join(NEW)).unwrap();
    std::fs::write(root.join(format!("{NEW}/notes.txt")), "human notes\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "notes"]);
    let (_, v) = bp(root, &["rename", FRONT, "door"]);
    assert!(v["fix"].as_str().unwrap().contains(&format!("{NEW}/ is taken")), "the fix names the directory: {v}");
    refused(root, &["rename", FRONT, "door"], "id_taken");
    // Only in HEAD: the human removed it and has not committed.
    std::fs::remove_dir_all(root.join(NEW)).unwrap();
    refused(root, &["rename", FRONT, "door"], "id_taken");
    git(root, &["checkout", "--", "."]);
    assert_eq!(read(root, &format!("{NEW}/notes.txt")), "human notes\n");
}

#[test]
fn every_whole_mention_is_rewritten_in_code_and_prose_and_tracked_dot_files_move() {
    let dir = vault();
    let root = dir.path();
    let prose = "---\nkind: thread\ntitle: P\nstatus: open\n---\n\n```\nbp show projects/bp/designs/containers/front\n```\n\n\
                 Run `projects/bp/designs/containers/front` and read projects/bp/designs/containers/front.\n";
    std::fs::write(root.join("shared/threads/p.md"), prose).unwrap();
    std::fs::write(root.join(format!("{FRONT}/.keep")), "").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "prose"]);
    let (code, v) = bp(root, &["rename", FRONT, "door", "--dry-run"]);
    assert_eq!(code, Some(0), "{v}");
    let pair = |from: &str, to: &str| serde_json::json!({ "from": from, "to": to });
    assert_eq!(
        v["files"],
        serde_json::json!({
            "moved": [
                pair(&format!("{FRONT}.md"), &format!("{NEW}.md")),
                pair(&format!("{FRONT}/.keep"), &format!("{NEW}/.keep")),
                pair(&format!("{FRONT}/diagrams/d.html"), &format!("{NEW}/diagrams/d.html")),
                pair(&format!("{FRONT}/diagrams/d.nodes.yaml"), &format!("{NEW}/diagrams/d.nodes.yaml")),
            ],
            "rewritten": [format!("{A}.md"), "shared/threads/p.md".to_string()],
        }),
        "the dry run lists the change"
    );
    let (code, v) = bp(root, &["rename", FRONT, "door"]);
    assert_eq!((code, v["rewritten"].as_u64()), (Some(0), Some(2)), "{v}");
    assert_eq!(read(root, "shared/threads/p.md"), prose.replace(FRONT, NEW), "the fence, the code span and the prose");
    assert_eq!(git(root, &["ls-files", "--", FRONT]), "", "no tracked file keeps the old directory");
    assert!(root.join(format!("{NEW}/.keep")).exists() && !root.join(FRONT).exists());
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn an_inbox_rename_keeps_the_creation_date() {
    let dir = common::vault(&[
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n"),
        ("shared/inbox/2026-05-11-carve.md", "---\nkind: inbox\ntitle: C\n---\n"),
    ]);
    let root = dir.path();
    refused(root, &["rename", "shared/inbox/2026-05-11-carve", "1999-01-01-carve"], "schema_invalid");
    let (code, v) = bp(root, &["rename", "shared/inbox/2026-05-11-carve", "trim"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some("shared/inbox/2026-05-11-trim")), "{v}");
    let (code, v) = bp(root, &["rename", "shared/inbox/2026-05-11-trim", "2026-05-11-cut"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some("shared/inbox/2026-05-11-cut")), "{v}");
}

#[test]
fn every_verb_takes_an_old_id_and_a_node_may_take_its_past_slug_back() {
    let dir = vault();
    let root = dir.path();
    // The alias line the rename adds costs the seed a few tokens; the rest of the walk is the same.
    let walk = || {
        let (code, mut v) = bp(root, &["walk", A, "--for", "complete"]);
        assert_eq!(code, Some(0), "{v}");
        v.as_object_mut().unwrap().remove("used");
        v.to_string()
    };
    let walked = walk();
    assert_eq!(bp(root, &["rename", A, "0001.renamed"]).0, Some(0));
    let renamed = "projects/bp/issues/0001.renamed";
    assert_eq!(walk(), walked.replace(A, renamed), "the walk through the old id");
    let (code, v) = bp(root, &["set", "projects/bp/issues/0001.A", "severity", "low"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some(renamed)), "an old id, in another case: {v}");
    assert!(read(root, &format!("{renamed}.md")).contains("severity: low"));
    assert!(!root.join(format!("{A}.md")).exists());
    let (code, v) = bp(root, &["rename", renamed, "0001.a"]);
    assert_eq!((code, v["id"].as_str()), (Some(0), Some(A)), "a past slug of the node itself: {v}");
    assert!(read(root, &format!("{A}.md")).contains("aliases: [\"0001.renamed\"]\n"), "the slug taken back leaves");
    assert_eq!(bp(root, &["show", renamed]).1["id"], A);
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn a_rename_whose_commit_fails_leaves_no_new_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = vault();
    let root = dir.path();
    let hook = root.join(".git/hooks/pre-commit");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (code, v) = bp(root, &["rename", FRONT, "door"]);
    assert_eq!((code, v["code"].as_str()), (Some(1), Some("could_not_run")), "{v}");
    assert!(!root.join(NEW).exists(), "the undo takes the directories the rename made");
    assert!(root.join(format!("{FRONT}/diagrams/d.html")).exists());
    assert_eq!((commits(root), git(root, &["status", "--porcelain"])), ("1".into(), String::new()));
}
