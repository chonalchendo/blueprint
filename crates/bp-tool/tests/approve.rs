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
    // `gh pr comment <n> --body <text>` adds the number and the text, so two posts show twice; it fails while the file
    // `fail-post` exists. `gh api --paginate <path>` prints `comments.json`, the forge's comments, page by page.
    let gh = format!(
        "#!/bin/sh\n[ \"$1 $2 $4\" = 'pr comment --body' ] && [ -e '{t_}/fail-post' ] && {{ echo 'error connecting' >&2; exit 1; }}\n\
         [ \"$1 $2 $4\" = 'pr comment --body' ] && {{ echo \"$3\" >> '{t_}/posted'; printf %s \"$5\" >> '{t_}/posted'; exit 0; }}\n\
         [ \"$1 $2 $3\" = 'api --paginate repos/{{owner}}/{{repo}}/issues/4/comments' ] && {{ cat '{t_}/comments.json' 2>/dev/null || echo '[]'; exit 0; }}\n\
         [ \"$1 $2\" = 'pr list' ] || exit 9\ncat '{t_}/prs.json' 2>/dev/null || echo '[]'\n"
    );
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
    assert!(text.contains("\nbase: master\n"), "the record keeps the signed base branch: {text}");
    let message = git(v, &["log", "-1", "--format=%B"]);
    assert!(message.contains("Bp-Verb: approve") && message.contains("Bp-Role: human"), "{message}");
    assert_eq!(common::commits(v).parse::<u32>().unwrap(), before.parse::<u32>().unwrap() + 1);
    // The comment for the forge check: the signed message's lines and the record's signature, in one block.
    let comment = out["comment"].as_str().unwrap();
    let block = format!(
        "```bp-approval\nact: approve\nproject: bp\nnode: {ISSUE}\npull_request: 4\nbase: master\nhead: {HEAD}\n"
    );
    assert!(comment.contains(&format!("{block}-----BEGIN SSH SIGNATURE-----\n")), "{comment}");
    let signature = &comment[comment.find("-----BEGIN").unwrap()..comment.rfind("\n```").unwrap()];
    assert!(signature.ends_with("-----END SSH SIGNATURE-----") && text.contains(signature), "{comment}");
    assert!(!t.join("posted").exists(), "no --post, no comment");
    assert_eq!(approve(&[], &human).0, Some(3), "a valid approval at this head is a repeat");
    let post = || bp(v, t, &[], &["approve", ISSUE, "--key", &human, "--post"]);
    // The human posted the comment by hand: the pull request holds the signature, so a repeat posts nothing.
    std::fs::write(t.join("comments.json"), json!([{ "body": comment }]).to_string()).unwrap();
    let (code, again) = post();
    assert_eq!((code, &again["comment"]), (Some(3), &out["comment"]), "a repeat gives the same comment: {again}");
    assert!(!t.join("posted").exists(), "the pull request holds this signature, so the repeat posts nothing");
    // The signature under another pull request's line is a block `bp gate` does not count: the repeat posts (row 53).
    let moved = comment.replace("pull_request: 4\n", "pull_request: 5\n");
    assert_ne!(moved, comment);
    std::fs::write(t.join("comments.json"), json!([{ "body": moved }]).to_string()).unwrap();
    assert_eq!(post().0, Some(3));
    assert_eq!(std::fs::read_to_string(t.join("posted")).unwrap(), format!("4\n{comment}"), "one post");
    std::fs::remove_file(t.join("posted")).unwrap();
    std::fs::remove_file(t.join("comments.json")).unwrap();

    // A retarget changes the signed base: the old approval is no repeat. Its post fails, and the record stays.
    let onto =
        json!([{ "number": 4, "state": "OPEN", "headRefName": "bp/a", "headRefOid": HEAD, "baseRefName": "bp/b" }]);
    std::fs::write(t.join("prs.json"), onto.to_string()).unwrap();
    std::fs::write(t.join("fail-post"), "").unwrap();
    let (code, out) = post();
    assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{out}");
    let fix = out["fix"].as_str().unwrap();
    assert!(fix.ends_with(&format!("the approval is recorded: rerun bp approve {ISSUE} --post to post it")), "{out}");
    assert!(!t.join("posted").exists(), "the post failed");
    // The rerun is a repeat, and the pull request lacks the signature: it posts once.
    std::fs::remove_file(t.join("fail-post")).unwrap();
    let (code, out) = post();
    assert_eq!(code, Some(3), "{out}");
    let comment = out["comment"].as_str().unwrap();
    assert!(comment.contains("pull_request: 4\nbase: bp/b\nhead: "), "{out}");
    let posted = format!("4\n{comment}");
    assert_eq!(std::fs::read_to_string(t.join("posted")).unwrap(), posted, "gh pr comment 4 --body <the comment>");
    // The forge now holds it, on its second page and with CRLF line ends: the next rerun posts nothing.
    let pages = format!("{}{}", json!([{ "body": "LGTM" }]), json!([{ "body": comment.replace('\n', "\r\n") }]));
    std::fs::write(t.join("comments.json"), pages).unwrap();
    assert_eq!(post().0, Some(3), "the post's repeat");
    assert_eq!(std::fs::read_to_string(t.join("posted")).unwrap(), posted, "one post, not two");
    std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();

    let lower = |level: &str, k: &str| bp(v, t, &[], &["lower", ISSUE, level, "--reason", "docs only", "--key", k]);
    assert_eq!(lower("high", &human).0, Some(4), "a lowering names low or medium");
    assert_eq!(lower("low", &agent).1["code"], "signature_invalid");
    let (code, out) = lower("low", &human);
    assert_eq!((code, out["id"].as_str()), (Some(0), Some(format!("{ISSUE}/lowerings/{HEAD}").as_str())), "{out}");
    assert!(out["comment"].is_null(), "a lowering has no forge comment: {out}");
    let text = std::fs::read_to_string(v.join(format!("{ISSUE}/lowerings/{HEAD}.md"))).unwrap();
    assert!(text.contains("level: low") && text.contains("reason: docs only"), "{text}");
    assert!(text.contains("\nbase: master\n"), "{text}");

    let (code, out) = common::bp(v, &["record", "approval", ISSUE, "--field", format!("head={HEAD}").as_str()]);
    assert_eq!(code, Some(4), "record writes no approval: {out}");
    assert!(out["fix"].as_str().unwrap().contains("bp approve"), "{out}");
}

#[test]
fn approve_reads_the_signer_from_the_forge_not_a_local_ref() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let code = t.join("code");
    let agent = t.join("agent").display().to_string();
    let pr =
        json!([{ "number": 4, "state": "OPEN", "headRefName": "bp/a", "headRefOid": HEAD, "baseRefName": "master" }]);
    std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();
    risk(t, "human", true);
    // The agent pushes its own branch, which names its key, as normal work.
    git(&code, &["checkout", "-q", "-b", "bp/x"]);
    risk(t, "agent", false);
    git(&code, &["push", "-q", "origin", "bp/x"]);
    git(&code, &["checkout", "-q", "master"]);
    let approve = || bp(v, t, &[("BP_ROLE", "human")], &["approve", ISSUE, "--key", &agent]);

    // Poison one: origin/HEAD names the agent's pushed branch, with no push.
    git(&code, &["remote", "set-head", "origin", "bp/x"]);
    assert!(git(&code, &["show", "origin/HEAD:.bp/risk.yml"]).contains(&public(t, "agent").trim().to_string()));
    let (code_, out) = approve();
    assert_eq!((code_, out["code"].as_str()), (Some(2), Some("signature_invalid")), "{out}");

    // Poison two: origin/master is a local commit that names the agent's key, and the fetch that `approve` runs
    // keeps it there.
    git(&code, &["remote", "set-head", "origin", "master"]);
    risk(t, "agent", false);
    git(&code, &["update-ref", "refs/remotes/origin/master", "HEAD"]);
    git(&code, &["config", "remote.origin.fetch", "+refs/heads/bp/x:refs/remotes/origin/master"]);
    let (code_, out) = approve();
    assert_eq!((code_, out["code"].as_str()), (Some(2), Some("signature_invalid")), "{out}");
    assert!(git(&code, &["show", "origin/master:.bp/risk.yml"]).contains(&public(t, "agent").trim().to_string()));

    // Poison three: a local `git replace` swaps origin's true default-branch commit for one whose tree names the
    // agent's key. ls-remote still names the true sha (review of #100, row S1).
    git(&code, &["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"]);
    let true_sha = git(&code, &["ls-remote", "origin", "refs/heads/master"]).split('\t').next().unwrap().to_string();
    git(&code, &["update-ref", "refs/remotes/origin/master", &true_sha]);
    let forged = git(&code, &["commit-tree", "HEAD^{tree}", "-m", "forged"]).trim().to_string();
    git(&code, &["replace", &true_sha, &forged]);
    assert!(git(&code, &["show", &format!("{true_sha}:.bp/risk.yml")]).contains(public(t, "agent").trim()));
    let (code_, out) = approve();
    assert_eq!((code_, out["code"].as_str()), (Some(2), Some("signature_invalid")), "{out}");
    assert!(!v.join(ISSUE).join("approvals").exists(), "no approval is written");
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
    // The base branch is a line of the signed message too.
    for base in [json!(null), json!(""), json!("a b"), json!("master\nhead: x")] {
        let pr =
            json!([{ "number": 4, "state": "OPEN", "headRefName": "bp/a", "headRefOid": HEAD, "baseRefName": base }]);
        std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();
        let (code, out) = bp(v, t, &[], &["approve", ISSUE, "--key", &human]);
        assert_eq!((code, out["code"].as_str()), (Some(1), Some("could_not_run")), "{base}: {out}");
        assert!(!v.join(ISSUE).join("approvals").exists(), "{base}: no record is written");
    }
}

/// The signer is read only from origin's own default branch: a `url.<x>.insteadOf` rule that sends `origin` to
/// another repository refuses, and a risk file that does not parse refuses `risk_file_invalid` (review of #95, rows
/// 24 and 25).
#[test]
fn approve_refuses_a_rewritten_origin_and_a_risk_file_that_does_not_parse() {
    let (v, t) = setup();
    let (v, t) = (v.path(), t.path());
    let human = t.join("human").display().to_string();
    let pr =
        json!([{ "number": 4, "state": "OPEN", "headRefName": "bp/a", "headRefOid": HEAD, "baseRefName": "master" }]);
    std::fs::write(t.join("prs.json"), pr.to_string()).unwrap();
    let approve = || bp(v, t, &[], &["approve", ISSUE, "--key", &human]);
    let code = t.join("code");
    let code_dir = code.join(".bp");
    std::fs::create_dir_all(&code_dir).unwrap();
    std::fs::write(code_dir.join("risk.yml"), "high: [\n").unwrap();
    git(&code, &["add", ".bp/risk.yml"]);
    git(&code, &["commit", "-qm", "broken"]);
    git(&code, &["push", "-q", "origin", "master"]);
    let (exit, out) = approve();
    assert_eq!((exit, out["code"].as_str()), (Some(2), Some("risk_file_invalid")), "{out}");
    assert_eq!(out["subject"], "origin/master:.bp/risk.yml", "{out}");

    // Another repository whose risk file names the agent's key, behind a rewrite of origin's url.
    let other = t.join("other.git");
    git(t, &["clone", "-q", "--bare", &t.join("origin.git").display().to_string(), "other.git"]);
    let origin = t.join("origin.git").display().to_string();
    git(&code, &["config", &format!("url.{}.insteadOf", other.display()), &origin]);
    let (exit, out) = approve();
    assert_eq!((exit, out["code"].as_str()), (Some(2), Some("signer_missing")), "{out}");
    assert!(out["fix"].as_str().unwrap().contains("remove the url.<x>.insteadOf rule"), "{out}");
    assert!(!v.join(ISSUE).join("approvals").exists(), "no approval is written");
}
