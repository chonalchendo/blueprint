//! `cargo xtask eval`, the skill eval (29, design changes 1 and 2; 17 check 6). Blueprint never runs an agent, so the
//! runner lives here, and the owner starts it in their own terminal. Skills: `bp-review`, `bp-complete`, `bp-respond`.
//!
//! ```text
//! cargo xtask eval <skill> --trials N [--arms with,without[,filler]] [--fixture <id>]
//! cargo xtask eval --grade [--check] <skill>
//! ```
//!
//! A run fills each arm and fixture to N stored trials (a retry runs only what is missing). A trial builds the
//! replay's sandbox, starts the agent in the clone, and stores the role's transcript and the vault log under
//! `$BP_REDESIGN/analysis/evals-skills/<skill>/`. Then it grades the store, as `--grade` does. The agent is
//! `$BP_EVAL_AGENT`, a shell command that reads the prompt from `$BP_EVAL_PROMPT`; if it writes the role's transcript
//! to `$BP_EVAL_TRANSCRIPT`, that file is the trace, and if it does not, the newest transcript of the role's subagent
//! that Claude Code wrote for the sandbox is. A run past `$BP_EVAL_TIMEOUT` seconds is killed (default 1200; 3600 for
//! `bp-complete`, which builds a whole issue; 1800 for `bp-respond`), and a trial with no transcript is stored as a
//! failed trial; a failed trial counts as a fail, and the report names the reason. `$BP_VAULT` is the vault to copy.
//! `$BP_EVAL_TARGET` is a cargo target directory to keep across runs (default: one per run), where a worker's blocks
//! build the issue's code. Flags for a test: `--repo <dir>` (holds the fixtures' commits), `--fixtures <dir>`,
//! `--bin <arm>=<bp>` (trusted: the arm's clone is not asserted). A usage error exits 4, as every xtask check does.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use bp_operation::{Action, READ_VERBS, Role};
use serde_json::json;

use crate::eval_agent::claude_transcript;
use crate::eval_arms::{bins, prompt};
use crate::eval_fixture::{Fixture, Review};
use crate::eval_grade::{Grade, Owner, bundle_hash, differences};
use crate::eval_sandbox::Sandbox;
use crate::eval_trace::Trace;

/// The agent the owner's replay used, headless: a session that dispatches the role. The tools are allowed by name,
/// not by skipping permissions, because the role's shell can reach the rest of the machine. JSON output carries the
/// denials, the cost and the turns. Without the ceiling, `-p` drops a background subagent's partial result after ten
/// idle minutes; the runner's own limit replaces it. Unverified until the owner's first run.
const AGENT: &str = r#"CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0 claude -p "$(cat "$BP_EVAL_PROMPT")" --setting-sources project,local --output-format json --allowedTools "Bash,Read,Grep,Glob,Edit,Write,Task,Agent""#;

/// A bad argument: `main` exits 4 for it, as for every other xtask usage error.
#[derive(Debug)]
pub struct Usage(String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

fn usage(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Usage(message.into()))
}

#[derive(Clone)]
pub struct Skill {
    pub name: String,
    pub action: Action,
    role: Role,
}

impl Skill {
    fn parse(name: &str) -> Result<Self> {
        let action: Action =
            name.strip_prefix("bp-").unwrap_or(name).parse().map_err(|_| usage(format!("{name} is no skill")))?;
        // The reviewer returns the review and the session records it (21), so no action belongs to the reviewer role.
        let role = match action {
            Action::Review => Role::Reviewer,
            Action::Complete | Action::Respond => Role::Worker,
            _ => return Err(usage(format!("{name}: no fixture or grader; the eval covers review, complete, respond"))),
        };
        Ok(Self { name: format!("bp-{action}"), action, role })
    }

    pub fn role_file(&self) -> String {
        format!("bp-{}", self.role)
    }

    /// The verbs the dispatched role may run for this action: the read verbs and the action's row in 21. The reviewer
    /// has no row, because the session records its review.
    pub fn allowed(&self) -> Vec<&'static str> {
        let row = if self.role == Role::Reviewer { &[][..] } else { self.action.verbs() };
        READ_VERBS.iter().copied().chain(row.iter().copied()).collect()
    }

    /// Seconds a trial may run by default: a complete trial builds a whole issue.
    fn seconds(&self) -> u64 {
        match self.action {
            Action::Complete => 3600,
            Action::Respond => 1800,
            _ => 1200,
        }
    }
}

#[derive(Debug)]
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
            let mut value = || it.next().ok_or_else(|| usage(format!("{arg} takes a value")));
            match arg.as_str() {
                "--grade" => a.grade = true,
                "--check" => a.check = true,
                "--trials" => a.trials = value()?.parse().map_err(|_| usage("--trials is a count"))?,
                "--arms" => a.arms = value()?.split(',').map(str::to_string).collect(),
                "--fixture" => a.fixture = Some(value()?.clone()),
                "--fixtures" => a.fixtures = Some(value()?.into()),
                "--repo" => a.repo = value()?.into(),
                "--bin" => {
                    let (arm, path) = value()?.split_once('=').ok_or_else(|| usage("--bin is <arm>=<path>"))?;
                    a.bins.push((arm.into(), path.into()));
                }
                s if !s.starts_with('-') && a.skill.is_empty() => a.skill = s.into(),
                s => return Err(usage(format!("unknown argument {s}"))),
            }
        }
        if a.skill.is_empty() || (!a.grade && a.trials == 0) || (a.check && !a.grade) {
            return Err(usage(
                "usage: cargo xtask eval <skill> --trials N [--arms ...] [--fixture <id>] | cargo xtask eval --grade [--check] <skill>",
            ));
        }
        if let Some(bad) = a.arms.iter().find(|x| !["with", "without", "filler"].contains(&x.as_str())) {
            return Err(usage(format!("arm {bad} is none of with, without, filler")));
        }
        Ok(a)
    }
}

/// Runs the verb. Violations are the lines `--grade --check` prints, or the reasons the store cannot be graded.
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
    let problems = grade.problems();
    if !problems.is_empty() {
        return Ok(problems);
    }
    let file = results.join(format!("{}.md", skill.name));
    let existing = std::fs::read_to_string(&file).ok();
    let owner = Owner::of(existing.as_deref())?;
    let text = grade.result(&owner);
    if a.check {
        let Some(have) = existing else {
            return Ok(vec![format!("{} does not exist; run cargo xtask eval --grade {}", file.display(), skill.name)]);
        };
        let mut out: Vec<String> = owner.contradicts(grade.uplift_low()).into_iter().collect();
        out.extend(differences(&text, &have));
        return Ok(out);
    }
    std::fs::write(&file, text)?;
    std::fs::write(store.join("report.md"), grade.report(&store))?;
    println!("{}\n{}", file.display(), store.join("report.md").display());
    Ok(Vec::new())
}

/// Fills each arm and fixture to `a.trials` stored trials, the arms interleaved so that host drift falls on both.
fn trials(a: &Args, skill: &Skill, root: &Path, fixtures: &Path, store: &Path) -> Result<()> {
    let source = std::env::var_os("BP_VAULT").context("set BP_VAULT to the vault to copy")?;
    let agent = std::env::var("BP_EVAL_AGENT").unwrap_or_else(|_| AGENT.into());
    let seconds = std::env::var("BP_EVAL_TIMEOUT").ok().and_then(|s| s.parse().ok()).unwrap_or(skill.seconds());
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        eprintln!("eval: ANTHROPIC_API_KEY is set; `claude -p` then bills the API key, not your subscription");
    }
    let list = Fixture::load(fixtures, a.fixture.as_deref())?;
    std::fs::create_dir_all(store)?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-eval"), 8)?;
    let bin = bins(root, skill, &a.arms, &work, &a.bins)?;
    let repo = a.repo.canonicalize().with_context(|| format!("read {}", a.repo.display()))?;
    let hash = bundle_hash(root, skill)?;
    let stored = |arm: &str, fx: &str| {
        let taken = std::fs::read_dir(store).into_iter().flatten().flatten();
        let prefix = format!("{arm}-{fx}-");
        taken
            .filter(|e| {
                e.file_name().to_string_lossy().starts_with(&prefix)
                    && e.path().extension().is_some_and(|x| x == "jsonl")
            })
            .count()
    };
    for round in 1..=a.trials {
        for arm in &a.arms {
            for fx in &list {
                let have = stored(arm, &fx.id);
                if have >= round {
                    continue;
                }
                let id = format!("{arm}-{}-{}", fx.id, have + 1);
                eprintln!("eval: trial {id}");
                let verify = !a.bins.iter().any(|(b, _)| b == arm);
                let sb = Sandbox::build(
                    &repo,
                    Path::new(&source),
                    &bin[arm],
                    &work,
                    fx,
                    skill,
                    verify.then_some(arm.as_str()),
                )
                .with_context(|| format!("build the sandbox of {id}"))?;
                // An arm built from an edited bundle dispatches from that bundle's template.
                let edited = work.join(format!("bundle-{arm}"));
                let bundle = if edited.is_dir() { edited } else { root.join("bundle") };
                let log = store.join(format!("{id}.agent.log"));
                let (made, timed_out) = sb.run_agent(
                    &agent,
                    &prompt(&bundle, skill, &sb.issue)?,
                    (arm, &fx.id),
                    (&log, Duration::from_secs(seconds)),
                )?;
                let config = std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(
                    || Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".claude"),
                    PathBuf::from,
                );
                let forge_name = sb.forge.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let found = if made.is_file() {
                    Some(made)
                } else {
                    claude_transcript(&config, &forge_name, &skill.role_file())
                };
                let mut text = found.as_ref().map(std::fs::read_to_string).transpose()?.unwrap_or_default();
                let failure = timed_out.or_else(|| {
                    found.is_none().then(|| {
                        format!("no {} transcript for the sandbox under {}", skill.role_file(), config.display())
                    })
                });
                if let Some(why) = &failure {
                    eprintln!("eval: {id} is stored as a failed trial: {why}; see {}", log.display());
                    let record = json!({"type": "system", "subtype": "eval_failed", "reason": why});
                    text = format!(
                        "{}{}\n",
                        if text.is_empty() || text.ends_with('\n') { text } else { text + "\n" },
                        record
                    );
                }
                // A reviewer returns its review and the session records it (21); a worker's stop is read from the
                // sandbox as the agent left it.
                let log = if skill.action == Action::Review {
                    if let Some(review) = Review::parse(&Trace::parse(&text).final_text()) {
                        sb.record_review(&fx.head, &review.body)?;
                    }
                    sb.vault_log()?
                } else {
                    format!("{}\n{}", sb.vault_log()?, sb.end_state(skill, fx)?)
                };
                std::fs::write(store.join(format!("{id}.vault.log")), log)?;
                std::fs::write(store.join(format!("{id}.hash")), format!("{hash}\n"))?;
                // The transcript lands last: a trial is stored once it has one, so an aborted run leaves no half trial.
                std::fs::write(store.join(format!("{id}.jsonl")), text)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reviewer_may_read_and_nothing_else() {
        let s = Skill::parse("bp-review").unwrap();
        assert_eq!((s.role_file().as_str(), s.allowed()), ("bp-reviewer", READ_VERBS.to_vec()));
        assert!(Skill::parse("review").is_ok() && Skill::parse("nope").is_err() && Skill::parse("bp-design").is_err());
    }

    #[test]
    fn a_worker_may_run_the_row_of_its_action() {
        let (complete, respond) = (Skill::parse("bp-complete").unwrap(), Skill::parse("respond").unwrap());
        assert_eq!(complete.role_file(), "bp-worker");
        assert_eq!(respond.role_file(), "bp-worker");
        assert!(complete.allowed().contains(&"cut") && !respond.allowed().contains(&"cut"), "cut is complete's");
        assert!(respond.allowed().contains(&"pr") && !respond.allowed().contains(&"create"));
    }

    #[test]
    fn arguments_are_checked() {
        let args = |s: &str| Args::parse(&s.split_whitespace().map(str::to_string).collect::<Vec<_>>());
        let a = args("bp-review --trials 5 --arms with,without,filler --fixture pr-20").unwrap();
        assert_eq!((a.trials, a.arms.len(), a.fixture.as_deref()), (5, 3, Some("pr-20")));
        assert!(args("--grade --check bp-review").is_ok());
        for bad in [
            "bp-review",
            "bp-review --trials 2 --check",
            "bp-review --trials 2 --arms with,more",
            "bp-review --trials",
            "bp-review --trials x",
        ] {
            assert!(args(bad).unwrap_err().is::<Usage>(), "{bad}");
        }
    }
}
