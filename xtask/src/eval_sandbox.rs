//! The sandbox of one trial (29, design change 1). It is the replay's: a clone at the fixture's recorded base wired to
//! the fake forge, a scratch copy of the vault, the bundle installed into the clone with every config home pointed at
//! a scratch directory, and the claim state and stop point of the stored run. Review and respond start at an open
//! pull request at the fixture's head (respond adds the recorded review); complete starts at an open issue with no
//! claim, and with a dependency's open pull request when the fixture is stacked. A mutant fixture's pull request is its
//! head with the mutant planted in it (30, design change 1).

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
    /// This trial's own temporary directory, which holds its vault and forge. The agent's `$TMPDIR` is its `agent`
    /// child: `cargo xtask scratch`, `forge` and `sandbox` prune `$TMPDIR/bp-*` by age and count, a worker's blocks
    /// call them on every `check` and `pr`, and a reviewer's probe runs them too, so a shared directory loses a live
    /// sandbox.
    tmp: PathBuf,
    /// One cargo target directory for every trial: the issue's block builds `xtask` in each clone. `$BP_EVAL_TARGET`
    /// keeps it across runs, so a smoke trial warms the build of the full run. Its binaries are the last head's until
    /// [`Sandbox::open_pr`] builds the fixture's own.
    target: PathBuf,
    /// The vault's head when the agent starts: the log is what the agent and the session add after it.
    start: String,
    /// What the worker's branch is measured against: the clone's base, or the dependency's head when stacked.
    pub scope_base: String,
    /// The head of the fixture's pull request: the fixture's own, or the planted one of a mutant.
    pub head: String,
}

impl Sandbox {
    /// Builds the sandbox of one trial. For a review the pull request `fx` records is open on the fake forge and its
    /// issue is claimed, so `walk --for review` reads `needs-review`; a respond adds the review, so it reads
    /// `needs-response`; a complete leaves the issue open and unclaimed. `repo` holds the base and the head.
    ///
    /// `check_arm` asserts, before the agent costs anything, that the clone holds the arm: the skill and the role's
    /// `skills:` line as shipped for `with` and `filler`, neither for `without`, and for an arm from a git ref (the
    /// sha after the arm) that commit's skill body. A skill placed wrong reads as zero recall (RA 5.5, 6). `work`
    /// holds the arm binaries and the cargo target directory that the sandbox shares.
    pub fn build(
        repo: &Path,
        source: &Path,
        bindir: &Path,
        work: &Path,
        fx: &Fixture,
        skill: &Skill,
        check_arm: Option<(&str, Option<&str>)>,
    ) -> Result<Self> {
        let exe = std::env::current_exe().context("find the xtask binary")?;
        let tmp = crate::scratch::fresh(&work.join("tmp"), 3)?.canonicalize()?;
        // The agent's `TMPDIR` is a child of `tmp`, apart from the vault and the forge that `tmp` itself holds: a
        // probe that runs the project's own `cargo xtask sandbox` or its tests there fills `$TMPDIR/bp-forge` past
        // `KEEP` and prunes the live checkout (the development run, `with-control-58-1`, 2026-10-26).
        std::fs::create_dir_all(tmp.join("agent"))?;
        let made = |verb: &[&str], vault: &Path| {
            let mut c = Command::new(&exe);
            capture(c.args(verb).env("BP_VAULT", vault).env("TMPDIR", &tmp).current_dir(work).stdin(Stdio::null()))
        };
        let vault = PathBuf::from(made(&["scratch"], source)?);
        let forge = PathBuf::from(made(&["forge", PROJECT], &vault)?);
        let mut sb = Self {
            vault,
            forge,
            issue: String::new(),
            bindir: bindir.to_path_buf(),
            tmp: tmp.clone(),
            target: std::env::var_os("BP_EVAL_TARGET").map_or_else(|| work.join("target"), PathBuf::from),
            start: String::new(),
            scope_base: fx.stack.as_ref().map_or(&fx.base, |s| &s.0).clone(),
            head: fx.head.clone(),
        };
        let code = sb.code();
        let repo_arg = repo.display().to_string();
        // The landed head is the answer to a complete trial, so its objects stay out of that clone.
        let mut fetch = vec!["fetch", "-q", repo_arg.as_str(), &fx.base];
        fetch.extend((skill.action != Action::Complete).then_some(fx.head.as_str()));
        fetch.extend(fx.stack.iter().map(|s| s.0.as_str()));
        git(&code, &fetch)?;
        let top =
            if skill.action == Action::Complete { fx.stack.as_ref().map_or(&fx.base, |s| &s.0) } else { &fx.head };
        let named: Vec<String> = named_commits(&code, top)?.into_iter().filter(|c| *c != fx.head).collect();
        if !named.is_empty() {
            let refs: Vec<&str> = named.iter().map(String::as_str).collect();
            git(&code, &[&["fetch", "-q", repo_arg.as_str()][..], &refs].concat())?;
        }
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
        // A rule the fixture's truth rests on has to be in the reviewer's walk, and the walk carries the body of a
        // design only (21, the context column): the rule is a component of the system design, cited by the issue.
        for (title, text) in &fx.rules {
            let file = sb.forge.join("rule.body");
            std::fs::write(&file, format!("{text}\n"))?;
            let slug = format!("components/{}", title.to_lowercase().replace(' ', "-"));
            let args = ["create", "design", "--project", PROJECT, "--under", DESIGN, "--slug", &slug, "--title", title];
            let made = sb.bp(&code, None, &[&args[..], &["--body-file", &file.display().to_string()]].concat())?;
            let id = made["id"].as_str().context("create design returned no id")?.to_string();
            sb.bp(&code, None, &["link", &sb.issue.clone(), "cites", &id])?;
        }

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

        if let Some((arm, at)) = check_arm {
            sb.assert_arm(skill, arm, at)?;
        }
        if let Some((head, title)) = &fx.stack {
            // The dependency holds an open pull request at its landed head and a check that needs no build, so the
            // worker's `cut` stacks on it (21, the complete row) and the trial pays for only its own issue.
            let stub = "## Check\n\n~~~check shell {name=stub expect=0}\ntrue\n~~~\n";
            let dependency = sb.create_issue(&code, m, title, stub)?;
            sb.open_pr(&code, &dependency, head, None)?;
            sb.bp(&code, None, &["link", &sb.issue.clone(), "depends", &dependency])?;
        }
        match skill.action {
            Action::Complete => {}
            Action::Respond => {
                sb.open_pr(&code, &sb.issue.clone(), &fx.head, None)?;
                if !sb.record_review(&fx.head, &fx.review_body())? {
                    bail!("the vault refused the recorded review of {}", fx.id);
                }
            }
            _ => {
                sb.head =
                    sb.open_pr(&code, &sb.issue.clone(), &fx.head, fx.mutant.as_ref().map(|m| m.diff.as_str()))?;
            }
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

    /// The worker's cut, the fixture's commits, the mutant planted when there is one, and `pr`: a pull request that the
    /// claim holds. Returns its head.
    fn open_pr(&self, code: &Path, issue: &str, head: &str, mutant: Option<&str>) -> Result<String> {
        let claimed = self.bp(code, Some("worker"), &["cut", issue])?;
        let worktree = PathBuf::from(claimed["worktree"].as_str().context("cut returned no worktree")?);
        git(&worktree, &["merge", "-q", "--ff-only", head])?;
        if let Some(diff) = mutant {
            let file = self.forge.join("mutant.diff");
            std::fs::write(&file, diff)?;
            plant(&worktree, &file)?;
        }
        // The cargo target is shared, so `target/debug/bp` is whatever the last head built, and an issue's own check
        // reads it (`xtask/tests/eval.rs` at #57 takes the `bp` beside the xtask test binary; `with-escape-11-19-20-1`
        // read the `bp` that #61's check had built and failed, 2026-10-26). Build this head's binaries before its
        // checks run. A fixture with no cargo workspace (the fake repositories of the tests) has no cargo check to
        // mislead.
        if worktree.join("Cargo.toml").exists() {
            capture(self.command("cargo", &worktree).args(["build", "-q", "--workspace", "--bins"]))?;
        }
        self.bp(&worktree, Some("worker"), &["pr", issue])?;
        git(&worktree, &["rev-parse", "HEAD"])
    }

    /// The installed clone holds what the arm says (see [`Sandbox::build`]).
    fn assert_arm(&self, skill: &Skill, arm: &str, at: Option<&str>) -> Result<()> {
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
        if let Some(sha) = at.filter(|sha| !holds_ref_body(&body, Path::new("."), sha, skill).unwrap_or(false)) {
            bail!(
                "the {arm} arm's clone does not hold the body of {sha}: {} differs; rebuild the arm",
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
            .env("TMPDIR", self.tmp.join("agent"))
            .env("CARGO_TARGET_DIR", &self.target)
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_BUILD_JOBS", std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "4".into()))
            .env("CARGO_NET_OFFLINE", "true")
            .env_remove("BP_REDESIGN")
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
        self.stop_leftovers();
        Ok((transcript, failure))
    }

    /// Kills any process whose working directory is under the sandbox: a `cargo` or `rustc` that outlived its shell
    /// (the group kill reaches only the shell's own group) would overlap the next trial and fill the disk. `lsof`
    /// lists the working directory of every process; a missing `lsof` leaves them alone.
    fn stop_leftovers(&self) {
        let Ok(out) = Command::new("lsof").args(["-d", "cwd", "-F", "pn"]).output() else { return };
        let (mut pid, mut stale) = (String::new(), Vec::new());
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some(p) = line.strip_prefix('p') {
                pid = p.to_string();
            } else if line.strip_prefix('n').is_some_and(|dir| Path::new(dir).starts_with(&self.forge)) {
                stale.push(pid.clone());
            }
        }
        for pid in stale {
            let _ = Command::new("kill").args(["-s", "KILL", &pid]).status();
        }
    }
}

/// The commits that the fixtures under `xtask/evals/` at `at` name on their `base:`, `head:` and `stack:` lines. From
/// #60 on, xtask's `the_shipped_fixtures_match_the_history_they_name` asserts that they are in the clone, and an issue's
/// Check that runs xtask's tests runs it. A fixture's head can be a branch commit that the fetched heads do not reach,
/// as `pr-20`'s is, so the clone fetches them too (review of PR 63, major 1).
fn named_commits(code: &Path, at: &str) -> Result<Vec<String>> {
    let files = git(code, &["ls-tree", "-r", "--name-only", at, "--", "xtask/evals"])?;
    let mut shas = std::collections::BTreeSet::new();
    for file in files.lines().filter(|f| Path::new(f).extension().is_some_and(|e| e == "md")) {
        let text = git(code, &["show", &format!("{at}:{file}")])?;
        for line in text.split("\n---\n").next().unwrap_or_default().lines() {
            let value = ["base:", "head:", "stack:"].iter().find_map(|k| line.strip_prefix(k));
            shas.extend(value.and_then(|v| v.split_whitespace().next()).map(str::to_string));
        }
    }
    Ok(shas.into_iter().collect())
}

/// Applies a mutant's diff to the head commit in `dir` and amends that commit, message and author kept. The pull
/// request is then one commit, as the landed one was, so its history does not point at the defect. The committer is
/// `bp-eval`, because the sandbox may have no git identity.
fn plant(dir: &Path, diff: &Path) -> Result<()> {
    git(dir, &["apply", &diff.display().to_string()])?;
    let me = ["-c", "user.name=bp-eval", "-c", "user.email=bp-eval@bp.invalid"];
    git(dir, &[&me[..], &["commit", "-q", "--no-verify", "-a", "--amend", "--no-edit"]].concat())?;
    Ok(())
}

/// Seeds a research fixture's nodes into the scratch vault at `vault` (31, design change 2): it writes each seed at its
/// node id, commits them, and has `bp` at `bp` show each one. There is no `bp reindex`: the index refreshes when a
/// file comes or the head moves, so the `show` rebuilds it and refuses a seed the vault does not take. A runner calls
/// it once per trial, on a fresh copy; `cargo xtask plants --verify` calls it once for a whole set.
pub(crate) fn seed(vault: &Path, bp: &Path, fixtures: &[&Fixture]) -> Result<()> {
    let seeds = fixtures.iter().flat_map(|f| f.research.iter().flat_map(|r| &r.plants));
    let mut ids = Vec::new();
    for s in seeds {
        let file = vault.join(s.file());
        std::fs::create_dir_all(file.parent().context("a node id has a parent")?)?;
        std::fs::write(&file, &s.text)?;
        ids.push(s.node.as_str());
    }
    let me = ["-c", "user.name=bp-eval", "-c", "user.email=bp-eval@bp.invalid"];
    git(vault, &["add", "-A"])?;
    git(vault, &[&me[..], &["commit", "-q", "--no-verify", "-m", "seed the research fixtures"]].concat())?;
    for id in ids {
        let mut show = Command::new(bp);
        show.args(["show", id, "--json"]).env("BP_VAULT", vault).env_remove("BP_ROLE").env_remove("BP_SESSION");
        capture(show.current_dir(vault).stdin(Stdio::null()))
            .with_context(|| format!("the scratch vault does not take the seed {id}"))?;
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    capture(Command::new("git").args(args).current_dir(dir).env("GIT_ALLOW_PROTOCOL", "file"))
}

/// Whether `installed` is the skill's body at the commit `sha` of the repository at `root`, byte for byte: what an
/// arm built from that commit must have installed (30, design change 2).
fn holds_ref_body(installed: &str, root: &Path, sha: &str, skill: &Skill) -> Result<bool> {
    Ok(installed == crate::eval_grade::show_at(root, sha, &format!("bundle/skills/{}/SKILL.md", skill.name))?)
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

#[cfg(test)]
mod tests {
    #[test]
    fn the_agent_gets_a_tmpdir_apart_from_the_vault_and_forge_and_no_redesign() {
        let t = tempfile::tempdir().unwrap();
        let sb = super::Sandbox {
            vault: t.path().join("v"),
            forge: t.path().join("f"),
            issue: String::new(),
            bindir: t.path().join("bin"),
            tmp: t.path().to_path_buf(),
            target: t.path().join("target"),
            start: String::new(),
            scope_base: String::new(),
            head: String::new(),
        };
        let c = sb.command("true", t.path());
        let env: std::collections::BTreeMap<_, _> = c.get_envs().collect();
        let var = |k: &str| env[std::ffi::OsStr::new(k)];
        assert_eq!(var("TMPDIR"), Some(t.path().join("agent").as_os_str()), "the agent's TMPDIR is tmp/agent");
        assert_eq!(var("BP_REDESIGN"), None, "a nested eval in a probe must not find the store");
    }

    use super::*;

    #[test]
    fn an_arm_from_a_ref_holds_that_refs_body() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let skill = Skill::parse("bp-review").unwrap();
        let Ok(old) = crate::eval_grade::show_at(&root, "fc51d9b", "bundle/skills/bp-review/SKILL.md") else {
            eprintln!("skipped: history is shallow");
            return;
        };
        assert!(holds_ref_body(&old, &root, "fc51d9b", &skill).unwrap());
        let shipped = std::fs::read_to_string(root.join("bundle/skills/bp-review/SKILL.md")).unwrap();
        assert!(old != shipped && !holds_ref_body(&shipped, &root, "fc51d9b", &skill).unwrap(), "a stale build");
        assert!(holds_ref_body(&old, &root, "0000000", &skill).is_err(), "no such commit");
    }

    #[test]
    fn a_planted_head_differs_from_the_landed_head_by_the_mutant_alone() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("r");
        let g = |args: &[&str]| {
            let me = ["-c", "user.name=t", "-c", "user.email=t@t.invalid"];
            git(&repo, &[&me[..], args].concat()).unwrap()
        };
        std::fs::create_dir(&repo).unwrap();
        g(&["init", "-q"]);
        std::fs::write(repo.join("a.rs"), "fn a() -> bool {\n    1 > 0\n}\n").unwrap();
        g(&["add", "-A"]);
        g(&["commit", "-qm", "base"]);
        let base = g(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.rs"), "fn a() -> bool {\n    2 > 0\n}\n\nfn b() {}\n").unwrap();
        g(&["commit", "-qam", "the landed change (#9)"]);
        let head = g(&["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.rs"), "fn a() -> bool {\n    2 >= 0\n}\n\nfn b() {}\n").unwrap();
        let diff = format!("{}\n", g(&["diff"]));
        g(&["checkout", "-q", "."]);
        let file = dir.path().join("mutant.diff");
        std::fs::write(&file, &diff).unwrap();

        plant(&repo, &file).unwrap();
        assert_eq!(format!("{}\n", g(&["diff", &head, "HEAD"])), diff, "the mutant and nothing else");
        assert_eq!(g(&["rev-parse", "HEAD^"]), base, "one commit on the base, as landed");
        assert_eq!(g(&["log", "-1", "--format=%s %an"]), "the landed change (#9) t", "message and author kept");
        assert!(g(&["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn the_clone_fetches_the_commits_its_own_fixtures_name() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, clone) = (dir.path().join("r"), dir.path().join("c"));
        let g = |at: &Path, args: &[&str]| {
            let me = ["-c", "user.name=t", "-c", "user.email=t@t.invalid"];
            git(at, &[&me[..], args].concat()).unwrap()
        };
        std::fs::create_dir(&repo).unwrap();
        g(&repo, &["init", "-q", "-b", "master"]);
        std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
        g(&repo, &["add", "-A"]);
        g(&repo, &["commit", "-qm", "base"]);
        let base = g(&repo, &["rev-parse", "HEAD"]);
        // A branch commit that master never reaches, as `pr-20`'s head is.
        g(&repo, &["checkout", "-q", "-b", "side"]);
        std::fs::write(repo.join("b.rs"), "fn b() {}\n").unwrap();
        g(&repo, &["add", "-A"]);
        g(&repo, &["commit", "-qm", "side"]);
        let side = g(&repo, &["rev-parse", "HEAD"]);
        g(&repo, &["checkout", "-q", "master"]);
        g(&repo, &["branch", "-q", "-D", "side"]);
        let fixture = format!(
            "kind: mutant\nbase: {base}\nhead: {side}\nmutant_diff: |\n    head: {base}x\ntruth:\n---\nhead: no\n"
        );
        std::fs::create_dir_all(repo.join("xtask/evals/bp-review")).unwrap();
        std::fs::write(repo.join("xtask/evals/bp-review/pr-1.md"), fixture).unwrap();
        g(&repo, &["add", "-A"]);
        g(&repo, &["commit", "-qm", "head"]);
        let head = g(&repo, &["rev-parse", "HEAD"]);

        std::fs::create_dir(&clone).unwrap();
        g(&clone, &["init", "-q"]);
        g(&clone, &["fetch", "-q", &repo.display().to_string(), &head]);
        let named = named_commits(&clone, &head).unwrap();
        assert_eq!(
            named,
            {
                let mut v = vec![base.clone(), side.clone()];
                v.sort();
                v
            },
            "the keys only, not the diff or the body"
        );
        assert!(
            git(&clone, &["cat-file", "-e", &format!("{side}^{{commit}}")]).is_err(),
            "the head's fetch leaves it out"
        );
        let refs: Vec<&str> = named.iter().map(String::as_str).collect();
        g(&clone, &[&["fetch", "-q", &repo.display().to_string()][..], &refs].concat());
        assert!(git(&clone, &["cat-file", "-e", &format!("{side}^{{commit}}")]).is_ok(), "an unreachable sha fetches");
    }
}
