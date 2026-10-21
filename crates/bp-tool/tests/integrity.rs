//! The `integrity` check through the binary (25, issue 11): a clean project passes; each rule fails a node of the
//! owner's project and none outside it; the run names the vault's head and the failing nodes; `blocked` holds for a
//! named open issue and milestone, each by its own project's run; a dirty project refuses; and rule 3 agrees with the
//! walk.

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
        (
            "projects/bp/designs/product.md",
            "---\nkind: design\ntitle: P\nstatus: active\nunder: projects/bp/project\n---\n\nThe product.\n",
        ),
        (
            "projects/bp/designs/system.md",
            "---\nkind: design\ntitle: S\nstatus: active\nunder: projects/bp/designs/product\n---\n\nThe system.\n",
        ),
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
    // A twin by case: git holds both paths on any file system; the default macOS one holds only one on disk. The
    // twin is written too, so the tree is clean where the disk holds both.
    let blob = git(vault, &["hash-object", "-w", "projects/bp/issues/ok.md"]);
    git(vault, &["update-index", "--add", "--cacheinfo", &format!("100644,{},projects/bp/issues/OK.md", blob.trim())]);
    let both_on_disk = !vault.join("projects/bp/issues/OK.md").exists();
    if both_on_disk {
        std::fs::copy(vault.join("projects/bp/issues/ok.md"), vault.join("projects/bp/issues/OK.md")).unwrap();
    }
    git(vault, &["commit", "-qm", "faults"]);
    let at = head(vault);

    let (exit, v) = check(vault, code);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("check_failed")), "{v}");
    let id = format!("{M}/runs/integrity-{at}-1");
    assert_eq!((v["runs"][0]["verdict"].as_str(), v["runs"][0]["id"].as_str()), (Some("fail"), Some(id.as_str())));
    let (_, run) = bp(vault, &["show", &id]);
    assert_eq!(run["commit"], at, "integrity is vault state: {run}");
    let names: Vec<&str> = run["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    // Where the disk holds both twins, both are nodes and both are named.
    let mut want = vec!["projects/bp/designs/containers/c", "projects/bp/issues/bare", "projects/bp/issues/links"];
    if both_on_disk {
        want.insert(1, "projects/bp/issues/OK");
    }
    want.push("projects/bp/issues/ok");
    assert_eq!(names, want, "sorted, once each; a broken file and a path no file holds are no edge");
    let text = std::fs::read_to_string(vault.join(format!("{id}.md"))).unwrap();
    for line in [
        "broken: projects/bp/issues/bad.md: ",
        "link: projects/bp/issues/links: body projects/bp/issues/nope: resolves to nothing",
        "link: projects/bp/issues/links: markdown /projects/bp/designs/system/diagrams/gone.html: resolves to nothing",
        "issue: projects/bp/issues/bare: no milestone under `under`",
        "issue: projects/bp/issues/bare: no design under `cites`",
        "level: projects/bp/designs/containers/c: `under` projects/bp/project is a project; a container design sits under a system design of its project",
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
    let note = format!("integrity: {id}; rerun bp check {M} --name integrity");
    assert_eq!(intent("projects/bp/issues/bare"), ("blocked".into(), note));
    assert_eq!(intent("projects/bp/issues/fine"), ("ready".into(), String::new()), "not named, not blocked");
    let at = text.find("\nbroken: ").unwrap();
    assert!(text[at..].lines().skip(1).all(|l| !l.starts_with("link: ")), "faults with no node come last: {text}");
}

#[test]
fn each_project_is_blocked_only_by_its_own_run() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let put = |rel: &str, text: &str| {
        let path = vault.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    let ms = std::fs::read_to_string(vault.join(format!("{M}.md"))).unwrap();
    for (rel, text) in [
        ("project.md", "---\nkind: project\ntitle: q\nstatus: active\n---\n".to_string()),
        (
            "designs/product.md",
            "---\nkind: design\ntitle: P\nstatus: active\nunder: projects/q/project\n---\n\nThe product.\n".into(),
        ),
        (
            "designs/system.md",
            "---\nkind: design\ntitle: S\nstatus: active\nunder: projects/q/designs/product\n---\n\nThe system.\n"
                .into(),
        ),
        ("milestones/m.md", ms.replace("projects/bp/", "projects/q/")),
    ] {
        put(&format!("projects/q/{rel}"), &text);
    }
    // Each project's run names its own bare issue only.
    for p in ["bp", "q"] {
        put(&format!("projects/{p}/issues/bare.md"), &issue("", "No context."));
    }
    git(vault, &["add", "-A"]);
    git(vault, &["commit", "-qm", "two projects"]);
    let mut notes = Vec::new();
    for owner in [M, "projects/q/milestones/m"] {
        let (exit, v) = bp_env(code, &[("BP_VAULT", vault.to_str().unwrap())], &["check", owner]);
        assert_eq!(exit, Some(2), "{v}");
        let run = v["runs"][0]["id"].as_str().unwrap();
        notes.push(format!("integrity: {run}; rerun bp check {owner} --name integrity"));
    }
    let (_, v) = bp(vault, &["list", "issue"]);
    let intent = |id: &str| {
        let row = v["rows"].as_array().unwrap().iter().find(|r| r["id"] == id).unwrap();
        (row["intent"].as_str().unwrap_or("-").to_string(), row["note"].as_str().unwrap_or("-").to_string())
    };
    for (p, note) in ["bp", "q"].iter().zip(notes) {
        assert_eq!(intent(&format!("projects/{p}/issues/bare")), ("blocked".into(), note), "{p}: its own run");
    }
}

#[test]
fn a_dirty_project_refuses_before_it_runs() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let before = commits(vault);
    std::fs::write(vault.join("projects/bp/issues/new.md"), issue("", "Untracked.")).unwrap();
    let (exit, v) = check(vault, code);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("path_dirty")), "{v}");
    assert_eq!(commits(vault), before, "no run recorded");
    std::fs::remove_file(vault.join("projects/bp/issues/new.md")).unwrap();
    std::fs::write(vault.join("shared/threads/t.md"), "---\nkind: thread\ntitle: T2\nstatus: open\n---\n").unwrap();
    assert_eq!(check(vault, code).0, Some(0), "another project's or a shared change is not this run's");
}

#[test]
fn rule_3_and_the_walk_agree_on_aliases_and_empty_designs() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let put = |rel: &str, text: &str| std::fs::write(vault.join(rel), text).unwrap();
    let ms = std::fs::read_to_string(vault.join(format!("{M}.md"))).unwrap();
    put(&format!("{M}.md"), &ms.replace("status: active\n", "status: active\naliases: [old-m]\n"));
    let sys = std::fs::read_to_string(vault.join("projects/bp/designs/system.md")).unwrap();
    put("projects/bp/designs/system.md", &sys.replace("status: active\n", "status: active\naliases: [old-s]\n"));
    put(
        "projects/bp/designs/data-model.md",
        "---\nkind: design\ntitle: D\nstatus: active\nunder: projects/bp/designs/system\n---\n",
    );
    put(
        "projects/bp/issues/alias-cite.md",
        &issue("under: projects/bp/milestones/m\ncites: [projects/bp/designs/old-s]\n", "A."),
    );
    put(
        "projects/bp/issues/alias-under.md",
        &issue("under: projects/bp/milestones/old-m\ncites: [projects/bp/designs/system]\n", "B."),
    );
    put(
        "projects/bp/issues/empty-cite.md",
        &issue("under: projects/bp/milestones/m\ncites: [projects/bp/designs/data-model]\n", "C."),
    );
    git(vault, &["add", "-A"]);
    git(vault, &["commit", "-qm", "context"]);
    let (exit, v) = check(vault, code);
    assert_eq!(exit, Some(2), "{v}");
    let (_, run) = bp(vault, &["show", v["runs"][0]["id"].as_str().unwrap()]);
    let names: Vec<&str> = run["names"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect();
    for i in ["alias-cite", "alias-under", "empty-cite"] {
        let id = format!("projects/bp/issues/{i}");
        let (exit, w) = bp(vault, &["walk", &id, "--for", "complete", "--ids"]);
        let refused = w["code"].as_str() == Some("context_incomplete");
        assert_eq!(refused, names.contains(&id.as_str()), "{i}: the walk and rule 3 agree: {w} {names:?}");
        assert_eq!((i == "empty-cite", exit == Some(0)), (refused, !refused), "{i}: {w}");
    }
}

#[test]
fn only_a_passing_or_failing_error_run_counts_and_a_named_milestone_is_blocked() {
    let (vault, code) = setup();
    let (vault, code) = (vault.path(), code.path());
    let put = |rel: &str, text: &str| std::fs::write(vault.join(rel), text).unwrap();
    put(
        "projects/bp/milestones/n.md",
        "---\nkind: milestone\ntitle: N\nstatus: planned\nunder: projects/bp/designs/system\n---\n\n[[projects/bp/issues/gone]]\n",
    );
    let ms = std::fs::read_to_string(vault.join(format!("{M}.md"))).unwrap();
    let extra = "~~~check integrity {name=w severity=warn}\n~~~\n~~~check integrity {name=c when=pr}\n~~~\n";
    put(&format!("{M}.md"), &format!("{ms}{extra}"));
    git(vault, &["add", "-A"]);
    git(vault, &["commit", "-qm", "named"]);
    // Each run a minute after the last: two runs in one second at two commits tie, and the id decides.
    let minute = std::cell::Cell::new(10);
    let only = |name: &str| {
        minute.set(minute.get() + 1);
        let now = format!("2026-10-20T10:{:02}:00Z", minute.get());
        bp_env(code, &[("BP_VAULT", vault.to_str().unwrap()), ("BP_NOW", &now)], &["check", M, "--name", name])
    };
    assert_eq!(only("integrity").0, Some(2));
    let blocked = |id: &str| {
        let (_, v) = bp(vault, &["list", "milestone"]);
        let row = v["rows"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().unwrap();
        row["intent"].as_str().map(str::to_string)
    };
    assert_eq!(blocked("projects/bp/milestones/n").as_deref(), Some("blocked"));
    // An approved milestone reads the run in its own arm, after its derivation records.
    let planned = std::fs::read_to_string(vault.join("projects/bp/milestones/n.md")).unwrap();
    put("projects/bp/milestones/n.md", &planned.replace("status: planned", "status: approved"));
    git(vault, &["commit", "-qam", "approved"]);
    assert_eq!(blocked("projects/bp/milestones/n").as_deref(), Some("blocked"), "approved and named");
    put(
        "projects/bp/milestones/n.md",
        "---\nkind: milestone\ntitle: N\nstatus: planned\nunder: projects/bp/designs/system\n---\n\nFixed.\n",
    );
    git(vault, &["commit", "-qam", "fixed"]);
    let (_, v) = only("c");
    assert_eq!(v["runs"][0]["verdict"], "could-not-run", "{v}");
    let (_, v) = only("w");
    assert_eq!(v["runs"][0]["verdict"], "pass", "{v}");
    assert_eq!(blocked("projects/bp/milestones/n").as_deref(), Some("blocked"), "neither run counts");
    assert_eq!(only("integrity").0, Some(0));
    assert_eq!(blocked("projects/bp/milestones/n").as_deref(), Some("needs-milestone-approval"), "a pass clears");
}
