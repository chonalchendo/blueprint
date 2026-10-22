//! `cargo xtask sandbox [claim]`: 27's shared prelude for a block that runs the bundle's hooks, printed as `export`
//! lines for the block to `eval`. A scratch vault `D`, a fake forge `T` whose clone `T/code` holds a Claude Code
//! install, and a temporary home `H`, with every harness variable pointed into them. With `claim`, a worker holds a
//! claim on the first ready issue `I`, in the worktree `W`; `G` is the installed guard hook's command and `K` the
//! role hook's. The agent id of that worker is `a-q`.
//!
//! The lines set `HOME`, so a block runs `cargo` before it evaluates them. The release `bp` must exist: `cargo build
//! --release -p bp-tool`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::scratch;

/// Homes kept, the new one included.
const KEEP: usize = 16;

/// The claim's session, which the block's hook inputs name as their `agent_id`: the role hook labels a worker's calls
/// with its agent id, so `cut` records it the same way.
const AGENT: &str = "a-q";

pub fn run(claim: bool) -> Result<String> {
    let release = std::env::current_dir()?.join("target/release");
    if !release.join("bp").exists() {
        bail!("{} holds no bp; run cargo build --release -p bp-tool first", release.display());
    }
    let vault = scratch::run()?;
    let forge = Command::new(std::env::current_exe()?)
        .args(["forge", "bp"])
        .env("BP_VAULT", &vault)
        .output()
        .context("run xtask forge")?;
    if !forge.status.success() {
        bail!("xtask forge exited {}: {}", forge.status, String::from_utf8_lossy(&forge.stderr));
    }
    let t = PathBuf::from(String::from_utf8(forge.stdout)?.trim());
    let h = scratch::fresh(&std::env::temp_dir().join("bp-sandbox"), KEEP)?;
    let code = t.join("code");
    let path = format!("{}:{}", t.join("bin").display(), release.display());
    let env = [
        ("BP_VAULT", vault.display().to_string()),
        ("GIT_ALLOW_PROTOCOL", "file".into()),
        ("HOME", h.display().to_string()),
        ("XDG_CONFIG_HOME", h.display().to_string()),
        ("CODEX_HOME", h.join(".codex").display().to_string()),
        ("CLAUDE_CONFIG_DIR", h.join(".claude").display().to_string()),
        ("CLAUDE_PROJECT_DIR", code.display().to_string()),
    ];
    let mut out = String::from("unset BP_ROLE BP_SESSION BP_HARNESS\n");
    writeln!(out, "export D='{}' T='{}' H='{}'", vault.display(), t.display(), h.display())?;
    writeln!(out, "export PATH='{path}':\"$PATH\"")?;
    for (name, value) in &env {
        writeln!(out, "export {name}='{value}'")?;
    }
    if claim {
        let bp = |args: &[&str], role: bool| -> Result<Value> {
            let mut cmd = Command::new(release.join("bp"));
            cmd.args(args).arg("--json").current_dir(&code).envs(env.iter().map(|(k, v)| (k, v)));
            cmd.env("PATH", format!("{path}:{}", std::env::var("PATH").unwrap_or_default()));
            if role {
                cmd.env("BP_ROLE", "worker").env("BP_SESSION", AGENT);
            } else {
                cmd.env_remove("BP_ROLE").env_remove("BP_SESSION");
            }
            let run = cmd.stdin(std::process::Stdio::null()).output().with_context(|| format!("run bp {args:?}"))?;
            serde_json::from_slice(&run.stdout).with_context(|| format!("bp {args:?} printed no JSON"))
        };
        bp(&["install", "--target", "claude"], false)?;
        let ready = bp(&["list", "issue", "--ready", "--limit", "1"], false)?;
        let issue = ready["rows"][0]["id"].as_str().context("the scratch vault holds no ready issue")?;
        let cut = bp(&["cut", issue], true)?;
        let worktree = cut["worktree"].as_str().with_context(|| format!("bp cut {issue} made no worktree: {cut}"))?;
        writeln!(out, "export I='{issue}' W='{worktree}'")?;
        let settings = std::fs::read_to_string(code.join(".claude/settings.local.json"))?;
        let settings: Value = serde_json::from_str(&settings)?;
        for (name, script) in [("G", "bp-guard"), ("K", "bp-role")] {
            writeln!(out, "export {name}='{}'", command(&settings, script, &code)?)?;
        }
    }
    Ok(out)
}

/// The command the install registered for `script`.
fn command(settings: &Value, script: &str, code: &Path) -> Result<String> {
    settings["hooks"]["PreToolUse"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| g["hooks"].as_array().into_iter().flatten())
        .find_map(|h| h["command"].as_str().filter(|c| c.contains(script)))
        .map(str::to_string)
        .with_context(|| format!("{} registers no {script} hook", code.join(".claude/settings.local.json").display()))
}
