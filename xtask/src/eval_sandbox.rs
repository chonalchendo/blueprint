//! The sandbox of one trial (29, design change 1). It is the replay's: a clone at the fixture's recorded base wired to
//! the fake forge, a scratch copy of the vault, the bundle installed into the clone with every config home pointed at
//! a scratch directory, and the claim state and stop point of the stored run. Review and respond start at an open
//! pull request at the fixture's head (respond adds the recorded review); complete starts at an open issue with no
//! claim, and with a dependency's open pull request when the fixture is stacked.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use bp_operation::Action;

use crate::eval::Skill;
use crate::eval_arms::FILLER;
use crate::eval_fixture::Fixture;

/// The design every sandbox issue cites: the walk refuses an issue that cites none (21, rule 3).
const DESIGN: &str = "projects/bp/designs/system";
const PROJECT: &str = "bp";

pub struct Sandbox {
    pub vault: PathBuf,
    pub forge: PathBuf,
    pub issue: String,
    bindir: PathBuf,
    /// One cargo target directory for every trial: the issue's block builds `xtask` in each clone. `$BP_EVAL_TARGET`
    /// keeps it across runs, so a smoke trial warms the build of the full run.
    target: PathBuf,
    /// The vault's head when the agent starts: the log is what the agent and the session add after it.
    start: String,
    /// What the worker's branch is measured against: the clone's base, or the dependency's head when stacked.
    pub scope_base: String,
}

impl Sandbox {
    /// Builds the sandbox of one trial. For a review the pull request `fx` records is open on the fake forge and its
    /// issue is claimed, so `walk --for review` reads `needs-review`; a respond adds the review, so it reads
    /// `needs-response`; a complete leaves the issue open and unclaimed. `repo` holds the base and the head.
    ///
    /// `check_arm` asserts, before the agent costs anything, that the clone holds the arm: the skill and the role's
    /// `skills:` line as shipped for `with` and `filler`, neither for `without`. A skill placed wrong reads as zero
    /// recall (RA 5.5, 6). `work` holds the arm binaries and the cargo target directory that the sandbox shares.
    pub fn build(
        repo: &Path,
        source: &Path,
        bindir: &Path,
        work: &Path,
        fx: &Fixture,
        skill: &Skill,
        check_arm: Option<&str>,
    ) -> Result<Self> {
        let exe = std::env::current_exe().context("find the xtask binary")?;
        let made = |verb: &[&str], vault: &Path| {
            capture(Command::new(&exe).args(verb).env("BP_VAULT", vault).current_dir(work).stdin(Stdio::null()))
        };
        let vault = PathBuf::from(made(&["scratch"], source)?);
        let forge = PathBuf::from(made(&["forge", PROJECT], &vault)?);
        let mut sb = Self {
            vault,
            forge,
            issue: String::new(),
            bindir: bindir.to_path_buf(),
            target: std::env::var_os("BP_EVAL_TARGET").map_or_else(|| work.join("target"), PathBuf::from),
            start: String::new(),
            scope_base: fx.stack.as_ref().map_or(&fx.base, |s| &s.0).clone(),
        };
        let code = sb.code();
        let repo_arg = repo.display().to_string();
        let mut fetch = vec!["fetch", "-q", repo_arg.as_str(), &fx.base, &fx.head];
        fetch.extend(fx.stack.iter().map(|s| s.0.as_str()));
        git(&code, &fetch)?;
        git(&code, &["reset", "-q", "--hard", &fx.base])?;
        git(&code, &["push", "-q", "-f", "origin", "HEAD:master"])?;

        let milestone = sb.forge.join("milestone.body");
        std::fs::write(&milestone, "## Goal\n\nA sandbox milestone for an eval trial.\n")?;
        let m = "projects/bp/milestones/eval-sandbox";
        sb.bp(
            &code,
            None,
            &[
                "create",
                "milestone",
                "--project",
                PROJECT,
                "--title",
                "Eval sandbox",
                "--under",
                DESIGN,
                "--body-file",
                &milestone.display().to_string(),
            ],
        )?;
        sb.bp(&code, None, &["transition", m, "approved"])?;
        sb.issue = sb.create_issue(&code, m, &fx.title, &fx.issue)?;

        // Every config home points into the forge directory, so the install touches nothing of the real home.
        let cfg = sb.forge.join("config");
        std::fs::create_dir_all(&cfg)?;
        let mut install = sb.command("bp", &code);
        for (key, dir) in
            [("HOME", "home"), ("XDG_CONFIG_HOME", "xdg"), ("CODEX_HOME", "codex"), ("CLAUDE_CONFIG_DIR", "claude")]
        {
            install.env(key, cfg.join(dir));
        }
        capture(install.args(["install", "--target", "claude", "--json"]))?;

        if let Some(arm) = check_arm {
            sb.assert_arm(skill, arm)?;
        }
        if let Some((head, title)) = &fx.stack {
            // The dependency holds an open pull request at its landed head and a check that needs no build, so the
            // worker's `cut` stacks on it (21, the complete row) and the trial pays for only its own issue.
            let stub = "## Check\n\n~~~check shell {name=stub expect=0}\ntrue\n~~~\n";
            let dependency = sb.create_issue(&code, m, title, stub)?;
            sb.open_pr(&code, &dependency, head)?;
            sb.bp(&code, None, &["link", &sb.issue.clone(), "depends", &dependency])?;
        }
        match skill.action {
            Action::Complete => {}
            Action::Respond => {
                sb.open_pr(&code, &sb.issue.clone(), &fx.head)?;
                if !sb.record_review(&fx.head, &fx.review_body())? {
                    bail!("the vault refused the recorded review of {}", fx.id);
                }
            }
            _ => sb.open_pr(&code, &sb.issue.clone(), &fx.head)?,
        }
        sb.start = git(&sb.vault, &["rev-parse", "HEAD"])?.trim().to_string();
        Ok(sb)
    }

    /// An issue under `milestone` that cites the design, so the walk accepts it (21, rule 3).
    fn create_issue(&self, code: &Path, milestone: &str, title: &str, text: &str) -> Result<String> {
        let file = self.forge.join("issue.body");
        std::fs::write(&file, format!("{text}\n"))?;
        let args = ["create", "issue", "--project", PROJECT, "--title", title, "--under", milestone, "--body-file"];
        let made = self.bp(code, None, &[&args[..], &[&file.display().to_string()]].concat())?;
        let id = made["id"].as_str().context("create issue returned no id")?.to_string();
        self.bp(code, None, &["link", &id, "cites", DESIGN])?;
        Ok(id)
    }

    /// The worker's cut, the fixture's commits and `pr`: a pull request at `head` that the claim holds.
    fn open_pr(&self, code: &Path, issue: &str, head: &str) -> Result<()> {
        let claimed = self.bp(code, Some("worker"), &["cut", issue])?;
        let worktree = PathBuf::from(claimed["worktree"].as_str().context("cut returned no worktree")?);
        git(&worktree, &["merge", "-q", "--ff-only", head])?;
        self.bp(&worktree, Some("worker"), &["pr", issue])?;
        Ok(())
    }

    /// The installed clone holds what the arm says (see [`Sandbox::build`]).
    fn assert_arm(&self, skill: &Skill, arm: &str) -> Result<()> {
        let code = self.code();
        let listed = |role: &Path| -> Result<Vec<String>> {
            let text = std::fs::read_to_string(role).with_context(|| format!("read {}", role.display()))?;
            let line = text.lines().find_map(|l| l.strip_prefix("skills:")).unwrap_or_default();
            Ok(line.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        };
        let want = listed(&Path::new("bundle/roles").join(format!("{}.md", skill.role_file())))?;
        let have = listed(&code.join(format!(".claude/agents/{}.md", skill.role_file())))?;
        let installed = code.join(".claude/skills").join(&skill.name).join("SKILL.md");
        let body = std::fs::read_to_string(&installed).unwrap_or_default();
        let ok = match arm {
            "without" => {
                !installed.exists() && have == want.iter().filter(|s| **s != skill.name).cloned().collect::<Vec<_>>()
            }
            "filler" => have == want && body.contains(FILLER),
            _ => have == want && !body.is_empty() && !body.contains(FILLER),
        };
        if !ok {
            bail!(
                "the {arm} arm's clone does not hold the arm: role skills {have:?} (shipped {want:?}), skill file {}",
                installed.display()
            );
        }
        Ok(())
    }

    pub fn code(&self) -> PathBuf {
        self.forge.join("code")
    }

    /// A command in `dir` with the sandbox's environment: the fake `gh` and the arm's `bp` first on the path, the
    /// scratch vault, and no role or session label from the caller's shell.
    pub fn command(&self, program: &str, dir: &Path) -> Command {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            [self.forge.join("bin"), self.bindir.clone()].into_iter().chain(std::env::split_paths(&path)),
        );
        let mut c = Command::new(if program == "bp" { self.bindir.join("bp") } else { program.into() });
        c.current_dir(dir)
            .env("PATH", path.unwrap_or_default())
            .env("BP_VAULT", &self.vault)
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("CARGO_TARGET_DIR", &self.target)
            .env("CARGO_NET_OFFLINE", "true")
            .env_remove("BP_ROLE")
            .env_remove("BP_SESSION")
            .env_remove("BP_NOW")
            .stdin(Stdio::null());
        c
    }

    pub(crate) fn bp(&self, dir: &Path, role: Option<&str>, args: &[&str]) -> Result<Value> {
        let mut c = self.command("bp", dir);
        if let Some(r) = role {
            c.env("BP_ROLE", r).env("BP_SESSION", "eval");
        }
        let out = capture(c.args(args).arg("--json"))?;
        serde_json::from_str(&out).with_context(|| format!("bp {} printed no JSON", args[0]))
    }

    /// The session's part of a review dispatch (21, the review row): it records what the reviewer returned. A
    /// refusal is a result, not an error: the stop then reads false.
    pub fn record_review(&self, head: &str, body: &str) -> Result<bool> {
        let file = self.forge.join("review.md");
        std::fs::write(&file, body)?;
        let field = format!("head={head}");
        let mut c = self.command("bp", &self.code());
        c.env("BP_ROLE", "session").env("BP_SESSION", "eval");
        let status = c
            .args(["record", "review", &self.issue, "--field", &field, "--field", "role=reviewer", "--body-file"])
            .arg(&file)
            .arg("--json")
            .stdout(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    /// The vault commits after the agent started: subject and body of each, oldest first.
    pub fn vault_log(&self) -> Result<String> {
        capture(
            Command::new("git")
                .args(["log", "--reverse", "--format=%s%n%b%n", &format!("{}..HEAD", self.start)])
                .current_dir(&self.vault),
        )
    }

    /// Runs the agent command in the clone and sends its output to `log`. The command gets the prompt's file, the
    /// path where a custom agent may write the role's transcript, the arm and the fixture. A non-zero exit is the
    /// agent's result. A run past `limit` is killed with its children, and the reason comes back.
    pub fn run_agent(
        &self,
        command: &str,
        prompt: &str,
        (arm, fixture): (&str, &str),
        (log, limit): (&Path, Duration),
    ) -> Result<(PathBuf, Option<String>)> {
        let file = self.forge.join("prompt.txt");
        std::fs::write(&file, prompt)?;
        let transcript = self.forge.join("transcript.jsonl");
        let out = std::fs::File::create(log)?;
        let mut c = self.command("sh", &self.code());
        // Its own process group, so a kill reaches the `claude` the shell started.
        let mut child = c
            .args(["-c", command])
            .env("BP_EVAL_PROMPT", &file)
            .env("BP_EVAL_TRANSCRIPT", &transcript)
            .env("BP_EVAL_ARM", arm)
            .env("BP_EVAL_FIXTURE", fixture)
            .stdout(out.try_clone()?)
            .stderr(out)
            .process_group(0)
            .spawn()
            .context("run the agent command")?;
        let started = Instant::now();
        let failure = loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    eprintln!("eval: the agent command exited {status}; its output is in {}", log.display());
                }
                break None;
            }
            if started.elapsed() > limit {
                capture(Command::new("kill").args(["-s", "KILL", "--", &format!("-{}", child.id())]))?;
                child.wait()?;
                break Some(format!("the agent ran past {} s and was killed", limit.as_secs()));
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        Ok((transcript, failure))
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    capture(Command::new("git").args(args).current_dir(dir).env("GIT_ALLOW_PROTOCOL", "file"))
}

/// Runs a command and returns its stdout. A failure carries stderr, and stdout when it holds an envelope.
pub(crate) fn capture(cmd: &mut Command) -> Result<String> {
    let out = cmd.output().with_context(|| format!("run {cmd:?}"))?;
    if !out.status.success() {
        bail!(
            "{cmd:?} exited {}: {} {}",
            out.status,
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
