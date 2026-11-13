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
    fn comment(&self, key: &str, fields: [&str; 5], signed: [&str; 5]) -> String {
        let text = |f: [&str; 5]| {
            let names = ["act", "project", "node", "pull_request", "head"];
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

    /// `bp gate` for `head` from the clone, with `comments` written to a file when given.
    fn gate(&self, head: &str, comments: Option<&Value>) -> (Option<i32>, Value) {
        let pr = PR.to_string();
        let mut args = vec!["gate", "--base", &self.base, "--head", head, "--pr", &pr, "--project", "bp"];
        let path = self.dir.path().join("comments.json");
        let path_text = path.display().to_string();
        if let Some(c) = comments {
            std::fs::write(&path, c.to_string()).unwrap();
            args.extend(["--comments", &path_text]);
        }
        common::bp_env(&self.code(), &[("SSH_AUTH_SOCK", "")], &args)
    }
}

const RULES: &str = "high: [infra/]\nlow: [docs/]\nsigner: {human}\n";

const ISSUE: &str = "projects/bp/issues/a";

/// The exit and the refusal code, `""` for a pass.
fn code(out: &(Option<i32>, Value)) -> (Option<i32>, String) {
    (out.0, out.1["code"].as_str().unwrap_or_default().to_string())
}

fn at(f: &[String; 5]) -> [&str; 5] {
    [&f[0], &f[1], &f[2], &f[3], &f[4]]
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
    let ours = |h: &str| ["approve", "bp", ISSUE, &pr, h].map(String::from);
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
    let pr8 = ["approve", "bp", ISSUE, "8", &head];
    assert_eq!(refused(Some(&json!([w.comment("human", pr8, pr8)]))), (Some(2), "approval_missing".into()));
    let elsewhere = ["approve", "mentat", "projects/mentat/issues/a", &pr, &head];
    assert_eq!(refused(Some(&json!([w.comment("human", elsewhere, elsewhere)]))), (Some(2), "approval_missing".into()));

    // Each block at this head that an agent can make without the human's key refuses.
    let lower = ["lower", "bp", ISSUE, &pr, &head];
    for (why, body) in [
        ("the agent's own key", w.comment("agent", at(&good), at(&good))),
        ("another head's signature, relabelled", w.comment("human", at(&good), at(&ours(&other)))),
        ("another pull request's signature, relabelled", w.comment("human", at(&good), pr8)),
        ("another act's signature, relabelled", w.comment("human", at(&good), lower)),
        ("an issue of another project", {
            let f = ["approve", "bp", "projects/mentat/issues/a", &pr, &head];
            w.comment("human", f, f)
        }),
        ("another project's valid approval, relabelled", {
            let theirs = ["approve", "mentat", "projects/mentat/issues/a", &pr, &head];
            w.comment("human", ["approve", "bp", "projects/mentat/issues/a", &pr, &head], theirs)
        }),
        (
            "no signature",
            format!("```bp-approval\nact: approve\nproject: bp\nnode: {ISSUE}\npull_request: {PR}\nhead: {head}\n```"),
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
        &["gate", "--base", &w.base, "--head", &head, "--pr", &pr, "--project", "bp", "--comments", &path],
    );
    assert_eq!((exit, v["level"].as_str(), v["approval"].as_str()), (Some(0), Some("high"), Some("signed")), "{v}");
}

#[test]
fn gate_reads_the_rules_and_the_signer_at_the_base_never_the_head() {
    let w = World::new(RULES);
    // The head drops `infra/` from the high paths and names the agent's key.
    let rules = format!("high: []\nlow: [docs/, infra/]\nsigner: {}\n", w.public("agent"));
    let head = w.commit(&[(".bp/risk.yml", &rules), ("infra/b.tf", "b\n")]);
    let pr = PR.to_string();
    let fields = ["approve", "bp", ISSUE, &pr, &head];
    let (exit, v) = w.gate(&head, Some(&json!([w.comment("agent", fields, fields)])));
    assert_eq!((exit, v["code"].as_str()), (Some(2), "signature_invalid".into()), "the head's signer is not trusted");
    let (exit, v) = w.gate(&head, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!(exit, Some(0), "{v}");
    let reasons = v["reasons"].to_string();
    assert!(reasons.contains("infra/b.tf matches the high pattern infra/"), "the base's rules judge it: {v}");

    // A base with no signer passes no high head, whatever signer the head names.
    let w = World::new("high: [infra/]\n");
    let rules = format!("signer: {}\n", w.public("human"));
    let head = w.commit(&[(".bp/risk.yml", &rules), ("src/b.rs", "b\n")]);
    let fields = ["approve", "bp", ISSUE, &pr, &head];
    let (exit, v) = w.gate(&head, Some(&json!([w.comment("human", fields, fields)])));
    assert_eq!((exit, v["code"].as_str()), (Some(2), "signer_missing".into()), "{v}");
    assert_eq!(v["subject"].as_str(), Some(format!("{}:.bp/risk.yml", w.base).as_str()));
}
