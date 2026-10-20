//! The `integrity` check through the binary (25, issue 11): a clean project passes; each rule fails a node of the
//! owner's project and none outside it; the run names the vault's head and the failing nodes; and `blocked` holds
//! for a named open issue.

use std::path::Path;

mod common;

use common::{bp, bp_env, commits, git};

const M: &str = "projects/bp/milestones/m";

fn issue(extra: &str, body: &str) -> String {
    format!("---\nkind: issue\ntitle: T\nstatus: open\n{extra}---\n\n{body}\n")
}

const CONTEXT: &str = "under: projects/bp/milestones/m\ncites: [projects/bp/designs/system]\n";

/// A project that keeps every rule, the owner `M` with an integrity block, and a code repository to run from.
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let milestone = "---\nkind: milestone\ntitle: M\nstatus: active\nunder: projects/bp/designs/system\n---\n\n\
                     ## Finish line\n\n~~~check integrity {name=integrity}\n~~~\n";
    let vault = common::vault(&[
        ("projects/bp/project.md", "---\nkind: project\ntitle: bp\nstatus: active\n---\n"),
        ("projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n\nThe system.\n"),
        ("projects/bp/designs/system/diagrams/d.html", "<p/>\n"),
        (
            "projects/bp/designs/containers/fine.md",
            "---\nkind: design\ntitle: C\nstatus: active\nunder: projects/bp/designs/system\n---\n\n\
             [d](/projects/bp/designs/system/diagrams/d.html)\n",
        ),
        (&format!("{M}.md"), milestone),
        ("projects/bp/issues/ok.md", &issue(CONTEXT, "[[projects/bp/designs/system]]")),
        ("projects/bp/issues/fine.md", &issue(CONTEXT, "Fine.")),
        // Out of every project's scope: a shared node's broken link fails no project.
        ("shared/threads/t.md", "---\nkind: thread\ntitle: T\nstatus: open\ncites: [shared/nowhere/x]\n---\n"),
    ]);
    (vault, common::vault(&[("marker.txt", "here\n")]))
}

fn check(vault: &Path, code: &Path) -> (Option<i32>, serde_json::Value) {
    bp_env(code, &[("BP_VAULT", vault.to_str().unwrap())], &["check", M])
}

fn head(root: &Path) -> String {
    git(root, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn a_clean_project_passes_at_the_vault_head() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let at = head(vault);
    let before: u32 = commits(vault).parse().unwrap();
    let (exit, v) = check(vault, code);
    assert_eq!(exit, Some(0), "{v}");
    assert_eq!(commits(vault).parse::<u32>().unwrap(), before + 1, "one commit: the run record");
    let id = format!("{M}/runs/integrity-{at}-1");
    assert_eq!((v["runs"][0]["verdict"].as_str(), v["runs"][0]["id"].as_str()), (Some("pass"), Some(id.as_str())));
    let (_, run) = bp(vault, &["show", &id]);
    assert_eq!((run["check_kind"].as_str(), run["commit"].as_str()), (Some("integrity"), Some(at.as_str())), "{run}");
    assert!(run.get("names").is_none(), "{run}");
}

#[test]
fn each_rule_fails_its_node_and_a_named_open_issue_is_blocked() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let put = |rel: &str, text: &str| {
        let path = vault.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    put("projects/bp/issues/bad.md", "---\nkind: [\n---\n");
    let body = "[[projects/bp/issues/nope]] and [gone](/projects/bp/designs/system/diagrams/gone.html)";
    put("projects/bp/issues/links.md", &issue(CONTEXT, body));
    put("projects/bp/issues/bare.md", &issue("", "No context."));
    put(
        "projects/bp/designs/containers/c.md",
        "---\nkind: design\ntitle: C\nstatus: active\nunder: projects/bp/project\n---\n\nBody.\n",
    );
    git(vault, &["add", "-A"]);
    // A twin by case: git holds both paths on any file system; the default macOS one holds only one on disk.
    let blob = git(vault, &["hash-object", "-w", "projects/bp/issues/ok.md"]);
    git(vault, &["update-index", "--add", "--cacheinfo", &format!("100644,{},projects/bp/issues/OK.md", blob.trim())]);
    git(vault, &["commit", "-qm", "faults"]);
    let at = head(vault);

    let (exit, v) = check(vault, code);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("check_failed")), "{v}");
    let id = format!("{M}/runs/integrity-{at}-1");
    assert_eq!((v["runs"][0]["verdict"].as_str(), v["runs"][0]["id"].as_str()), (Some("fail"), Some(id.as_str())));
    let (_, run) = bp(vault, &["show", &id]);
    assert_eq!(run["commit"], at, "integrity is vault state: {run}");
    let names: Vec<&str> = run["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "projects/bp/designs/containers/c",
            "projects/bp/issues/bare",
            "projects/bp/issues/links",
            "projects/bp/issues/ok"
        ],
        "sorted, once each; a broken file and a path no file holds are no edge"
    );
    let text = std::fs::read_to_string(vault.join(format!("{id}.md"))).unwrap();
    for line in [
        "broken: projects/bp/issues/bad.md: ",
        "link: projects/bp/issues/links: body projects/bp/issues/nope: resolves to nothing",
        "link: projects/bp/issues/links: markdown /projects/bp/designs/system/diagrams/gone.html: resolves to nothing",
        "issue: projects/bp/issues/bare: no `under` milestone",
        "issue: projects/bp/issues/bare: cites no design that exists",
        "level: projects/bp/designs/containers/c: `under` projects/bp/project is no design; a container sits under a system design",
        "case: projects/bp/issues/OK: differs only by case from projects/bp/issues/ok.md",
        "case: projects/bp/issues/ok: differs only by case from projects/bp/issues/OK.md",
    ] {
        assert!(text.contains(&format!("\n{line}")), "{line}\n{text}");
    }
    assert!(!text.contains("shared/"), "a shared node is out of scope: {text}");

    let (exit, v) = bp(vault, &["list", "issue"]);
    assert_eq!(exit, Some(0), "{v}");
    let intent = |id: &str| {
        let row = v["rows"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap();
        (row["intent"].as_str().unwrap_or("-").to_string(), row["note"].as_str().unwrap_or("-").to_string())
    };
    assert_eq!(intent("projects/bp/issues/bare"), ("blocked".into(), format!("integrity: {id}")));
    assert_eq!(intent("projects/bp/issues/fine"), ("ready".into(), String::new()), "not named, not blocked");
}
