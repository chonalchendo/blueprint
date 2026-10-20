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
            "---\nkind: issue\ntitle: A\nstatus: open\ncites: [\"projects/bp/designs/containers/front\"]\n---\n\n\
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
    let want = "---\nkind: issue\ntitle: A\nstatus: open\ncites: [\"projects/bp/designs/containers/door\"]\n---\n\n\
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
