//! The arms of an eval and the sandbox of one trial (29, design change 1). The sandbox is the replay's: a clone at the
//! fixture's recorded base wired to the fake forge, a scratch copy of the vault, the bundle installed into the clone
//! with every config home pointed at a scratch directory, and the claim state and stop point of the stored run. An
//! arm is a `bp` binary that embeds another bundle: the manifest check refuses an installed file that differs from the
//! embedded one, so the skill cannot be taken out of an install.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::eval::Skill;
use crate::eval_fixture::Fixture;

/// The text of the `filler` arm's body: it holds no instruction and no fact.
pub const FILLER: &str = "This line holds no instruction.";

/// The design every sandbox issue cites: the walk refuses an issue that cites none (21, rule 3).
const DESIGN: &str = "projects/bp/designs/system";
const PROJECT: &str = "bp";

/// What follows a skill file's frontmatter.
pub fn body_of(skill_md: &str) -> &str {
    skill_md.strip_prefix("---\n").and_then(|t| t.split_once("\n---\n")).map_or(skill_md, |(_, body)| body)
}

/// Lines of [`FILLER`] cut to `len` bytes: the body of the `filler` arm is as long as the real one.
fn filler(len: usize) -> String {
    let mut text = format!("{FILLER}\n").repeat(len / (FILLER.len() + 1) + 1);
    text.truncate(len);
    text
}

/// The role file without the skill in its `skills:` line; the line goes when no skill is left.
fn unpreload(role: &str, skill: &str) -> String {
    let mut out = Vec::new();
    for line in role.lines() {
        let Some(list) = line.strip_prefix("skills:") else {
            out.push(line.to_string());
            continue;
        };
        let left: Vec<&str> = list.split(',').map(str::trim).filter(|s| *s != skill).collect();
        if !left.is_empty() {
            out.push(format!("skills: {}", left.join(", ")));
        }
    }
    out.join("\n") + "\n"
}

/// A copy of the bundle at `dest` with the arm's edit: no skill and no preload, or the skill's body as filler.
fn arm_bundle(root: &Path, skill: &Skill, arm: &str, dest: &Path) -> Result<()> {
    capture(Command::new("cp").arg("-R").arg(root.join("bundle")).arg(dest))?;
    let dir = dest.join("skills").join(&skill.name);
    if arm == "without" {
        std::fs::remove_dir_all(&dir)?;
        let role = dest.join(format!("roles/{}.md", skill.role_file()));
        std::fs::write(&role, unpreload(&std::fs::read_to_string(&role)?, &skill.name))?;
        // Install refuses a template that names a skill its role does not preload, so the arm's dispatch names none.
        let template = dest.join(format!("roles/{}/{}.md", skill.role_file(), skill.action));
        let text = std::fs::read_to_string(&template)?;
        let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with("Skill:")).collect();
        std::fs::write(&template, kept.join("\n") + "\n")?;
    } else {
        let file = dir.join("SKILL.md");
        let text = std::fs::read_to_string(&file)?;
        let head = text.len() - body_of(&text).len();
        std::fs::write(&file, format!("{}{}", &text[..head], filler(text.len() - head)))?;
    }
    Ok(())
}

/// One directory per arm, each holding a `bp`. `given` names binaries the caller built; the others build here, the
/// edited bundles first and the shipped bundle last, so `target/release/bp` ends as the shipped bundle's even when
/// only an edited arm runs.
pub fn bins(
    root: &Path,
    skill: &Skill,
    arms: &[String],
    work: &Path,
    given: &[(String, PathBuf)],
) -> Result<BTreeMap<String, PathBuf>> {
    let mut out = BTreeMap::new();
    let mut order: Vec<String> = arms.to_vec();
    let edited = |arm: &String| arm != "with" && !given.iter().any(|(a, _)| a == arm);
    if order.iter().any(edited) && !order.contains(&"with".to_string()) {
        order.push("with".into());
    }
    order.sort_by_key(|a| a == "with");
    for arm in order {
        let dir = work.join(format!("bin-{arm}"));
        std::fs::create_dir_all(&dir)?;
        if let Some((_, path)) = given.iter().find(|(a, _)| *a == arm) {
            std::fs::copy(path, dir.join("bp")).with_context(|| format!("copy {}", path.display()))?;
        } else {
            let bundle = if arm == "with" {
                None
            } else {
                let b = work.join(format!("bundle-{arm}"));
                arm_bundle(root, skill, &arm, &b)?;
                Some(b.canonicalize()?)
            };
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let mut build = Command::new(cargo);
            build.args(["build", "--release", "-p", "bp-tool"]).current_dir(root).stdin(Stdio::null());
            match &bundle {
                Some(b) => build.env("BP_BUNDLE_DIR", b),
                None => build.env_remove("BP_BUNDLE_DIR"),
            };
            if !build.status().context("run cargo build")?.success() {
                bail!("cargo build for the {arm} arm failed");
            }
            let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
            std::fs::copy(target.join("release/bp"), dir.join("bp")).context("copy the built bp")?;
        }
        out.insert(arm, dir);
    }
    Ok(out)
}

/// The dispatch prompt: the role's template for the action, from the arm's `bundle`, with the node filled in, under
/// the line the owner's replay typed to a session so that it dispatches the role and adds nothing (27, the replay).
pub fn prompt(bundle: &Path, skill: &Skill, id: &str) -> Result<String> {
    let file = bundle.join(format!("roles/{}/{}.md", skill.role_file(), skill.action));
    let template = std::fs::read_to_string(&file).with_context(|| format!("read {}", file.display()))?;
    Ok(format!(
        "Dispatch the {} agent with exactly this prompt and add nothing to it:\n\n{}",
        skill.role_file(),
        template.replace("<id>", id)
    ))
}

pub struct Sandbox {
    pub vault: PathBuf,
    pub forge: PathBuf,
    pub issue: String,
    bindir: PathBuf,
    /// The vault's head when the agent starts: the log is what the agent and the session add after it.
    start: String,
}

impl Sandbox {
    /// Builds the sandbox of one trial of a review: the pull request `fx` records is open on the fake forge and its
    /// issue is claimed, so `walk --for review` reads `needs-review`. `repo` holds the base and the head.
    pub fn build(repo: &Path, source: &Path, bindir: &Path, work: &Path, fx: &Fixture) -> Result<Self> {
        let exe = std::env::current_exe().context("find the xtask binary")?;
        let made = |verb: &[&str], vault: &Path| {
            capture(Command::new(&exe).args(verb).env("BP_VAULT", vault).current_dir(work).stdin(Stdio::null()))
        };
        let vault = PathBuf::from(made(&["scratch"], source)?);
        let forge = PathBuf::from(made(&["forge", PROJECT], &vault)?);
        let mut sb = Self { vault, forge, issue: String::new(), bindir: bindir.to_path_buf(), start: String::new() };
        let code = sb.code();
        let git = |dir: &Path, args: &[&str]| {
            capture(Command::new("git").args(args).current_dir(dir).env("GIT_ALLOW_PROTOCOL", "file"))
        };
        let repo_arg = repo.display().to_string();
        git(&code, &["fetch", "-q", &repo_arg, &fx.base, &fx.head])?;
        git(&code, &["reset", "-q", "--hard", &fx.base])?;
        git(&code, &["push", "-q", "-f", "origin", "HEAD:master"])?;

        let body = |name: &str, text: &str| -> Result<String> {
            let file = sb.forge.join(name);
            std::fs::write(&file, text)?;
            Ok(file.display().to_string())
        };
        let milestone = body("milestone.body", "## Goal\n\nA sandbox milestone for an eval trial.\n")?;
        let issue = body("issue.body", &format!("{}\n", fx.issue))?;
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
                &milestone,
            ],
        )?;
        sb.bp(&code, None, &["transition", m, "approved"])?;
        let made = sb.bp(
            &code,
            None,
            &["create", "issue", "--project", PROJECT, "--title", &fx.title, "--under", m, "--body-file", &issue],
        )?;
        sb.issue = made["id"].as_str().context("create issue returned no id")?.to_string();
        sb.bp(&code, None, &["link", &sb.issue.clone(), "cites", DESIGN])?;

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

        let claimed = sb.bp(&code, Some("worker"), &["cut", &sb.issue.clone()])?;
        let worktree = PathBuf::from(claimed["worktree"].as_str().context("cut returned no worktree")?);
        git(&worktree, &["merge", "-q", "--ff-only", &fx.head])?;
        sb.bp(&worktree, Some("worker"), &["pr", &sb.issue.clone()])?;
        sb.start = git(&sb.vault, &["rev-parse", "HEAD"])?.trim().to_string();
        Ok(sb)
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
            .env_remove("BP_ROLE")
            .env_remove("BP_SESSION")
            .env_remove("BP_NOW")
            .stdin(Stdio::null());
        c
    }

    fn bp(&self, dir: &Path, role: Option<&str>, args: &[&str]) -> Result<Value> {
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
    /// agent's result; the caller reads whatever transcript exists.
    pub fn run_agent(&self, command: &str, prompt: &str, arm: &str, fixture: &str, log: &Path) -> Result<PathBuf> {
        let file = self.forge.join("prompt.txt");
        std::fs::write(&file, prompt)?;
        let transcript = self.forge.join("transcript.jsonl");
        let out = std::fs::File::create(log)?;
        let mut c = self.command("sh", &self.code());
        let status = c
            .args(["-c", command])
            .env("BP_EVAL_PROMPT", &file)
            .env("BP_EVAL_TRANSCRIPT", &transcript)
            .env("BP_EVAL_ARM", arm)
            .env("BP_EVAL_FIXTURE", fixture)
            .stdout(out.try_clone()?)
            .stderr(out)
            .status()
            .context("run the agent command")?;
        if !status.success() {
            eprintln!("eval: the agent command exited {status}; its output is in {}", log.display());
        }
        Ok(transcript)
    }
}

/// The newest subagent transcript Claude Code wrote for a session started in a directory whose name holds `name`:
/// `<config>/projects/<encoded cwd>/<session>/subagents/agent-*.jsonl`, as the owner's replay found it.
pub fn claude_transcript(config: &Path, name: &str) -> Option<PathBuf> {
    let dirs = |p: &Path| std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    let mut found = Vec::new();
    for project in dirs(&config.join("projects")).into_iter().filter(|p| p.to_string_lossy().contains(name)) {
        for session in dirs(&project) {
            for file in dirs(&session.join("subagents")) {
                let agent = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("agent-") && Path::new(n).extension().is_some_and(|e| e == "jsonl"));
                if let (true, Ok(m)) = (agent, file.metadata().and_then(|m| m.modified())) {
                    found.push((m, file));
                }
            }
        }
    }
    found.into_iter().max().map(|(_, f)| f)
}

/// Runs a command and returns its stdout. A failure carries stderr, and stdout when it holds an envelope.
fn capture(cmd: &mut Command) -> Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filler_is_as_long_as_the_body_and_holds_no_instruction() {
        for len in [0, 5, 32, 33, 1000] {
            let f = filler(len);
            assert_eq!(f.len(), len);
            assert!(f.lines().all(|l| FILLER.starts_with(l)));
        }
        assert_eq!(body_of("---\nname: x\n---\nTask: a\n"), "Task: a\n");
        assert_eq!(body_of("no frontmatter"), "no frontmatter");
    }

    #[test]
    fn the_preload_goes_and_the_other_skills_stay() {
        let role = "---\nname: r\nskills: bp-complete, bp-respond\n---\nbody\n";
        assert_eq!(unpreload(role, "bp-complete"), "---\nname: r\nskills: bp-respond\n---\nbody\n");
        assert_eq!(unpreload("---\nskills: bp-review\n---\nx\n", "bp-review"), "---\n---\nx\n");
    }

    #[test]
    fn finds_the_newest_subagent_transcript_of_the_session() {
        let cfg = tempfile::tempdir().unwrap();
        let sub = cfg.path().join("projects/-tmp-bp-forge-AbCd1234-code/sess/subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("agent-1.jsonl"), "{}").unwrap();
        let newer = sub.join("agent-2.jsonl");
        std::fs::write(&newer, "{}").unwrap();
        std::fs::File::open(&newer)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        std::fs::create_dir_all(cfg.path().join("projects/-tmp-bp-forge-Other999-code/s/subagents")).unwrap();
        assert_eq!(claude_transcript(cfg.path(), "AbCd1234"), Some(newer));
        assert_eq!(claude_transcript(cfg.path(), "Nothing"), None);
    }
}
