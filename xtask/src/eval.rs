//! `cargo xtask eval`, the skill eval (29, design change 1; 17 check 6). Blueprint never runs an agent, so the runner
//! lives here, and the owner starts it in their own terminal.
//!
//! ```text
//! cargo xtask eval <skill> --trials N [--arms with,without[,filler]] [--fixture <id>]
//! cargo xtask eval --grade [--check] <skill>
//! ```
//!
//! A run makes N trials per arm and per fixture. A trial builds the replay's sandbox, starts the agent in the clone,
//! and stores the role's transcript and the vault log under `$BP_REDESIGN/analysis/evals-skills/<skill>/`. Then it
//! grades the store, as `--grade` does. The agent is `$BP_EVAL_AGENT`, a shell command that reads the prompt from
//! `$BP_EVAL_PROMPT`; if it writes the role's transcript to `$BP_EVAL_TRANSCRIPT`, that file is the trace, and if it
//! does not, the newest subagent transcript that Claude Code wrote for the sandbox is. `$BP_VAULT` is the vault to
//! copy. Flags for a test: `--repo <dir>` (holds the fixtures' commits), `--fixtures <dir>`, `--bin <arm>=<bp>`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bp_operation::{Action, READ_VERBS, Role};

use crate::eval_fixture::{Fixture, Review};
use crate::eval_grade::{Grade, Owner, differences};
use crate::eval_sandbox::{Sandbox, bins, claude_transcript, prompt};
use crate::eval_trace::Trace;

/// The agent the owner's replay used, headless: a session that dispatches the role. The tools are allowed by name,
/// not by skipping permissions, because the role's shell can reach the rest of the machine.
const AGENT: &str = r#"claude -p "$(cat "$BP_EVAL_PROMPT")" --setting-sources project,local --allowedTools "Bash,Read,Grep,Glob,Edit,Write,Task,Agent""#;

#[derive(Clone)]
pub struct Skill {
    pub name: String,
    pub action: Action,
    role: Role,
}

impl Skill {
    fn parse(name: &str) -> Result<Self> {
        let action: Action =
            name.strip_prefix("bp-").unwrap_or(name).parse().with_context(|| format!("{name} is no skill"))?;
        // The reviewer returns the review and the session records it (21), so no action belongs to the reviewer role.
        let role = if action == Action::Review { Role::Reviewer } else { action.role() };
        if action != Action::Review {
            bail!("{name}: no fixture or grader yet; issue 6 of milestone 6 adds complete and respond");
        }
        Ok(Self { name: format!("bp-{action}"), action, role })
    }

    pub fn role_file(&self) -> String {
        format!("bp-{}", self.role)
    }

    /// The verbs the dispatched role may run for this action: the read verbs and the action's row, or the role's own
    /// row when the action belongs to another role (the reviewer's row is empty).
    pub fn allowed(&self) -> Vec<&'static str> {
        let row = if self.action.role() == self.role {
            self.action.verbs().to_vec()
        } else {
            self.role.verbs().unwrap_or_default()
        };
        READ_VERBS.iter().copied().chain(row).collect()
    }
}

struct Args {
    skill: String,
    trials: usize,
    arms: Vec<String>,
    fixture: Option<String>,
    fixtures: Option<PathBuf>,
    repo: PathBuf,
    bins: Vec<(String, PathBuf)>,
    grade: bool,
    check: bool,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self> {
        let mut a = Self {
            skill: String::new(),
            trials: 0,
            arms: vec!["with".into(), "without".into()],
            fixture: None,
            fixtures: None,
            repo: ".".into(),
            bins: Vec::new(),
            grade: false,
            check: false,
        };
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().with_context(|| format!("{arg} takes a value"));
            match arg.as_str() {
                "--grade" => a.grade = true,
                "--check" => a.check = true,
                "--trials" => a.trials = value()?.parse().context("--trials is a count")?,
                "--arms" => a.arms = value()?.split(',').map(str::to_string).collect(),
                "--fixture" => a.fixture = Some(value()?.clone()),
                "--fixtures" => a.fixtures = Some(value()?.into()),
                "--repo" => a.repo = value()?.into(),
                "--bin" => {
                    let (arm, path) = value()?.split_once('=').context("--bin is <arm>=<path>")?;
                    a.bins.push((arm.into(), path.into()));
                }
                s if !s.starts_with('-') && a.skill.is_empty() => a.skill = s.into(),
                s => bail!("unknown argument {s}"),
            }
        }
        if a.skill.is_empty() || (!a.grade && a.trials == 0) || (a.check && !a.grade) {
            bail!(
                "usage: cargo xtask eval <skill> --trials N [--arms ...] [--fixture <id>] | cargo xtask eval --grade [--check] <skill>"
            );
        }
        if let Some(bad) = a.arms.iter().find(|x| !["with", "without", "filler"].contains(&x.as_str())) {
            bail!("arm {bad} is none of with, without, filler");
        }
        Ok(a)
    }
}

/// Runs the verb. Violations are the lines `--grade --check` prints; a run or a regrade has none.
pub fn run(args: &[String]) -> Result<Vec<String>> {
    let a = Args::parse(args)?;
    let skill = Skill::parse(&a.skill)?;
    let root = Path::new(".");
    let redesign = std::env::var_os("BP_REDESIGN").context("set BP_REDESIGN to the designs repository")?;
    let results = Path::new(&redesign).join("analysis/evals-skills");
    let store = results.join(&skill.name);
    let fixtures = a.fixtures.clone().unwrap_or_else(|| root.join("xtask/evals").join(&skill.name));
    if !a.grade {
        trials(&a, &skill, root, &fixtures, &store)?;
    }
    let grade = Grade::read(&skill, &store, &fixtures, root)?;
    let file = results.join(format!("{}.md", skill.name));
    let existing = std::fs::read_to_string(&file).ok();
    let text = grade.result(&Owner::of(existing.as_deref())?);
    if a.check {
        let Some(have) = existing else {
            return Ok(vec![format!("{} does not exist; run cargo xtask eval --grade {}", file.display(), skill.name)]);
        };
        return Ok(differences(&text, &have));
    }
    std::fs::write(&file, text)?;
    std::fs::write(store.join("report.md"), grade.report(&store))?;
    println!("{}\n{}", file.display(), store.join("report.md").display());
    Ok(Vec::new())
}

/// Stores `a.trials` new trials per arm and per fixture, the arms interleaved so that host drift falls on both.
fn trials(a: &Args, skill: &Skill, root: &Path, fixtures: &Path, store: &Path) -> Result<()> {
    let source = std::env::var_os("BP_VAULT").context("set BP_VAULT to the vault to copy")?;
    let agent = std::env::var("BP_EVAL_AGENT").unwrap_or_else(|_| AGENT.into());
    let list = Fixture::load(fixtures, a.fixture.as_deref())?;
    std::fs::create_dir_all(store)?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-eval"), 8)?;
    let bin = bins(root, skill, &a.arms, &work, &a.bins)?;
    let repo = a.repo.canonicalize().with_context(|| format!("read {}", a.repo.display()))?;
    let next = |arm: &str, fx: &str| {
        let taken = std::fs::read_dir(store).into_iter().flatten().flatten();
        taken
            .filter(|e| {
                e.file_name().to_string_lossy().starts_with(&format!("{arm}-{fx}-"))
                    && e.path().extension().is_some_and(|x| x == "jsonl")
            })
            .count()
            + 1
    };
    for _ in 0..a.trials {
        for arm in &a.arms {
            for fx in &list {
                let id = format!("{arm}-{}-{}", fx.id, next(arm, &fx.id));
                eprintln!("eval: trial {id}");
                let sb = Sandbox::build(&repo, Path::new(&source), &bin[arm], &work, fx)
                    .with_context(|| format!("build the sandbox of {id}"))?;
                // An arm built from an edited bundle dispatches from that bundle's template.
                let edited = work.join(format!("bundle-{arm}"));
                let bundle = if edited.is_dir() { edited } else { root.join("bundle") };
                let made = sb.run_agent(
                    &agent,
                    &prompt(&bundle, skill, &sb.issue)?,
                    arm,
                    &fx.id,
                    &store.join(format!("{id}.agent.log")),
                )?;
                let found =
                    if made.is_file() { Some(made) } else { claude_transcript(&claude_config(), &dir_name(&sb.forge)) };
                let found = found.with_context(|| {
                    format!("{id}: the agent left no transcript; see {}.agent.log", store.join(&id).display())
                })?;
                let trace = Trace::parse(&std::fs::read_to_string(&found)?);
                if let Some(review) = Review::parse(&trace.final_text()) {
                    sb.record_review(&fx.head, &review.body)?;
                }
                std::fs::write(store.join(format!("{id}.vault.log")), sb.vault_log()?)?;
                // The transcript lands last: a trial is stored once it has one, so a failed run leaves no half trial.
                std::fs::copy(&found, store.join(format!("{id}.jsonl")))?;
            }
        }
    }
    Ok(())
}

fn claude_config() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map_or_else(|| Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".claude"), PathBuf::from)
}

fn dir_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reviewer_may_read_and_nothing_else() {
        let s = Skill::parse("bp-review").unwrap();
        assert_eq!((s.role_file().as_str(), s.allowed()), ("bp-reviewer", READ_VERBS.to_vec()));
        assert!(
            Skill::parse("bp-complete").is_err() && Skill::parse("review").is_ok() && Skill::parse("nope").is_err()
        );
    }

    #[test]
    fn arguments_are_checked() {
        let args = |s: &str| Args::parse(&s.split_whitespace().map(str::to_string).collect::<Vec<_>>());
        let a = args("bp-review --trials 5 --arms with,without,filler --fixture pr-20").unwrap();
        assert_eq!((a.trials, a.arms.len(), a.fixture.as_deref()), (5, 3, Some("pr-20")));
        assert!(args("bp-review").is_err(), "no trials");
        assert!(args("--grade --check bp-review").is_ok());
        assert!(args("bp-review --trials 2 --check").is_err(), "--check belongs to --grade");
        assert!(args("bp-review --trials 2 --arms with,more").is_err());
        assert!(args("bp-review --trials").is_err());
    }
}
