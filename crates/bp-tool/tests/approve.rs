//! `bp approve` and `bp lower` through the binary: a vault, a bare origin and its clone, a `gh` that answers `pr list`
//! from a file, and an `ssh-add` that lists the keys of a file. The keys are made here; no test reads `~/.ssh` or
//! reaches an ssh-agent, as `SSH_AUTH_SOCK` is removed.

mod common;

use std::path::Path;

use common::git;
use serde_json::{Value, json};

const ISSUE: &str = "projects/bp/issues/a";

const HEAD: &str = "abcdefabcdefabcdefabcdefabcdefabcdefabcd";

/// The vault, and a directory holding `origin.git`, its clone `code`, the fakes in `bin/` and the keys `human` and
/// `agent`, each with no passphrase.
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let t = tempfile::tempdir().unwrap();
    let origin = t.path().join("origin.git");
    git(t.path(), &["init", "-q", "--bare", "-b", "master", "origin.git"]);
    git(t.path(), &["init", "-q", "-b", "master", "code"]);
    let code = t.path().join("code");
    git(&code, &["config", "user.name", "t"]);
    git(&code, &["config", "user.email", "t@t"]);
    std::fs::write(code.join("README.md"), "# bp\n").unwrap();
    git(&code, &["add", "README.md"]);
    git(&code, &["commit", "-qm", "one"]);
    git(&code, &["remote", "add", "origin", &origin.display().to_string()]);
    git(&code, &["push", "-q", "-u", "origin", "master"]);
    git(&code, &["remote", "set-head", "origin", "master"]);
    let bin = t.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let t_ = t.path().display();
    let gh = format!("#!/bin/sh\n[ \"$1 $2\" = 'pr list' ] || exit 9\ncat '{t_}/prs.json' 2>/dev/null || echo '[]'\n");
    let ssh_add = format!("#!/bin/sh\n[ \"$1\" = -L ] && [ -s '{t_}/agent-keys' ] || exit 1\ncat '{t_}/agent-keys'\n");
    for (name, text) in [("gh", gh), ("ssh-add", ssh_add)] {
        std::fs::write(bin.join(name), text).unwrap();
        std::fs::set_permissions(bin.join(name), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    for key in ["human", "agent"] {
        let out = std::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", key, "-f"])
            .arg(t.path().join(key))
            .output()
            .unwrap();
        assert!(out.status.success());
    }
    let project = format!("---\nkind: project\ntitle: Bp\nstatus: active\nrepository: {}\n---\n", origin.display());
    let v = common::vault(&[
        ("shared/.gitkeep", ""),
        ("projects/bp/project.md", &project),
        (&format!("{ISSUE}.md"), "---\nkind: issue\ntitle: A\nstatus: in-progress\n---\n\nBody.\n"),
    ]);
    (v, t)
}

/// `bp <args> --json` from the clone, with the vault named, the fakes first on `PATH` and no ssh-agent.
fn bp(v: &Path, t: &Path, env: &[(&str, &str)], args: &[&str]) -> (Option<i32>, Value) {
    let path = format!("{}:{}", t.join("bin").display(), std::env::var("PATH").unwrap());
    let vault = v.display().to_string();
    let base = [("BP_VAULT", vault.as_str()), ("PATH", path.as_str()), ("SSH_AUTH_SOCK", "")];
    common::bp_env(&t.join("code"), &base.into_iter().chain(env.iter().copied()).collect::<Vec<_>>(), args)
}

/// The `.pub` line of a key.
fn public(t: &Path, name: &str) -> String {
    std::fs::read_to_string(t.join(format!("{name}.pub"))).unwrap()
}

/// Commits `.bp/risk.yml` naming `name`'s key as the signer in the clone, and pushes it when `push`.
fn risk(t: &Path, name: &str, push: bool) {
    let code = t.join("code");
    std::fs::create_dir_all(code.join(".bp")).unwrap();
    std::fs::write(code.join(".bp/risk.yml"), format!("high:\n  - .bp/\nsigner: {}", public(t, name))).unwrap();
    git(&code, &["add", ".bp/risk.yml"]);
    git(&code, &["commit", "-qm", "risk"]);
    if push {
        git(&code, &["push", "-q", "origin", "master"]);
    }
}

#[test]
fn approve_and_lower_sign_the_head_with_the_signers_key() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let human = t.join("human").display().to_string();
    let agent = t.join("agent").display().to_string();
    let approve = |env: &[(&str, &str)], k: &str| bp(v, t, env, &["approve", ISSUE, "--key", k]);

    let (code, out) = approve(&[], &human);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("approval_missing")), "no pull request: {out}");
    let pr =
        json!([{ "number": 4, "state": "OPEN", "headRefName": "bp/a", "headRefOid": HEAD, "baseRefName": "master" }]);
    std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();
    let (code, out) = approve(&[], &human);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("signer_missing")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains("signer: "), "the fix names the line: {out}");

    // The agent names its own key in the clone, committed but not on origin: no signer.
    risk(t, "agent", false);
    assert_eq!(
        approve(&[("BP_ROLE", "human")], &agent).1["code"],
        "signer_missing",
        "the clone's file is not origin's"
    );
    git(&t.join("code"), &["reset", "-q", "--hard", "origin/master"]);
    risk(t, "human", true);

    std::fs::write(t.join("agent-keys"), public(t, "human")).unwrap();
    let (code, out) = approve(&[], &human);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("key_in_agent")), "{out}");
    std::fs::remove_file(t.join("agent-keys")).unwrap();

    // The label `human` is no key: the agent's own key signs a signature that does not verify.
    let (code, out) = approve(&[("BP_ROLE", "human")], &agent);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("signature_invalid")), "{out}");
    let (code, out) = approve(&[("BP_ROLE", "worker")], &human);
    assert_eq!((code, out["code"].as_str()), (Some(2), Some("role_forbidden")), "{out}");

    let before = common::commits(v);
    let (code, out) = approve(&[], &human);
    assert_eq!((code, out["id"].as_str()), (Some(0), Some(format!("{ISSUE}/approvals/{HEAD}").as_str())), "{out}");
    assert_eq!((out["head"].as_str(), &out["pull_request"]), (Some(HEAD), &json!(4)));
    let text = std::fs::read_to_string(v.join(format!("{ISSUE}/approvals/{HEAD}.md"))).unwrap();
    assert!(text.contains("kind: approval") && text.contains("-----BEGIN SSH SIGNATURE-----"), "{text}");
    let message = git(v, &["log", "-1", "--format=%B"]);
    assert!(message.contains("Bp-Verb: approve") && message.contains("Bp-Role: human"), "{message}");
    assert_eq!(common::commits(v).parse::<u32>().unwrap(), before.parse::<u32>().unwrap() + 1);
    assert_eq!(approve(&[], &human).0, Some(3), "a valid approval at this head is a repeat");

    let lower = |level: &str, k: &str| bp(v, t, &[], &["lower", ISSUE, level, "--reason", "docs only", "--key", k]);
    assert_eq!(lower("high", &human).0, Some(4), "a lowering names low or medium");
    assert_eq!(lower("low", &agent).1["code"], "signature_invalid");
    let (code, out) = lower("low", &human);
    assert_eq!((code, out["id"].as_str()), (Some(0), Some(format!("{ISSUE}/lowerings/{HEAD}").as_str())), "{out}");
    let text = std::fs::read_to_string(v.join(format!("{ISSUE}/lowerings/{HEAD}.md"))).unwrap();
    assert!(text.contains("level: low") && text.contains("reason: docs only"), "{text}");

    let (code, out) = common::bp(v, &["record", "approval", ISSUE, "--field", format!("head={HEAD}").as_str()]);
    assert_eq!(code, Some(4), "record writes no approval: {out}");
    assert!(out["fix"].as_str().unwrap().contains("bp approve"), "{out}");
}

#[test]
fn approve_refuses_a_head_that_is_not_a_full_sha() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    risk(t, "human", true);
    let human = t.join("human").display().to_string();
    for head in ["../../../x", "abcdef\nact: lower", "ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD", "abcdef1"] {
        let pr = json!([{ "number": 4, "state": "OPEN", "headRefName": "bp/a", "headRefOid": head }]);
        std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();
        let (code, out) = bp(v, t, &[], &["approve", ISSUE, "--key", &human]);
        assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{head:?}: {out}");
        assert!(!v.join(ISSUE).join("approvals").exists(), "{head:?}: no record is written");
    }
}
