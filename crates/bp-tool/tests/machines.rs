//! `bp transition` through the binary, every pair of statuses for every kind: a pair in the table of 25 (design
//! change 3) commits one changed line, the same status exits 3, and any other pair refuses `illegal_transition`
//! with the head unmoved. The table here is the design's, written out, so the domain crates' copy is checked
//! against it rather than against itself.

use std::path::Path;
use std::process::Command as Std;

use assert_cmd::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Std::new("git").arg("-C").arg(root).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn bp(root: &Path, args: &[&str]) -> (Option<i32>, serde_json::Value) {
    let out = Command::cargo_bin("bp")
        .unwrap()
        .current_dir(root)
        .env_remove("BP_VAULT")
        .env_remove("BP_ROLE")
        .env("BP_NOW", "2026-10-19T10:00:00Z")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    (out.status.code(), serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null))
}

fn texts<T: Copy>(all: &[T], text: fn(T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(|v| text(*v)).collect()
}

/// One kind: its name, its statuses from the domain crate, the moves of 25's table, and where its nodes live.
type Machine = (&'static str, Vec<&'static str>, Vec<(&'static str, &'static str)>, fn(usize) -> String);

fn kinds() -> Vec<Machine> {
    use bp_change::{IssueStatus as I, MilestoneStatus as M};
    use bp_design::{DesignStatus as D, ProjectStatus as P};
    use bp_reasoning::ThreadStatus as T;
    use bp_reasoning::{ConventionStatus as C, DecisionStatus as De, InboxStatus as X, LearningStatus as L};
    vec![
        (
            "milestone",
            texts(M::ALL, M::as_str),
            vec![
                ("planned", "approved"),
                ("planned", "abandoned"),
                ("approved", "abandoned"),
                ("abandoned", "planned"),
            ],
            |n| format!("projects/bp/milestones/m{n}"),
        ),
        ("issue", texts(I::ALL, I::as_str), vec![("open", "abandoned"), ("abandoned", "open")], |n| {
            format!("projects/bp/issues/i{n}")
        }),
        (
            "design",
            texts(D::ALL, D::as_str),
            vec![("draft", "active"), ("active", "deprecated"), ("active", "retired"), ("deprecated", "retired")],
            |n| format!("projects/bp/designs/components/d{n}"),
        ),
        (
            "thread",
            texts(T::ALL, T::as_str),
            vec![("open", "paused"), ("paused", "open"), ("open", "closed"), ("paused", "closed")],
            |n| format!("projects/bp/threads/t{n}"),
        ),
        (
            "decision",
            texts(De::ALL, De::as_str),
            vec![("proposed", "accepted"), ("proposed", "rejected"), ("accepted", "superseded")],
            |n| format!("projects/bp/decisions/d{n}"),
        ),
        (
            "learning",
            texts(L::ALL, L::as_str),
            vec![("draft", "verified"), ("draft", "retracted"), ("verified", "retracted")],
            |n| format!("shared/learnings/l{n}"),
        ),
        ("inbox", texts(X::ALL, X::as_str), vec![("raw", "promoted"), ("raw", "dropped")], |n| {
            format!("shared/inbox/x{n}")
        }),
        ("project", texts(P::ALL, P::as_str), vec![("active", "retired")], |n| format!("projects/p{n}/project")),
        ("convention", texts(C::ALL, C::as_str), vec![("active", "retired")], |n| format!("shared/conventions/c{n}")),
    ]
}

#[test]
fn every_pair_of_every_machine() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut pairs = Vec::new();
    for (kind, statuses, moves, path) in kinds() {
        for from in &statuses {
            for to in &statuses {
                let id = path(pairs.len());
                let file = root.join(format!("{id}.md"));
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, format!("---\nkind: {kind}\ntitle: T\nstatus: {from}\n---\n\nBody.\n")).unwrap();
                pairs.push((kind, id, *from, *to, moves.contains(&(*from, *to))));
            }
        }
    }
    std::fs::create_dir_all(root.join("shared")).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@t"]);
    std::fs::write(root.join(".gitignore"), ".bp/\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "one"]);
    let mut commits = 1;
    for (kind, id, from, to, legal) in pairs {
        let (exit, v) = bp(root, &["transition", &id, to]);
        let head = git(root, &["rev-list", "--count", "HEAD"]).trim().parse::<usize>().unwrap();
        let case = format!("{kind} {from} -> {to}: {v}");
        if from == to {
            assert_eq!((exit, v["id"].as_str()), (Some(3), Some(id.as_str())), "{case}");
        } else if legal {
            commits += 1;
            assert_eq!(exit, Some(0), "{case}");
            assert_eq!(v["commit"].as_str().unwrap(), git(root, &["rev-parse", "HEAD"]).trim(), "{case}");
            let lines = if kind == "decision" && matches!(to, "accepted" | "rejected") { 2 } else { 1 };
            let numstat = format!("{lines}\t1\t{id}.md\n");
            assert_eq!(git(root, &["diff", "--numstat", "HEAD~1", "HEAD"]), numstat, "{case}");
            assert!(git(root, &["show", &format!("HEAD:{id}.md")]).contains(&format!("\nstatus: {to}\n")), "{case}");
            assert!(git(root, &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Verb: transition"), "{case}");
        } else {
            assert_eq!((exit, v["code"].as_str()), (Some(2), Some("illegal_transition")), "{case}");
        }
        assert_eq!(head, commits, "{case}");
    }
    assert_eq!(git(root, &["status", "--porcelain"]), "");
}

#[test]
fn the_fix_names_the_owning_verb_and_the_reason_is_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for (id, text) in [
        ("projects/bp/issues/i", "---\nkind: issue\ntitle: I\nstatus: open\n---\n"),
        ("projects/bp/issues/w", "---\nkind: issue\ntitle: W\nstatus: in-progress\n---\n"),
        ("projects/bp/milestones/m", "---\nkind: milestone\ntitle: M\nstatus: approved\n---\n"),
        ("projects/bp/decisions/d", "---\nkind: decision\ntitle: D\nstatus: proposed\n---\n"),
        ("projects/bp/issues/i/claims/c", "---\nkind: claim\nunder: projects/bp/issues/i\n---\n"),
    ] {
        let file = root.join(format!("{id}.md"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    std::fs::create_dir_all(root.join("shared")).unwrap();
    std::fs::write(root.join(".gitignore"), ".bp/\n").unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "user.email", "t@t"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "one"]);
    for (id, to, verb) in [
        ("projects/bp/issues/i", "resolved", "bp land "),
        ("projects/bp/issues/i", "in-progress", "bp cut "),
        ("projects/bp/issues/w", "open", "bp release "),
        ("projects/bp/milestones/m", "done", "bp stamp "),
        ("projects/bp/issues/i/claims/c", "open", "a record is never edited"),
    ] {
        let (exit, v) = bp(root, &["transition", id, to]);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("illegal_transition")), "{id} {to}");
        assert!(v["fix"].as_str().unwrap().contains(verb), "{id} {to}: {v}");
    }
    let (exit, v) = bp(root, &["transition", "projects/bp/issues/i", "closed"]);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("unknown_value")), "{v}");
    let (exit, v) = bp(root, &["transition", "projects/bp/issues/i", "abandoned", "--dry-run"]);
    assert_eq!((exit, v["dry_run"].as_bool()), (Some(0), Some(true)), "{v}");
    assert_eq!(git(root, &["rev-list", "--count", "HEAD"]).trim(), "1");
    let (exit, _) = bp(root, &["transition", "projects/bp/issues/i", "abandoned", "--reason", "out of scope"]);
    assert_eq!(exit, Some(0));
    assert_eq!(
        git(root, &["log", "-1", "--format=%B"]),
        "transition projects/bp/issues/i abandoned\n\nout of scope\n\nBp-Verb: transition\nBp-Role: human\n\n"
    );
    let (exit, _) = bp(root, &["transition", "projects/bp/decisions/d", "accepted"]);
    assert_eq!(exit, Some(0));
    let (_, v) = bp(root, &["show", "projects/bp/decisions/d"]);
    assert_eq!((v["status"].as_str(), v["decided_at"].as_str()), (Some("accepted"), Some("2026-10-19T10:00:00Z")));
}
