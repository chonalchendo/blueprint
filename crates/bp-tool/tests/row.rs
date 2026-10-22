//! The tool's role check (21, rule 6): a verb outside the row of `BP_ROLE` refuses `verb_outside_role`, whichever
//! verb it is, before it reads or writes anything. The ids name nothing, so a verb that passed would show another code.

mod common;

use bp_operation::{READ_VERBS, Role};
use common::{bp, bp_env, commits};

fn vault() -> tempfile::TempDir {
    common::vault(&[("projects/.keep", "")])
}

/// Every write verb with arguments that parse, so only the role check can refuse it first.
const WRITES: [&[&str]; 11] = [
    &["set", "projects/bp/issues/none", "severity", "low"],
    &["create", "inbox", "--title", "Q"],
    &["link", "projects/bp/issues/none", "depends_on", "projects/bp/issues/other"],
    &["transition", "projects/bp/issues/none", "resolved"],
    &["record", "handoff", "projects/bp/project"],
    &["rename", "projects/bp/issues/none", "new"],
    &["cut", "projects/bp/issues/none"],
    &["release", "projects/bp/issues/none", "--reason", "dropped"],
    &["pr", "projects/bp/issues/none"],
    &["check", "projects/bp/issues/none"],
    &["install", "--target", "claude"],
];

#[test]
fn every_verb_outside_the_row_refuses_and_commits_nothing() {
    let dir = vault();
    let root = dir.path();
    let before = commits(root);
    for role in Role::ALL.iter().filter(|r| **r != Role::Human) {
        let row = role.verbs().unwrap();
        for args in WRITES {
            if row.contains(&args[0]) {
                continue;
            }
            let (code, v) = bp_env(root, &[("BP_ROLE", role.as_str())], args);
            assert_eq!((code, v["code"].as_str()), (Some(2), Some("verb_outside_role")), "{role} {args:?}: {v}");
            assert_eq!(v["subject"], format!("{} under BP_ROLE={role}", args[0]));
            assert!(v["fix"].as_str().unwrap().contains(&format!("`bp {}`", args[0])), "{v}");
        }
    }
    assert_eq!(commits(root), before, "a refusal commits nothing");
}

#[test]
fn a_read_verb_a_row_verb_and_the_human_pass_the_check() {
    let dir = vault();
    let root = dir.path();
    for role in ["session", "worker", "reviewer", "researcher"] {
        for read in READ_VERBS {
            let (_, v) = bp_env(root, &[("BP_ROLE", role)], &[read, "projects/bp/issues/none"]);
            assert_ne!(v["code"], "verb_outside_role", "{role} {read}: {v}");
        }
    }
    let (_, v) = bp_env(root, &[("BP_ROLE", "worker")], &["record", "review", "projects/bp/issues/none"]);
    assert_ne!(v["code"], "verb_outside_role", "record is in the worker's row: {v}");
    let (code, v) = bp(root, &["create", "inbox", "--title", "Q"]);
    assert_ne!(v["code"], "verb_outside_role", "the human has no row: {v} {code:?}");
}

#[test]
fn the_humans_acts_keep_role_forbidden() {
    let dir = vault();
    let (_, v) = bp_env(dir.path(), &[("BP_ROLE", "worker")], &["stamp", "projects/bp/milestones/none"]);
    assert_eq!(v["code"], "role_forbidden", "a code keeps its meaning (15): {v}");
}
