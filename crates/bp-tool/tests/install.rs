//! `bp install` and the hooks it ships, through the binary: a vault, a clone whose origin is the project's repository,
//! and a temporary home, so nothing reaches the real harness directories (27, design changes 1, 2 and 4).

// `common::bp` runs under the real home; every command here runs under a temporary one instead.
#[allow(dead_code)]
mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::git;
use serde_json::Value;

const MILESTONE: &str = "projects/bp/milestones/m";
const ISSUE: &str = "projects/bp/issues/a";

/// The vault, a directory holding the clone `code`, and the temporary home every command runs under.
struct World {
    vault: tempfile::TempDir,
    t: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let project = "---\nkind: project\ntitle: Bp\nstatus: active\nrepository: https://h.invalid/o/r\n---\n";
        let vault = common::vault(&[
            ("shared/.gitkeep", ""),
            ("projects/bp/project.md", project),
            (&format!("{MILESTONE}.md"), "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n"),
            (&format!("{ISSUE}.md"), "---\nkind: issue\ntitle: A\nstatus: open\n---\n\nBody.\n"),
        ]);
        let t = tempfile::tempdir().unwrap();
        git(t.path(), &["init", "-q", "-b", "master", "code"]);
        git(&t.path().join("code"), &["remote", "add", "origin", "https://h.invalid/o/r"]);
        Self { vault, t, home: tempfile::tempdir().unwrap() }
    }

    fn code(&self) -> PathBuf {
        self.t.path().join("code")
    }

    /// A command with the vault named, the home redirected and no role or session inherited.
    fn command(&self, program: impl AsRef<std::ffi::OsStr>, cwd: &Path) -> Command {
        let mut c = Command::new(program);
        let h = self.home.path();
        c.current_dir(cwd)
            .env("BP_VAULT", self.vault.path())
            .env("BP_NOW", "2026-10-19T08:00:00Z")
            .env_remove("BP_ROLE")
            .env_remove("BP_SESSION")
            .env("HOME", h)
            .env("XDG_CONFIG_HOME", h)
            .env("CLAUDE_CONFIG_DIR", h.join(".claude"))
            .env("CODEX_HOME", h.join(".codex"))
            .env("CLAUDE_PROJECT_DIR", self.code());
        c
    }

    fn bp(&self, cwd: &Path, env: &[(&str, &str)], args: &[&str]) -> (Option<i32>, Value) {
        let (h, vault) = (self.home.path().display().to_string(), self.vault.path().display().to_string());
        let (claude, codex) = (format!("{h}/.claude"), format!("{h}/.codex"));
        let base = [
            ("BP_VAULT", vault.as_str()),
            ("BP_SESSION", ""),
            ("HOME", h.as_str()),
            ("XDG_CONFIG_HOME", h.as_str()),
            ("CLAUDE_CONFIG_DIR", claude.as_str()),
            ("CODEX_HOME", codex.as_str()),
        ];
        let env: Vec<(&str, &str)> = base.into_iter().chain(env.iter().copied()).collect();
        common::bp_env(cwd, &env, args)
    }

    /// Runs an installed hook script with `input` on stdin, `path` as `PATH`, and `env` over the world's.
    fn hook(&self, script: &str, input: &str, path: &str, env: &[(&str, &str)]) -> Output {
        let mut c = self.command(self.code().join(".claude/hooks").join(script), &self.code());
        c.env("PATH", path)
            .envs(env.iter().copied())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = c.spawn().unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }
}

/// A directory of links to `programs`, for a `PATH` that holds only them, and the system's path after it when `rest`.
fn bin(dir: &Path, programs: &[(&str, PathBuf)], rest: bool) -> String {
    std::fs::create_dir_all(dir).unwrap();
    for (name, target) in programs {
        std::os::unix::fs::symlink(target, dir.join(name)).unwrap();
    }
    if rest { format!("{}:{}", dir.display(), std::env::var("PATH").unwrap()) } else { dir.display().to_string() }
}

/// The path of a program on the test's own `PATH`.
fn which(name: &str) -> PathBuf {
    let out = Command::new("sh").args(["-c", &format!("command -v {name}")]).output().unwrap().stdout;
    PathBuf::from(String::from_utf8(out).unwrap().trim())
}

#[test]
fn install_refuses_outside_a_clone_and_a_repeat_exits_3() {
    let w = World::new();
    let (code, v) = w.bp(w.t.path(), &[], &["install", "--target", "claude"]);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("no_clone")), "{v}");
    assert!(v["fix"].as_str().unwrap().contains("projects/bp/project https://h.invalid/o/r"), "{v}");
    assert_eq!(common::commits(w.vault.path()), "1", "install writes nothing in the vault");
    let (code, v) = w.bp(&w.code(), &[], &["install", "--target", "claude"]);
    assert_eq!((code, v["target"].as_str()), (Some(0), Some("claude")), "{v}");
    assert_eq!(w.bp(&w.code(), &[], &["install", "--target", "claude"]).0, Some(3));
    assert_eq!(std::fs::read_dir(w.home.path()).unwrap().count(), 0, "the install writes nothing in the home");
}

#[test]
fn walk_refuses_a_drifted_bundle_before_it_reads_the_vault() {
    let w = World::new();
    let walk = ["walk", MILESTONE, "--for", "derive", "--ids"];
    assert_eq!(w.bp(&w.code(), &[], &walk).0, Some(0), "a clone with no bundle is not checked");
    w.bp(&w.code(), &[], &["install", "--target", "claude"]);
    let skill = w.code().join(".claude/skills/bp-complete/SKILL.md");
    std::fs::write(&skill, "edited").unwrap();
    let (code, v) = w.bp(&w.code(), &[("BP_VAULT", "/nonexistent")], &walk);
    assert_eq!((code, v["code"].as_str()), (Some(2), Some("bundle_mismatch")), "{v}");
    assert!(v["fix"].as_str().unwrap().ends_with("bp install --target claude"), "{v}");
}

#[test]
fn the_guard_denies_a_vault_path_and_fails_closed() {
    let w = World::new();
    w.bp(&w.code(), &[], &["install", "--target", "claude"]);
    let bp = assert_cmd::cargo::cargo_bin("bp");
    let path = bin(&w.t.path().join("bin"), &[("bp", bp.clone())], true);
    let edit = |p: &Path| {
        serde_json::json!({ "hook_event_name": "PreToolUse", "tool_name": "Edit", "tool_input": { "file_path": p } })
            .to_string()
    };
    let guard = |p: &Path, path: &str, env: &[(&str, &str)]| w.hook("bp-guard.sh", &edit(p), path, env);
    let vault = w.vault.path();
    let inside = vault.join("projects/bp/project.md");
    let out = guard(&inside, &path, &[]);
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(guard(&vault.join("projects/bp/new/deep.md"), &path, &[]).status.code(), Some(2), "a new file");
    let notes = w.code().join("notes.md");
    let out = guard(&notes, &path, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    // `..` after a directory that does not exist: the walk alone would stop outside the vault.
    let ups = "../".repeat(w.code().join("nope").components().count());
    let through = w.code().join("nope").join(format!("{ups}{}", inside.display().to_string().trim_start_matches('/')));
    assert_eq!(guard(&through, &path, &[]).status.code(), Some(2), "{}", through.display());
    // A file in the clone that links to a vault file: a Write follows the link.
    let link = w.code().join("link.md");
    std::os::unix::fs::symlink(&inside, &link).unwrap();
    assert_eq!(guard(&link, &path, &[]).status.code(), Some(2), "a symlink to a vault file");
    // Fails closed: each cause denies even a path outside the vault, and the message names it.
    let (bash, jq) = (which("bash"), which("jq"));
    let no_jq = bin(&w.t.path().join("nojq"), &[("bash", bash.clone()), ("bp", bp)], false);
    let no_bp = bin(&w.t.path().join("nobp"), &[("bash", bash), ("jq", jq)], false);
    for (path, env, cause) in [
        (no_jq.as_str(), &[][..], "jq"),
        (no_bp.as_str(), &[][..], "bp is not on the hook's PATH"),
        (path.as_str(), &[("BP_VAULT", "/nonexistent")][..], "no_vault"),
    ] {
        let out = guard(&notes, path, env);
        assert_eq!(out.status.code(), Some(2), "{cause}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(cause), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let empty = w.hook("bp-guard.sh", "", &path, &[]);
    assert_eq!(empty.status.code(), Some(2), "an empty input denies");
}

#[test]
fn the_session_hooks_record_the_seed_and_walk_it() {
    let w = World::new();
    w.bp(&w.code(), &[], &["install", "--target", "claude"]);
    let path = bin(&w.t.path().join("bin"), &[("bp", assert_cmd::cargo::cargo_bin("bp"))], true);
    let (code, v) =
        w.bp(&w.code(), &[("BP_ROLE", "session"), ("BP_SESSION", "s-one")], &["set", MILESTONE, "tags", "a"]);
    assert_eq!(code, Some(0), "{v}");
    assert!(git(w.vault.path(), &["log", "-1", "--format=%B"]).lines().any(|l| l == "Bp-Session: s-one"));
    let end = w.hook(
        "bp-end.sh",
        &serde_json::json!({ "hook_event_name": "SessionEnd", "session_id": "s-one" }).to_string(),
        &path,
        &[],
    );
    assert_eq!(end.status.code(), Some(0), "{}", String::from_utf8_lossy(&end.stderr));
    let (_, handoff) = w.bp(w.vault.path(), &[], &["show", "projects/bp/handoffs/2026-10-19-s-one", "--no-body"]);
    assert_eq!(handoff["seed"], MILESTONE, "{handoff}");
    // A later session that committed nothing leaves a handoff with no seed; the start looks past it.
    let empty = serde_json::json!({ "hook_event_name": "SessionEnd", "session_id": "s-empty" }).to_string();
    assert_eq!(w.hook("bp-end.sh", &empty, &path, &[]).status.code(), Some(0));
    let (_, seedless) = w.bp(w.vault.path(), &[], &["show", "projects/bp/handoffs/2026-10-19-s-empty", "--no-body"]);
    assert!(seedless["id"].is_string() && seedless.get("seed").is_none(), "{seedless}");
    let start = w.hook(
        "bp-start.sh",
        &serde_json::json!({ "hook_event_name": "SessionStart", "session_id": "s-two" }).to_string(),
        &path,
        &[],
    );
    let v: Value = serde_json::from_slice(&start.stdout).unwrap();
    assert_eq!((v["seed"].as_str(), v["for"].as_str()), (Some(MILESTONE), Some("derive")), "{v}");
}

#[test]
fn a_handoff_seed_follows_a_record_to_its_node() {
    let w = World::new();
    let head = "a".repeat(40);
    let (code, v) = w.bp(
        w.vault.path(),
        &[("BP_SESSION", "s-r")],
        &["record", "review", ISSUE, "--field", &format!("head={head}")],
    );
    assert_eq!(code, Some(0), "{v}");
    let (code, v) = w.bp(w.vault.path(), &[], &["record", "handoff", "projects/bp/project", "--field", "session=s-r"]);
    assert_eq!(code, Some(0), "{v}");
    let (_, handoff) = w.bp(w.vault.path(), &[], &["show", v["id"].as_str().unwrap(), "--no-body"]);
    assert_eq!(handoff["seed"], ISSUE, "{handoff}");
    let (_, v) = w.bp(w.vault.path(), &[], &["record", "handoff", "projects/bp/project", "--field", "session=s-none"]);
    let (_, handoff) = w.bp(w.vault.path(), &[], &["show", v["id"].as_str().unwrap(), "--no-body"]);
    assert!(handoff.get("seed").is_none(), "a session with no commit has no seed: {handoff}");
}

#[test]
fn the_role_hook_names_the_role_and_denies_a_verb_outside_it() {
    let w = World::new();
    w.bp(&w.code(), &[], &["install", "--target", "claude"]);
    let path = bin(&w.t.path().join("bin"), &[("bp", assert_cmd::cargo::cargo_bin("bp"))], true);
    let call = |command: &str, agent: Option<&str>| {
        let mut input = serde_json::json!({
            "hook_event_name": "PreToolUse", "session_id": "s-one", "tool_name": "Bash",
            "tool_input": { "command": command, "timeout": 9 },
        });
        if let Some(a) = agent {
            input["agent_id"] = "a-one".into();
            input["agent_type"] = a.into();
        }
        w.hook("bp-role.sh", &input.to_string(), &path, &[])
    };
    // The session's call runs as the harness runs it: the returned input, with every other key kept.
    let out = call(&format!("cd {} && bp set {MILESTONE} tags a", w.code().display()), None);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let input = &v["hookSpecificOutput"]["updatedInput"];
    assert!(v["hookSpecificOutput"].get("permissionDecision").is_none(), "{v}");
    assert_eq!(input["timeout"], 9, "{v}");
    let run = w.command("bash", &w.code()).env("PATH", &path).args(["-c", input["command"].as_str().unwrap()]).output();
    assert!(run.unwrap().status.success());
    let message = git(w.vault.path(), &["log", "-1", "--format=%B"]);
    assert!(message.contains("Bp-Role: session\nBp-Session: s-one"), "{message}");
    let out = call("bp walk x --for complete --json | jq .", Some("bp-worker"));
    assert!(String::from_utf8_lossy(&out.stdout).contains("export BP_ROLE=worker BP_SESSION='a-one'; bp walk"));
    // A read and a call with no verb pass for every role; an agent type with no role reads with no prefix.
    for (command, agent, prefix) in [
        ("bp list issue --json", Some("bp-reviewer"), "export BP_ROLE=reviewer"),
        ("bp --help", Some("bp-worker"), "export BP_ROLE=worker"),
        ("bp where && bp show x", Some("Explore"), ""),
    ] {
        let out = call(command, agent);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success() && stdout.contains(prefix) && prefix.is_empty() == stdout.is_empty(), "{command}");
    }
    let no_jq = bin(&w.t.path().join("nojq"), &[("bash", which("bash"))], false);
    for (command, agent, cause) in [
        ("cd /tmp && FOO=1 timeout 30 bp --json land x", Some("bp-worker"), "bp land is outside the worker role"),
        ("echo $(bp record review x)", Some("bp-reviewer"), "bp record is outside the reviewer role"),
        ("bp show x; bp set x tags a", Some("Explore"), "agent type 'Explore' is no bp role, so its bp set"),
        ("bp stamp m", None, "bp stamp is outside the session role"),
    ] {
        let out = call(command, agent);
        assert_eq!(out.status.code(), Some(2), "{command}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(cause), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let out = call("ls -la", Some("bp-worker"));
    assert!(out.status.success() && out.stdout.is_empty(), "a command with no bp is not touched");
    // Fails closed: no jq, malformed input and empty input each deny, whatever the command.
    let ls = serde_json::json!({ "session_id": "s", "tool_input": { "command": "ls" } }).to_string();
    for (input, path, cause) in
        [(ls.as_str(), no_jq.as_str(), "jq"), ("{bad", path.as_str(), "JSON"), ("", &path, "empty")]
    {
        let out = w.hook("bp-role.sh", input, path, &[]);
        assert_eq!(out.status.code(), Some(2), "{cause}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(cause), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
