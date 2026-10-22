//! Grades the stored trials of a skill (29, design change 2). Code only: it reads each trial's transcript and vault
//! log from the store, the fixture and the bundle, and it recomputes every number of the result file. `--grade`
//! writes the file and the report. `--grade --check` compares the file with the recomputation, so a number typed by
//! hand, a swapped transcript or log, a dropped trial or an edited skill shows. The owner's two lines, `walkthrough:`
//! and `status:`, are kept.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::eval::Skill;
use crate::eval_arms::{FILLER, body_of};
use crate::eval_fixture::{Fixture, NEAR, Review};
use crate::eval_trace::{BpCall, Trace, sha256};

/// A run passes when its review is recorded (`walk --done`) and finds at least this share of the ground truth.
const RECALL: f64 = 0.5;

const STATUSES: [&str; 4] = ["pending", "capability", "regression", "fold"];

/// The load-bearing steps of `bp-review` that a transcript shows (21, the review row; the skill's steps 1 to 5).
const STEPS: [&str; 5] = ["walk", "claim", "diff", "deltas", "review"];

#[derive(Default)]
pub struct Graded {
    pub arm: String,
    pub fixture: String,
    pub number: usize,
    pub trial: String,
    pub sha: String,
    pub log_sha: String,
    /// The hash of the role file and skill body this trial ran on, from `<trial>.hash`.
    pub hash: Option<String>,
    pub models: BTreeSet<String>,
    /// The skill is where the arm puts it: preloaded in `with`, absent in `without`, filler in `filler`.
    pub loaded: bool,
    /// The run made at least one `bp` call: only such a run is scored on adherence.
    pub reached: bool,
    pub steps: [bool; 5],
    pub out_of_row: usize,
    pub done: bool,
    /// Ground-truth items some finding of the table matches, and items the final text names in any form.
    pub hits: usize,
    pub prose_hits: usize,
    pub truth: usize,
    /// Each finding's location, its text and the ground-truth item it matches (a position), if any.
    pub findings: Vec<(String, String, Option<usize>)>,
    pub failure: Option<String>,
}

impl Graded {
    pub fn passed(&self) -> bool {
        self.done && self.truth > 0 && share(self.hits, self.truth) >= RECALL
    }

    fn adherence(&self) -> f64 {
        share(self.steps.iter().filter(|s| **s).count(), STEPS.len())
    }
}

pub struct Grade {
    skill: Skill,
    pub trials: Vec<Graded>,
    holdout: usize,
    /// The hash of the role file and skill body as the files hold them now.
    current: String,
    fixtures: Vec<Fixture>,
}

/// The hash of the role file and the skill body together, `git hash-object --stdin`: the key of check 10. A run
/// writes it beside its transcript, and a grade compares the traces' hash with the files'.
pub fn bundle_hash(root: &Path, skill: &Skill) -> Result<String> {
    let body = std::fs::read_to_string(root.join("bundle/skills").join(&skill.name).join("SKILL.md"))
        .with_context(|| format!("read the body of {}", skill.name))?;
    let role = std::fs::read_to_string(root.join(format!("bundle/roles/{}.md", skill.role_file())))?;
    hash_object(&format!("{role}{body}"))
}

impl Grade {
    /// Grades every `<arm>-<fixture>-<n>.jsonl` in `store`. `fixtures` is the skill's fixture directory.
    pub fn read(skill: &Skill, store: &Path, fixtures: &Path, root: &Path) -> Result<Self> {
        let fixtures_all = Fixture::load(fixtures, None)?;
        let body = std::fs::read_to_string(root.join("bundle/skills").join(&skill.name).join("SKILL.md"))
            .with_context(|| format!("read the body of {}", skill.name))?;
        let marker = body_of(&body).lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string();
        let mut trials = Vec::new();
        let mut names: Vec<_> = std::fs::read_dir(store)
            .with_context(|| format!("read {}", store.display()))?
            .filter_map(|e| e.ok()?.file_name().to_str().map(str::to_string))
            .filter(|n| Path::new(n).extension().is_some_and(|e| e == "jsonl"))
            .collect();
        names.sort();
        for name in names {
            let trial = name.trim_end_matches(".jsonl").to_string();
            let (arm, rest) = trial.split_once('-').context("a trial is <arm>-<fixture>-<n>")?;
            let (fixture, number) = rest.rsplit_once('-').context("a trial is <arm>-<fixture>-<n>")?;
            let fx = fixtures_all
                .iter()
                .find(|f| f.id == fixture)
                .with_context(|| format!("{trial}: no fixture {fixture} in {}", fixtures.display()))?;
            let path = store.join(&name);
            let trace = Trace::parse(&std::fs::read_to_string(&path)?);
            let log_path = store.join(format!("{trial}.vault.log"));
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            let loaded = match arm {
                "with" => trace.preloaded(&marker),
                "without" => !trace.mentions(&marker) && !trace.mentions(FILLER),
                "filler" => trace.preloaded(FILLER) && !trace.mentions(&marker),
                _ => bail!("{trial}: arm {arm} is none of with, without, filler"),
            };
            let mut graded = Graded {
                arm: arm.into(),
                fixture: fixture.into(),
                number: number.parse().with_context(|| format!("{trial}: the trial number is a count"))?,
                trial: trial.clone(),
                sha: sha256(&path)?,
                log_sha: if log_path.is_file() { sha256(&log_path)? } else { String::new() },
                hash: std::fs::read_to_string(store.join(format!("{trial}.hash"))).ok().map(|h| h.trim().to_string()),
                models: trace.models(),
                loaded,
                failure: trace.failure().map(str::to_string),
                ..Graded::default()
            };
            score(&mut graded, skill, fx, &trace, &log);
            trials.push(graded);
        }
        let holdout = crate::eval_fixture::holdout(fixtures).len();
        Ok(Self { skill: skill.clone(), trials, holdout, current: bundle_hash(root, skill)?, fixtures: fixtures_all })
    }

    /// What makes the store ungradable, one line each: a trial without its hash or vault log, trials run on two
    /// bundles or on one that is not the files', and trial numbers with a gap (a dropped trial).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.trials {
            if t.hash.is_none() {
                out.push(format!("{}: no {}.hash; rerun the trial", t.trial, t.trial));
            }
            if t.log_sha.is_empty() {
                out.push(format!("{}: no {}.vault.log", t.trial, t.trial));
            }
        }
        let hashes: BTreeSet<&String> = self.trials.iter().filter_map(|t| t.hash.as_ref()).collect();
        if hashes.len() > 1 {
            out.push(format!("the store holds trials of {} bundles: {hashes:?}; rerun them on one", hashes.len()));
        } else if let Some(h) = hashes.first().filter(|h| ***h != self.current) {
            out.push(format!(
                "the trials ran on {h}, the role and skill files hash to {}; rerun the evals",
                self.current
            ));
        }
        let groups: BTreeSet<(&String, &String)> = self.trials.iter().map(|t| (&t.arm, &t.fixture)).collect();
        for (arm, fixture) in groups {
            let mut numbers: Vec<usize> =
                self.trials.iter().filter(|t| &t.arm == arm && &t.fixture == fixture).map(|t| t.number).collect();
            numbers.sort_unstable();
            if numbers != (1..=numbers.len()).collect::<Vec<_>>() {
                out.push(format!("{arm}-{fixture}: trial numbers {numbers:?} are not 1 to {}", numbers.len()));
            }
        }
        out
    }

    /// The lower bound of `uplift_ci`, when both arms ran.
    pub fn uplift_low(&self) -> Option<f64> {
        let (with, without) = (Stats::of(&self.arm("with")), Stats::of(&self.arm("without")));
        (with.runs > 0 && without.runs > 0)
            .then(|| newcombe((with.passes, with.runs), (without.passes, without.runs)).1)
    }

    /// The bundle the trials ran on: their one hash, else the files' (and `problems` says why they differ).
    fn hash(&self) -> &str {
        let ran: BTreeSet<&String> = self.trials.iter().filter_map(|t| t.hash.as_ref()).collect();
        if ran.len() == 1 { ran.first().map_or(&self.current, |h| h) } else { &self.current }
    }

    fn arm(&self, arm: &str) -> Vec<&Graded> {
        self.trials.iter().filter(|t| t.arm == arm).collect()
    }

    /// The result file: the numbers recomputed, then the owner's two lines as `keep` gives them.
    pub fn result(&self, keep: &Owner) -> String {
        let (with, without) = (Stats::of(&self.arm("with")), Stats::of(&self.arm("without")));
        let rate = |k: usize, n: usize| if n == 0 { "n/a".to_string() } else { f2(share(k, n)) };
        let arms: Vec<&str> =
            [("with", &with), ("without", &without)].iter().filter(|a| a.1.runs > 0).map(|a| a.0).collect();
        let loaded = self.trials.iter().filter(|t| t.loaded).count();
        let models: BTreeSet<&String> = self.arm("with").into_iter().flat_map(|r| &r.models).collect();
        let ci = (with.runs > 0 && without.runs > 0)
            .then(|| newcombe((with.passes, with.runs), (without.passes, without.runs)));
        let mut lines = vec![
            format!("# Eval: {}\n", self.skill.name),
            format!("arms: {}", arms.join(", ")),
            format!("holdout: {}", self.holdout),
            if loaded == self.trials.len() {
                "skill_loaded: all".to_string()
            } else {
                format!("skill_loaded: {loaded} of {}", self.trials.len())
            },
            format!("hash: {}", self.hash()),
            format!(
                "model: {}",
                if models.is_empty() {
                    "unknown".into()
                } else {
                    models.into_iter().cloned().collect::<Vec<_>>().join(", ")
                }
            ),
            format!("n: {}", with.runs.min(without.runs)),
            format!("baseline: {}", rate(without.passes, without.runs)),
            format!("with: {}", rate(with.passes, with.runs)),
            format!("uplift: {}", ci.map_or("n/a".into(), |c| f2(c.0))),
            format!("uplift_ci: {}", ci.map_or("n/a".into(), |c| format!("[{}, {}]", f2(c.1), f2(c.2)))),
            format!("adherence: {}", with.adherence.map_or("n/a".into(), f2)),
            format!("out_of_row: {}", with.out_of_row),
            format!("false_findings: {}", rate(with.wrong, with.found)),
            format!("walkthrough: {}", keep.walkthrough),
            format!("status: {}", keep.status),
            "\n## Trials\n".to_string(),
            "| trial | sha256 | vault log sha256 |\n|---|---|---|".to_string(),
        ];
        lines.extend(self.trials.iter().map(|t| format!("| {} | {} | {} |", t.trial, t.sha, t.log_sha)));
        lines.join("\n") + "\n"
    }

    /// The report the owner reads at the walkthrough: per arm, then per trial with a pointer to the raw trace.
    pub fn report(&self, store: &Path) -> String {
        let mut lines =
            vec![format!("# Eval report: {}\n", self.skill.name), format!("Traces: `{}`\n", store.display())];
        lines.push("## Ground truth\n".into());
        for f in &self.fixtures {
            lines.push(format!("Fixture {} (base `{}`, head `{}`):\n", f.id, short(&f.base), short(&f.head)));
            lines.extend(f.truth.iter().enumerate().map(|(i, t)| format!("{}. `{}`: {}", i + 1, t.location(), t.what)));
            lines.push(String::new());
        }
        lines.push("## Arms\n".into());
        lines.push("| arm | runs | outcome | prose recall | reached the tool | adherence there | out of row | findings | false | runs with no finding | skill loaded |".into());
        lines.push("|---|---|---|---|---|---|---|---|---|---|---|".into());
        for arm in ["with", "without", "filler"] {
            let runs = self.arm(arm);
            let s = Stats::of(&runs);
            if s.runs > 0 {
                lines.push(format!(
                    "| {arm} | {} | {} of {} | {} of {} | {} | {} | {} | {} | {} | {} | {} |",
                    s.runs,
                    s.passes,
                    s.runs,
                    s.prose,
                    s.truth,
                    s.reached,
                    s.adherence.map_or("n/a".into(), f2),
                    s.out_of_row,
                    s.found,
                    s.wrong,
                    s.silent,
                    s.loaded
                ));
            }
        }
        lines.push(format!("\n- **Outcome:** a run passes when `bp walk --for review --done` holds after the review is recorded and the findings table matches at least {RECALL} of the ground truth. That needs the shape only `bp-review` defines (a `head:` line, one table).\n- **Prose recall:** items of the ground truth that the final text names in any form (its file and a line within {NEAR} lines). It does not depend on the shape, so a gap in outcome with none in prose recall means the arm cannot write the shape.\n- **Uplift:** the `with` rate less the `without` rate; the result file gives its Newcombe 95% interval.\n- **Role body:** the `without` arm's copy drops the sentences about the `Skill:` line; the `filler` arm keeps them.\n"));
        lines.push("## Trials".into());
        for t in &self.trials {
            let steps: Vec<String> =
                STEPS.iter().zip(t.steps).map(|(n, ok)| format!("{n} {}", if ok { "yes" } else { "no" })).collect();
            let yes = |b: bool| if b { "yes" } else { "no" };
            lines.push(format!(
                "\n### {t}\n\n- outcome: {}; recorded: {}; ground truth matched by a table row: {} of {}; named in the text: {}\n- reached the tool: {}; steps: {}\n- out of row: {}; skill loaded as designed: {}\n- trace: `{dir}/{t}.jsonl`, vault log: `{dir}/{t}.vault.log`",
                if t.passed() { "pass" } else { "fail" },
                yes(t.done),
                t.hits,
                t.truth,
                t.prose_hits,
                yes(t.reached),
                steps.join(", "),
                t.out_of_row,
                if t.loaded { "yes" } else { "NO" },
                t = t.trial,
                dir = store.display(),
            ));
            if let Some(why) = &t.failure {
                lines.push(format!("- FAILED: {why}"));
            }
            for (at, what, item) in &t.findings {
                let what: String = what.chars().take(160).collect();
                let verdict = item.map_or("FALSE".to_string(), |i| format!("match, truth item {}", i + 1));
                lines.push(format!("  - {verdict} `{at}`: {what}"));
            }
        }
        lines.join("\n") + "\n"
    }
}

/// One arm's trials, counted.
struct Stats {
    runs: usize,
    passes: usize,
    /// Ground-truth items named in the final texts, over all runs, and the items the runs could have named.
    prose: usize,
    truth: usize,
    reached: usize,
    /// The mean share of steps done, over the runs that reached the tool; `None` when none did.
    adherence: Option<f64>,
    out_of_row: usize,
    found: usize,
    wrong: usize,
    /// Runs whose table held no finding: a reviewer that returns nothing is not precise.
    silent: usize,
    loaded: usize,
}

impl Stats {
    fn of(runs: &[&Graded]) -> Self {
        let reached: Vec<&&Graded> = runs.iter().filter(|r| r.reached).collect();
        let found: Vec<&(String, String, Option<usize>)> = runs.iter().flat_map(|r| &r.findings).collect();
        Self {
            runs: runs.len(),
            passes: runs.iter().filter(|r| r.passed()).count(),
            prose: runs.iter().map(|r| r.prose_hits).sum(),
            truth: runs.iter().map(|r| r.truth).sum(),
            reached: reached.len(),
            adherence: (!reached.is_empty())
                .then(|| reached.iter().map(|r| r.adherence()).sum::<f64>() / float(reached.len())),
            out_of_row: runs.iter().map(|r| r.out_of_row).sum(),
            found: found.len(),
            wrong: found.iter().filter(|f| f.2.is_none()).count(),
            silent: runs.iter().filter(|r| r.findings.is_empty()).count(),
            loaded: runs.iter().filter(|r| r.loaded).count(),
        }
    }
}

/// A count as a float. Counts of trials and findings are far below 2^52.
#[allow(clippy::cast_precision_loss)]
fn float(n: usize) -> f64 {
    n as f64
}

/// `k` over `n`, for `n` above 0.
fn share(k: usize, n: usize) -> f64 {
    float(k) / float(n)
}

/// Fills the scores of a `bp-review` trial: the steps its transcript shows, the calls outside the role's row, the
/// record in the vault log and the findings against the ground truth.
fn score(g: &mut Graded, skill: &Skill, fx: &Fixture, trace: &Trace, log: &str) {
    let calls = trace.bp_calls();
    let allowed = skill.allowed();
    let review = Review::parse(&trace.final_text());
    let first = |c: &BpCall| c.args.split_whitespace().next().unwrap_or_default().to_string();
    let head = short(&fx.head);
    g.reached = !calls.is_empty();
    g.steps = [
        calls.iter().any(|c| c.verb == "walk" && c.args.contains("--for review")),
        calls.iter().any(|c| c.verb == "list" && first(c) == "claim"),
        trace.commands().iter().any(|c| c.contains("git") && c.contains("diff") && c.contains(head)),
        calls.iter().any(|c| c.verb == "list" && first(c) == "delta"),
        review.as_ref().is_some_and(|r| r.tables == 1 && r.head.starts_with(head)),
    ];
    g.out_of_row = calls.iter().filter(|c| !allowed.contains(&c.verb.as_str())).count();
    g.done = log.lines().any(|l| l.starts_with("record review "));
    let findings = review.map(|r| r.findings).unwrap_or_default();
    let assigned = fx.assign(&findings);
    g.hits = fx.hits(&assigned);
    g.prose_hits = fx.named_in(&trace.final_text());
    g.truth = fx.truth.len();
    g.findings = findings.iter().zip(assigned).map(|(f, item)| (f.location.clone(), f.text.clone(), item)).collect();
}

/// The owner's lines of a result file: kept across a regrade.
pub struct Owner {
    walkthrough: String,
    status: String,
}

impl Owner {
    /// The values in `existing`, or `pending` for both. `Err` names a line the owner broke.
    pub fn of(existing: Option<&str>) -> Result<Self> {
        let get = |key: &str| existing.and_then(|t| t.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(": ")));
        let (walkthrough, status) = (get("walkthrough").unwrap_or("pending"), get("status").unwrap_or("pending"));
        if !STATUSES.contains(&status) {
            bail!("status: {status} is none of {}", STATUSES.join(", "));
        }
        if walkthrough.trim().is_empty() {
            bail!("walkthrough: holds nothing; the owner writes `approved` or the friction");
        }
        Ok(Self { walkthrough: walkthrough.into(), status: status.into() })
    }

    /// `capability` says the skill adds something; an interval that does not clear 0 says otherwise (29, decision 7).
    pub fn contradicts(&self, uplift_low: Option<f64>) -> Option<String> {
        let clear = uplift_low.is_some_and(|lo| lo >= 0.005);
        (self.status == "capability" && !clear)
            .then(|| "status: capability, but the lower bound of uplift_ci does not clear 0; the owner decides regression or fold".into())
    }
}

/// The 95% interval of a difference of two proportions (Newcombe's hybrid score): `(difference, low, high)`. A Wald
/// interval has no width at 0 or 1, and five trials per arm are often there.
fn newcombe((k1, n1): (usize, usize), (k0, n0): (usize, usize)) -> (f64, f64, f64) {
    let (p1, l1, u1) = wilson(k1, n1);
    let (p0, l0, u0) = wilson(k0, n0);
    let d = p1 - p0;
    (d, d - ((p1 - l1).powi(2) + (u0 - p0).powi(2)).sqrt(), d + ((u1 - p1).powi(2) + (p0 - l0).powi(2)).sqrt())
}

fn wilson(k: usize, n: usize) -> (f64, f64, f64) {
    let (p, n, z2) = (share(k, n), float(n), 1.96_f64 * 1.96);
    let (centre, spread, scale) =
        (p + z2 / (2.0 * n), 1.96 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt(), 1.0 + z2 / n);
    (p, (centre - spread) / scale, (centre + spread) / scale)
}

fn f2(x: f64) -> String {
    format!("{:.2}", if x.abs() < 0.005 { 0.0 } else { x })
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// `git hash-object --stdin`: the hash of the role file and the skill body together, the key of check 10.
fn hash_object(text: &str) -> Result<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("run git hash-object")?;
    child.stdin.take().context("git's stdin")?.write_all(text.as_bytes())?;
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The lines where `file` differs from `want`: empty when they are equal.
pub fn differences(want: &str, file: &str) -> Vec<String> {
    let (w, f): (Vec<&str>, Vec<&str>) = (want.lines().collect(), file.lines().collect());
    let mut out = Vec::new();
    for i in 0..w.len().max(f.len()) {
        let (a, b) = (w.get(i).copied(), f.get(i).copied());
        if a != b {
            out.push(format!(
                "line {}: the traces give `{}`, the file holds `{}`",
                i + 1,
                a.unwrap_or(""),
                b.unwrap_or("")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interval_has_width_at_the_extremes() {
        let (d, lo, hi) = newcombe((5, 5), (0, 5));
        assert!((d - 1.0).abs() < 1e-9 && lo > 0.0 && lo < 1.0 && (hi - 1.0).abs() < 1e-9, "{lo} {hi}");
        let (d, lo, hi) = newcombe((2, 5), (2, 5));
        assert!(d.abs() < 1e-9 && lo < 0.0 && hi > 0.0, "equal rates: an interval over 0");
        assert_eq!(f2(-0.001), "0.00");
    }

    #[test]
    fn the_owner_lines_are_kept_and_checked() {
        let o = Owner::of(Some("n: 5\nwalkthrough: approved\nstatus: fold\n")).unwrap();
        assert_eq!((o.walkthrough.as_str(), o.status.as_str()), ("approved", "fold"));
        let o = Owner::of(None).unwrap();
        assert_eq!((o.walkthrough.as_str(), o.status.as_str()), ("pending", "pending"));
        assert!(Owner::of(Some("status: keep\n")).is_err());
        let capability = Owner::of(Some("status: capability\n")).unwrap();
        assert!(capability.contradicts(Some(-0.1)).is_some() && capability.contradicts(None).is_some());
        assert!(capability.contradicts(Some(0.39)).is_none() && o.contradicts(None).is_none());
    }

    #[test]
    fn differences_name_the_line() {
        assert!(differences("a\nb\n", "a\nb\n").is_empty());
        let d = differences("a\nn: 5\n", "a\nn: 9\nextra\n");
        assert_eq!(d.len(), 2);
        assert!(d[0].starts_with("line 2") && d[0].contains("`n: 5`") && d[0].contains("`n: 9`"));
    }
}
