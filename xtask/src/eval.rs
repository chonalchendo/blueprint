//! `cargo xtask eval`, the skill eval (29, design changes 1 and 2; 17 check 6). Blueprint never runs an agent, so the
//! runner lives here, and the owner starts it in their own terminal. Skills: `bp-review`, `bp-complete`, `bp-respond`,
//! `bp-research` (31, design change 3).
//!
//! ```text
//! cargo xtask eval <skill> --trials N [--arms with,without,filler,<arm>] [--fixture <id>] [--bin <arm>=<bp>]...
//!                                     [--arm <arm>=<git ref>]...
//! cargo xtask eval --grade [--check] <skill>
//! ```
//!
//! A run fills each arm and fixture to N stored trials (a retry runs only what is missing, and a failed trial is
//! missing: the retry replaces it). A trial builds the
//! replay's sandbox, starts the agent in the clone, and stores the role's transcript and the vault log under
//! `$BP_REDESIGN/analysis/evals-skills/<skill>/`. Then it grades the store, as `--grade` does. The agent is
//! `$BP_EVAL_AGENT`, a shell command that reads the prompt from `$BP_EVAL_PROMPT`; if it writes the role's transcript
//! to `$BP_EVAL_TRANSCRIPT`, that file is the trace. If it does not, the trace is the lead of the session that the
//! agent's result JSON names (`eval_agent.rs`), and the store keeps every transcript of that session under
//! `<trial>/`, with a `run.txt` that names the lead and each child. A run past `$BP_EVAL_TIMEOUT` seconds is killed
//! (default 1800; 3600 for `bp-complete`, which builds a whole issue), and a trial with no transcript is stored as a
//! failed trial; a failed trial counts as a fail, and the report names the reason. `$BP_VAULT` is the vault to copy.
//! `$BP_EVAL_TARGET` is a cargo target directory to keep across runs (default: one per run), where a worker's blocks
//! build the issue's code; every trial gets its own `$TMPDIR` under the run's work directory, apart from the directory
//! that holds its vault and forge (a probe's `cargo xtask sandbox` or `xtask` tests would prune them),
//! `CARGO_INCREMENTAL=0` and,
//! unless set, `CARGO_BUILD_JOBS=4`. Flags for a test: `--repo <dir>` (holds the fixtures' commits), `--fixtures <dir>`,
//! `--bin <arm>=<bp>` (trusted: the arm's clone is not asserted, and its trials carry the shipped role-and-skill hash, because
//! the store holds one hash; the arm's body is the caller's claim). An arm outside `with`, `without` and `filler`, such
//! as a body with one rule cut, runs only with its `--bin` or its `--arm`, and grades as `with` does. `--arm old=<ref>`
//! (30, design change 2) builds the arm from `git archive <ref> bundle/`: its trials carry that ref's hash and a
//! `ref:` line, dispatch from its template and check its body's first line, and `--arm` adds the arm to `--arms`. A
//! usage error exits 4, as every xtask check does.
//!
//! `bp-research` runs the arms `with` and those from `--arm` (31, Decided 4: no `without` or `filler`). Its sandbox
//! seeds the fixture's nodes and dispatches the question, and its grader is issue 3's: a run stores the trials, and
//! `--grade --check` parses every fixture and prints its counts. A trial whose arm has no skill directory records
//! `skill: none`.
//!
//! A fixture set other than the skill's own writes its own store and result file (30, "Blocks of 24 to 29"): the
//! directory's name names them, and the development set `<skill>-mutants` names them `<skill>-dev`. A set with no
//! trials and no result file grades as nothing to check.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use bp_operation::{Action, READ_VERBS, Role};
use serde_json::json;

use crate::eval_agent::{Session, project_dir};
use crate::eval_arms::{bins, prompt};
use crate::eval_fixture::{Fixture, Review};
use crate::eval_grade::{Grade, Marker, bundle_hash, read_hash, ref_hash, stamp};
use crate::eval_plant::Research;
use crate::eval_result::{Owner, differences};
use crate::eval_sandbox::{Sandbox, capture};
use crate::eval_trace::Trace;

/// The agent the owner's replay used, headless: a session that dispatches the role. The tools are allowed by name,
/// not by skipping permissions, because the role's shell can reach the rest of the machine. JSON output carries the
/// denials, the cost and the turns. Without the ceiling, `-p` drops a background subagent's partial result after ten
/// idle minutes; the runner's own limit replaces it. Unverified until the owner's first run.
const AGENT: &str = r#"CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0 claude -p "$(cat "$BP_EVAL_PROMPT")" --setting-sources project,local --output-format json --allowedTools "Bash,Read,Grep,Glob,Edit,Write,Task,Agent""#;

/// The node a research dispatch names: the project node, since `projects/bp` is no node (31, design change 2).
const NODE: &str = "projects/bp/project";

/// A bad argument: `main` exits 4 for it, as for every other xtask usage error.
#[derive(Debug)]
pub struct Usage(String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Usage {}

pub(crate) fn usage(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Usage(message.into()))
}

#[derive(Clone)]
pub struct Skill {
    pub name: String,
    pub action: Action,
    role: Role,
}

impl Skill {
    pub(crate) fn parse(name: &str) -> Result<Self> {
        let action: Action =
            name.strip_prefix("bp-").unwrap_or(name).parse().map_err(|_| usage(format!("{name} is no skill")))?;
        // The reviewer returns the review and the session records it (21), so no action belongs to the reviewer role.
        let role = match action {
            Action::Review => Role::Reviewer,
            Action::Complete | Action::Respond => Role::Worker,
            Action::Research => Role::Researcher,
            _ => {
                return Err(usage(format!(
                    "{name}: no fixture or grader; the eval covers review, complete, respond, research"
                )));
            }
        };
        Ok(Self { name: format!("bp-{action}"), action, role })
    }

    /// The skills whose bodies the result hash covers: the measured one, and for a worker its sibling, because the
    /// role preloads both (`skills:` in `bp-worker.md`).
    pub fn preloaded(&self) -> Vec<String> {
        if self.role == Role::Worker {
            vec!["bp-complete".into(), "bp-respond".into()]
        } else {
            vec![self.name.clone()]
        }
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
            // A probing review builds and runs the code (30, design change 5).
            _ => 1800,
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
    /// `(arm, git ref)`: an arm built from the bundle at that ref.
    refs: Vec<(String, String)>,
    grade: bool,
    check: bool,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self> {
        let mut a = Self {
            skill: String::new(),
            trials: 0,
            arms: vec!["with".into(), "without".into(), "filler".into()],
            fixture: None,
            fixtures: None,
            repo: ".".into(),
            bins: Vec::new(),
            refs: Vec::new(),
            grade: false,
            check: false,
        };
        let mut arms = false;
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| usage(format!("{arg} takes a value")));
            match arg.as_str() {
                "--grade" => a.grade = true,
                "--check" => a.check = true,
                "--trials" => a.trials = value()?.parse().map_err(|_| usage("--trials is a count"))?,
                "--arms" => {
                    a.arms = value()?.split(',').map(str::to_string).collect();
                    arms = true;
                }
                "--fixture" => a.fixture = Some(value()?.clone()),
                "--fixtures" => a.fixtures = Some(value()?.into()),
                "--repo" => a.repo = value()?.into(),
                "--bin" => {
                    let (arm, path) = value()?.split_once('=').ok_or_else(|| usage("--bin is <arm>=<path>"))?;
                    a.bins.push((arm.into(), path.into()));
                }
                "--arm" => {
                    let (arm, at) = value()?.split_once('=').ok_or_else(|| usage("--arm is <arm>=<git ref>"))?;
                    a.refs.push((arm.into(), at.into()));
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
        // Research has no skill to take out today, and its eval runs `with` and `old` only (31, Decided 4).
        if Skill::parse(&a.skill).is_ok_and(|s| s.action == Action::Research) {
            if !arms {
                a.arms = vec!["with".into()];
            }
            if let Some(bad) = a.arms.iter().find(|x| ["without", "filler"].contains(&x.as_str())) {
                return Err(usage(format!(
                    "bp-research has no {bad} arm; its arms are with and an --arm <arm>=<git ref>"
                )));
            }
        }
        // The runner builds the three arms and an arm from a git ref; another arm's bundle is the caller's, so its
        // binary must come with it. A trial is `<arm>-<fixture>-<n>`, so an arm is one lower-case word.
        let known = |x: &String| ["with", "without", "filler"].contains(&x.as_str());
        for (arm, _) in &a.refs {
            if known(arm) || a.bins.iter().any(|(b, _)| b == arm) || arm.is_empty() {
                return Err(usage(format!("--arm {arm}: an arm from a git ref is a new name, with no --bin")));
            }
            if !a.arms.contains(arm) {
                a.arms.push(arm.clone());
            }
        }
        let named = |x: &String| a.bins.iter().any(|(b, _)| b == x) || a.refs.iter().any(|(r, _)| r == x);
        let given = |x: &String| named(x) && x.bytes().all(|b| b.is_ascii_lowercase());
        if let Some(bad) = a.arms.iter().find(|x| !known(x) && !given(x)) {
            return Err(usage(format!(
                "arm {bad} is none of with, without, filler; another arm is one lower-case word with --bin {bad}=<bp> or --arm {bad}=<git ref>"
            )));
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
    let fixtures = a.fixtures.clone().unwrap_or_else(|| root.join("xtask/evals").join(&skill.name));
    let name = set_name(&skill.name, &fixtures);
    let store = results.join(&name);
    if !a.grade {
        trials(&a, &skill, root, &fixtures, &store)?;
    }
    if skill.action == Action::Research {
        println!("{}", counts(&Fixture::load(&fixtures, None)?, root, &skill)?);
        return Ok(Vec::new());
    }
    let grade = Grade::read(&skill, &store, &fixtures, root)?;
    let problems = grade.problems();
    if !problems.is_empty() {
        return Ok(problems);
    }
    let file = results.join(format!("{name}.md"));
    if grade.trials.is_empty() && !file.exists() {
        eprintln!("eval: no trials in {}", store.display());
        return Ok(Vec::new());
    }
    let existing = std::fs::read_to_string(&file).ok();
    let owner = Owner::of(existing.as_deref())?;
    let text = grade.result(&owner);
    // The grader refuses a `capability` that the interval does not carry, and writes nothing.
    let refused: Vec<String> = grade.refusal(&owner).into_iter().collect();
    if a.check {
        let Some(have) = existing else {
            return Ok(vec![format!("{} does not exist; run cargo xtask eval --grade {}", file.display(), skill.name)]);
        };
        return Ok(refused.into_iter().chain(differences(&text, &have)).collect());
    }
    if !refused.is_empty() {
        return Ok(refused);
    }
    std::fs::write(&file, text)?;
    std::fs::write(store.join("report.md"), grade.report(&store))?;
    println!("{}\n{}", file.display(), store.join("report.md").display());
    Ok(Vec::new())
}

/// The store and result-file name of the fixture set in `fixtures` (see the module comment): the directory's name,
/// and `<skill>-dev` for the development set `<skill>-mutants`. Another directory never falls back to the skill's own
/// store, so a test's set cannot overwrite the shipped result file.
fn set_name(skill: &str, fixtures: &Path) -> String {
    let full = fixtures.canonicalize().unwrap_or_else(|_| fixtures.to_path_buf());
    let dir = full.file_name().and_then(|d| d.to_str()).unwrap_or_default();
    if dir.strip_prefix(skill).and_then(|d| d.strip_prefix('-')) == Some("mutants") {
        format!("{skill}-dev")
    } else {
        dir.to_string()
    }
}

/// The counts of a research fixture set (31, design change 3): what `--grade --check` prints until issue 3's
/// grader reads the store. A plant is a unit of evidence, so a two-part file counts two. A set with no `plant`
/// fixture, or with a fixture that is no research fixture, is refused, so a wrong `--fixtures` directory fails.
fn counts(list: &[Fixture], root: &Path, skill: &Skill) -> Result<String> {
    if let Some(f) = list.iter().find(|f| f.research.is_none()) {
        bail!("{}: no research fixture; bp-research reads plant and control fixtures", f.id);
    }
    let research = list.iter().filter_map(|f| f.research.as_ref());
    let (controls, plants): (Vec<&Research>, Vec<&Research>) = research.partition(|r| r.control);
    let seeds: usize = plants.iter().map(|r| r.plants.len()).sum();
    let two = plants.iter().filter(|r| r.plants.len() == 2).count();
    if plants.is_empty() {
        bail!("the set holds no plant fixture, so there is nothing to measure");
    }
    let skilled = root.join("bundle/skills").join(&skill.name).is_dir();
    Ok(format!(
        "plants: {seeds} in {} files\ncontrols: {}\ntwo_part: {two}\nskill: {}\ngrader: none until issue 3",
        plants.len(),
        controls.len(),
        if skilled { skill.name.as_str() } else { "none" }
    ))
}

/// Each `--arm <arm>=<git ref>` as its commit, the hash of the role and skill files there, and whether it holds the
/// skill.
fn resolve(refs: &[(String, String)], root: &Path, skill: &Skill) -> Result<BTreeMap<String, (String, String, bool)>> {
    let mut out = BTreeMap::new();
    for (arm, at) in refs {
        let sha =
            capture(Command::new("git").args(["rev-parse", "--verify", &format!("{at}^{{commit}}")]).current_dir(root))
                .with_context(|| format!("--arm {arm}={at}: no commit"))?;
        let (hash, marker) = ref_hash(root, skill, &sha)?;
        out.insert(arm.clone(), (sha, hash, matches!(marker, Marker::Preload(_))));
    }
    Ok(out)
}

/// The lead's transcript of the agent's session (`Session::find`), with every transcript of that session kept in
/// `kept`, also when no transcript is the lead; else why there is none.
fn lead(sb: &Sandbox, log: &Path, skill: &Skill, kept: &Path) -> Result<Result<PathBuf, String>> {
    let config = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map_or_else(|| Path::new(&std::env::var_os("HOME").unwrap_or_default()).join(".claude"), PathBuf::from);
    let output = std::fs::read_to_string(log).unwrap_or_default();
    let project = project_dir(&config, &sb.code());
    let Some(session) = Session::find(&config, &sb.code(), &output, &skill.role_file()) else {
        return Ok(Err(format!(
            "no session of the agent in {} and no result line in {}",
            project.display(),
            log.display()
        )));
    };
    session.store(kept)?;
    let why = format!("no {} transcript at depth 1 in {}", skill.role_file(), project.join(&session.id).display());
    Ok(session.lead.ok_or(why))
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
    // A trial is stored once it has a transcript. One that holds the failure record does not count: the retry
    // runs it again and replaces its files.
    let stored = |id: &str| {
        let text = std::fs::read_to_string(store.join(format!("{id}.jsonl"))).ok();
        text.is_some_and(|t| Trace::parse(&t).failure().is_none())
    };
    // A stored trial of another fixture or arm protocol would be kept and graded beside the new ones: refuse before
    // anything is built.
    let stamps = list.iter().map(stamp).collect::<Result<Vec<_>>>()?;
    for (fx, stamp) in list.iter().zip(&stamps) {
        for id in (1..=a.trials).flat_map(|n| a.arms.iter().map(move |arm| format!("{arm}-{}-{n}", fx.id))) {
            let have = read_hash(store, &id).map(|h| h.1);
            if stored(&id) && have.as_ref() != Some(stamp) {
                bail!("{id} ran on another fixture or arm protocol; move {} aside and rerun", store.display());
            }
        }
    }
    let refs = resolve(&a.refs, root, skill)?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-eval"), 8)?;
    let shas = refs.iter().map(|(arm, (sha, ..))| (arm.clone(), sha.clone())).collect();
    let bin = bins(root, skill, &a.arms, &work, (&a.bins, &shas))?;
    let repo = a.repo.canonicalize().with_context(|| format!("read {}", a.repo.display()))?;
    let shipped = bundle_hash(root, skill)?;
    let skilled = skill.action != Action::Research || root.join("bundle/skills").join(&skill.name).is_dir();
    for round in 1..=a.trials {
        for arm in &a.arms {
            for (fx, stamp) in list.iter().zip(&stamps) {
                let id = format!("{arm}-{}-{round}", fx.id);
                if stored(&id) {
                    continue;
                }
                eprintln!("eval: trial {id}");
                let verify = !a.bins.iter().any(|(b, _)| b == arm);
                let check = verify.then_some((arm.as_str(), refs.get(arm).map(|r| r.0.as_str())));
                let (src, bindir) = (Path::new(&source), &bin[arm]);
                let research = fx.research.as_ref();
                let sb = match research {
                    Some(_) => Sandbox::research(&repo, src, bindir, &work, fx, skill, check),
                    None => Sandbox::build(&repo, src, bindir, &work, fx, skill, check),
                }
                .with_context(|| format!("build the sandbox of {id}"))?;
                // An arm built from an edited bundle dispatches from that bundle's template.
                let edited = work.join(format!("bundle-{arm}"));
                let bundle = if edited.is_dir() { edited } else { root.join("bundle") };
                let log = store.join(format!("{id}.agent.log"));
                // A research dispatch names the project node, as `walk --for research` takes a project (31).
                let (node, question) =
                    research.map_or((sb.issue.as_str(), None), |r| (NODE, Some(r.question.as_str())));
                let (made, timed_out) = sb.run_agent(
                    &agent,
                    &prompt(&bundle, skill, node, question)?,
                    (arm, &fx.id),
                    (&log, Duration::from_secs(seconds)),
                )?;
                let found = if made.is_file() { Ok(made) } else { lead(&sb, &log, skill, &store.join(&id))? };
                let mut text = found.as_ref().map(std::fs::read_to_string).ok().transpose()?.unwrap_or_default();
                let failure = timed_out.or_else(|| found.err());
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
                        sb.record_review(&sb.head, &review.body)?;
                    }
                    sb.vault_log()?
                } else if research.is_some() {
                    sb.vault_log()?
                } else {
                    format!("{}\n{}", sb.vault_log()?, sb.end_state(skill, fx)?)
                };
                std::fs::write(store.join(format!("{id}.vault.log")), log)?;
                // An arm from a git ref ran that ref's body: its hash, and the ref the grader reads it from.
                let (hash, at, skilled) =
                    refs.get(arm).map_or((shipped.as_str(), String::new(), skilled), |(sha, hash, s)| {
                        (hash.as_str(), format!("ref: {sha}\n"), *s)
                    });
                let bare = if skilled { "" } else { "skill: none\n" };
                std::fs::write(store.join(format!("{id}.hash")), format!("{hash}\nstamp: {stamp}\n{at}{bare}"))?;
                // The pull request's head: a mutant's planted one is what the reviewer diffs and names.
                std::fs::write(store.join(format!("{id}.head")), format!("{}\n", sb.head))?;
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
        let r = Skill::parse("bp-research").unwrap();
        assert_eq!((r.role_file().as_str(), r.allowed(), r.seconds()), ("bp-researcher", READ_VERBS.to_vec(), 1800));
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
    fn the_research_check_refuses_a_set_with_no_plant() {
        let (root, skill) = (Path::new(env!("CARGO_MANIFEST_DIR")).join(".."), Skill::parse("bp-research").unwrap());
        let set = |d: &str| Fixture::load(&root.join("xtask/evals").join(d), None).unwrap();
        let have = counts(&set("bp-research-plants"), &root, &skill).unwrap();
        assert!(have.starts_with("plants: 15 in 12 files\ncontrols: 5\ntwo_part: 3\n"), "{have}");
        let review = counts(&set("bp-review"), &root, &skill).unwrap_err().to_string();
        assert!(review.ends_with("no research fixture; bp-research reads plant and control fixtures"), "{review}");
        let controls: Vec<Fixture> =
            set("bp-research-plants").into_iter().filter(|f| f.kind.as_deref() == Some("control")).collect();
        assert!(counts(&controls, &root, &skill).unwrap_err().to_string().contains("no plant fixture"));
    }

    #[test]
    fn a_fixture_set_names_its_store_and_result_file() {
        let name = |d: &str| set_name("bp-review", Path::new(d));
        assert_eq!(name("xtask/evals/bp-review"), "bp-review");
        assert_eq!(name("xtask/evals/bp-review-mutants/"), "bp-review-dev");
        assert_eq!(name("xtask/evals/bp-review-holdout"), "bp-review-holdout");
        assert_eq!(name("/tmp/x/fixtures"), "fixtures", "another directory names its own store");
        assert_eq!(name("/tmp/bp-reviewer"), "bp-reviewer");
    }

    #[test]
    fn arguments_are_checked() {
        let args = |s: &str| Args::parse(&s.split_whitespace().map(str::to_string).collect::<Vec<_>>());
        let a = args("bp-review --trials 5 --arms with,without,filler --fixture pr-20").unwrap();
        assert_eq!((a.trials, a.arms.len(), a.fixture.as_deref()), (5, 3, Some("pr-20")));
        assert!(args("--grade --check bp-review").is_ok());
        let list = args("bp-review --trials 5 --arms with,list --bin list=/tmp/bp").unwrap();
        assert_eq!(list.arms, ["with", "list"], "an arm outside the three runs with its own bp");
        assert_eq!(
            args("bp-review --trials 5").unwrap().arms,
            ["with", "without", "filler"],
            "the same n for each arm"
        );
        let old = args("bp-review --trials 1 --arms with --arm old=b7ae491").unwrap();
        assert_eq!(
            (old.arms.as_slice(), old.refs.len()),
            (&["with".to_string(), "old".into()][..], 1),
            "--arm adds it"
        );
        let research = args("bp-research --trials 1 --arm old=a27fc93").unwrap();
        assert_eq!(research.arms, ["with", "old"], "research runs with and old, no without or filler");
        assert!(args("--grade --check bp-research --fixtures xtask/evals/bp-research-plants").is_ok());
        for bad in [
            "bp-research --trials 1 --arms with,without",
            "bp-research --trials 1 --arms filler",
            "bp-review --trials 1 --arm with=HEAD",
            "bp-review --trials 1 --arm old=HEAD --bin old=/x",
            "bp-review --trials 1 --arm old",
            "bp-review --trials 1 --arm Old=HEAD",
            "bp-review",
            "bp-review --trials 2 --check",
            "bp-review --trials 2 --arms with,more",
            "bp-review --trials 2 --arms with,more --bin with=/x",
            "bp-review --trials 2 --arms with,mo-re --bin mo-re=/x",
            "bp-review --trials",
            "bp-review --trials x",
        ] {
            assert!(args(bad).unwrap_err().is::<Usage>(), "{bad}");
        }
    }
}
