//! `cargo xtask stage`: a staged copy (32, design change 11). A scratch copy of `$BP_VAULT` and a fake forge for
//! project `bp`, then the tool's own verbs make one issue of each intent the view must show: `needs-landing` (cut,
//! `pr`, approved by the reviewer identity, green at the head), `working` (cut), `blocked` (cut, released
//! `escalated`), `waiting` (depends on the working one), and a `planned` milestone (`needs-milestone-approval`). A
//! `ready` issue is the copy's own. It prints `VAULT=`, `FORGE=` and `CLONE=` lines for a block to `eval`.
//!
//! Every call makes a fresh copy and forge; nothing writes into `$BP_VAULT`. The release `bp` must exist: `cargo build
//! --release -p bp-tool`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::scratch;

/// The reviewer identity the fake forge approves as (26, design change 3).
const REVIEWER: &str = "quokka-reviewer";

/// The block each staged issue carries, which the landing issue's `pr` runs at its head.
const BODY: &str = "## Check\n\n~~~check shell {name=staged expect=0}\ntest -f staged.txt\n~~~\n";

pub fn run() -> Result<String> {
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
    let stage = Stage { bp: release.join("bp"), vault, code: t.join("code"), t };
    stage.make()?;
    Ok(format!("VAULT={}\nFORGE={}\nCLONE={}\n", stage.vault.display(), stage.t.display(), stage.code.display()))
}

struct Stage {
    bp: PathBuf,
    vault: PathBuf,
    /// The fake forge.
    t: PathBuf,
    /// Its clone, where the repo verbs run.
    code: PathBuf,
}

impl Stage {
    fn make(&self) -> Result<()> {
        let body = self.t.join("staged-body.md");
        std::fs::write(&body, BODY)?;
        let body = body.display().to_string();
        self.bp(&["set", "projects/bp/project", "reviewer", REVIEWER])?;
        let design = "projects/bp/designs/system";
        let milestone =
            self.id(&["create", "milestone", "--project", "bp", "--title", "Staged milestone", "--under", design])?;
        self.bp(&["transition", &milestone, "approved"])?;
        let issue = |title: &str| {
            self.id(&[
                "create",
                "issue",
                "--project",
                "bp",
                "--title",
                title,
                "--under",
                &milestone,
                "--body-file",
                &body,
            ])
        };
        let (landing, working, blocked, waiting) =
            (issue("Staged landing")?, issue("Staged working")?, issue("Staged blocked")?, issue("Staged waiting")?);

        // Needs landing: a commit that passes the block, its pull request, the reviewer's approval and a green check at
        // the head. The later repo verbs rewrite the snapshot, so it reads the approval and the check.
        let cut = self.bp(&["cut", &landing])?;
        let worktree =
            Path::new(cut["worktree"].as_str().with_context(|| format!("bp cut {landing} made no worktree"))?);
        std::fs::write(worktree.join("staged.txt"), "staged\n")?;
        self.git(worktree, &["add", "staged.txt"])?;
        self.git(worktree, &["commit", "-qm", "staged"])?;
        let pr = self.bp(&["pr", &landing])?;
        let (number, head) = (pr["pull_request"].to_string(), pr["head"].as_str().context("bp pr printed no head")?);
        self.run(
            Command::new(self.t.join("bin/gh"))
                .args(["pr", "review", &number, "--approve"])
                .env("FAKE_GH_LOGIN", REVIEWER),
        )?;
        std::fs::write(self.t.join("forge/checks").join(format!("{head}.json")), "[{\"conclusion\":\"SUCCESS\"}]\n")?;

        self.bp(&["cut", &working])?;
        self.bp(&["cut", &blocked])?;
        self.bp(&["release", &blocked, "--reason", "escalated"])?;
        self.bp(&["link", &waiting, "depends", &working])?;
        self.id(&["create", "milestone", "--project", "bp", "--title", "Staged planned milestone", "--under", design])?;
        Ok(())
    }

    /// `bp <args> --json` in the clone, as a human with no role: its result, which must be a write (exit 0) or a
    /// repeat (exit 3).
    fn bp(&self, args: &[&str]) -> Result<Value> {
        let out = self.run(Command::new(&self.bp).args(args).arg("--json"))?;
        serde_json::from_slice(&out).with_context(|| format!("bp {args:?} printed no JSON"))
    }

    /// The id a `create` wrote.
    fn id(&self, args: &[&str]) -> Result<String> {
        let made = self.bp(args)?;
        made["id"].as_str().map(str::to_string).with_context(|| format!("bp {args:?} printed no id: {made}"))
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
        self.run(Command::new("git").arg("-C").arg(dir).args(args))
    }

    /// Runs `cmd` in the clone against the copy and the fake forge, with git's global config left out so the
    /// owner's own (signing, hooks) cannot reach the copy. Exit 3, a repeat, passes.
    fn run(&self, cmd: &mut Command) -> Result<Vec<u8>> {
        let path = format!("{}:{}", self.t.join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let out = cmd
            .current_dir(&self.code)
            .env("BP_VAULT", &self.vault)
            .env("PATH", path)
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("BP_ROLE")
            .env_remove("BP_SESSION")
            .env_remove("BP_FORGE_SNAPSHOT")
            .env_remove("BP_NOW")
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("run {cmd:?}"))?;
        if !matches!(out.status.code(), Some(0 | 3)) {
            bail!(
                "{cmd:?} exited {}: {}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(out.stdout)
    }
}
