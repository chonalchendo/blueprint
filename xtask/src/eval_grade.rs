//! Grades the stored trials of a skill (29, design change 2). Code only: it reads each trial's transcript and vault
//! log from the store, the fixture and the bundle, and it recomputes every number of the result file. `--grade`
//! writes the file and the report. `--grade --check` compares the file with the recomputation, so a number typed by
//! hand, a swapped transcript or an edited skill shows. The owner's two lines, `walkthrough:` and `status:`, are kept.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::eval::Skill;
use crate::eval_fixture::{Fixture, Review};
use crate::eval_sandbox::{FILLER, body_of};
use crate::eval_trace::{BpCall, Trace, sha256};

/// A run passes when its review is recorded (`walk --done`) and finds at least this share of the ground truth.
const RECALL: f64 = 0.5;

const STATUSES: [&str; 3] = ["capability", "regression", "fold"];

/// The load-bearing steps of `bp-review` that a transcript shows (21, the review row; the skill's steps 1 to 5).
const STEPS: [&str; 5] = ["walk", "claim", "diff", "deltas", "review"];

#[derive(Default)]
pub struct Graded {
    pub arm: String,
    pub trial: String,
    pub sha: String,
    pub models: BTreeSet<String>,
    /// The skill is where the arm puts it: present in `with`, absent in `without`, filler in `filler`.
    pub loaded: bool,
    /// The run made at least one `bp` call: only such a run is scored on adherence.
    pub reached: bool,
    pub steps: [bool; 5],
    pub out_of_row: usize,
    pub done: bool,
    pub hits: usize,
    pub truth: usize,
    /// Each finding's location, its text and whether it matches the ground truth.
    pub findings: Vec<(String, String, bool)>,
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
    hash: String,
    fixtures: Vec<Fixture>,
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
            let fixture = rest.rsplit_once('-').context("a trial is <arm>-<fixture>-<n>")?.0;
            let fx = fixtures_all
                .iter()
                .find(|f| f.id == fixture)
                .with_context(|| format!("{trial}: no fixture {fixture} in {}", fixtures.display()))?;
            let path = store.join(&name);
            let trace = Trace::parse(&std::fs::read_to_string(&path)?);
            let log = std::fs::read_to_string(store.join(format!("{trial}.vault.log"))).unwrap_or_default();
            let loaded = match arm {
                "with" => trace.mentions(&marker),
                "without" => !trace.mentions(&marker) && !trace.mentions(FILLER),
                "filler" => trace.mentions(FILLER) && !trace.mentions(&marker),
                _ => bail!("{trial}: arm {arm} is none of with, without, filler"),
            };
            let mut graded = Graded {
                arm: arm.into(),
                trial: trial.clone(),
                sha: sha256(&path)?,
                models: trace.models(),
                loaded,
                ..Graded::default()
            };
            score(&mut graded, skill, fx, &trace, &log);
            trials.push(graded);
        }
        let role = std::fs::read_to_string(root.join(format!("bundle/roles/{}.md", skill.role_file())))?;
        let hash = hash_object(&format!("{role}{body}"))?;
        let holdout = crate::eval_fixture::holdout(fixtures).len();
        Ok(Self { skill: skill.clone(), trials, holdout, hash, fixtures: fixtures_all })
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
            format!("hash: {}", self.hash),
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
            "| trial | sha256 |\n|---|---|".to_string(),
        ];
        lines.extend(self.trials.iter().map(|t| format!("| {} | {} |", t.trial, t.sha)));
        lines.join("\n") + "\n"
    }

    /// The report the owner reads at the walkthrough: per arm, then per trial with a pointer to the raw trace.
    pub fn report(&self, store: &Path) -> String {
        let mut lines =
            vec![format!("# Eval report: {}\n", self.skill.name), format!("Traces: `{}`\n", store.display())];
        lines.push("## Ground truth\n".into());
        for f in &self.fixtures {
            lines.push(format!("Fixture {} (base `{}`, head `{}`):\n", f.id, short(&f.base), short(&f.head)));
            lines.extend(f.truth.iter().map(|t| format!("- `{}`: {}", t.location(), t.what)));
            lines.push(String::new());
        }
        lines.push("## Arms\n".into());
        lines.push("| arm | runs | outcome | reached the tool | adherence there | out of row | findings | false | skill loaded |".into());
        lines.push("|---|---|---|---|---|---|---|---|---|".into());
        for arm in ["with", "without", "filler"] {
            let runs = self.arm(arm);
            let s = Stats::of(&runs);
            if s.runs > 0 {
                lines.push(format!(
                    "| {arm} | {} | {} of {} | {} | {} | {} | {} | {} | {} |",
                    s.runs,
                    s.passes,
                    s.runs,
                    s.reached,
                    s.adherence.map_or("n/a".into(), f2),
                    s.out_of_row,
                    s.found,
                    s.wrong,
                    s.loaded
                ));
            }
        }
        lines.push(format!("\nA run passes when `bp walk --for review --done` holds after the review is recorded and it finds at least {RECALL} of the ground truth. Uplift is the `with` rate less the `without` rate; the result file gives its Newcombe 95% interval.\n"));
        lines.push("## Trials".into());
        for t in &self.trials {
            let steps: Vec<String> =
                STEPS.iter().zip(t.steps).map(|(n, ok)| format!("{n} {}", if ok { "yes" } else { "no" })).collect();
            let yes = |b: bool| if b { "yes" } else { "no" };
            lines.push(format!(
                "\n### {t}\n\n- outcome: {}; recorded: {}; ground truth found: {} of {}\n- reached the tool: {}; steps: {}\n- out of row: {}; skill loaded as designed: {}\n- trace: `{dir}/{t}.jsonl`, vault log: `{dir}/{t}.vault.log`",
                if t.passed() { "pass" } else { "fail" },
                yes(t.done),
                t.hits,
                t.truth,
                yes(t.reached),
                steps.join(", "),
                t.out_of_row,
                if t.loaded { "yes" } else { "NO" },
                t = t.trial,
                dir = store.display(),
            ));
            for (at, what, ok) in &t.findings {
                let what: String = what.chars().take(160).collect();
                lines.push(format!("  - {} `{at}`: {what}", if *ok { "match" } else { "FALSE" }));
            }
        }
        lines.join("\n") + "\n"
    }
}

/// One arm's trials, counted.
struct Stats {
    runs: usize,
    passes: usize,
    reached: usize,
    /// The mean share of steps done, over the runs that reached the tool; `None` when none did.
    adherence: Option<f64>,
    out_of_row: usize,
    found: usize,
    wrong: usize,
    loaded: usize,
}

impl Stats {
    fn of(runs: &[&Graded]) -> Self {
        let reached: Vec<&&Graded> = runs.iter().filter(|r| r.reached).collect();
        let found: Vec<&(String, String, bool)> = runs.iter().flat_map(|r| &r.findings).collect();
        Self {
            runs: runs.len(),
            passes: runs.iter().filter(|r| r.passed()).count(),
            reached: reached.len(),
            adherence: (!reached.is_empty()).then(|| {
                reached.iter().map(|r| r.adherence()).sum::<f64>()
                    / f64::from(u32::try_from(reached.len()).unwrap_or(u32::MAX))
            }),
            out_of_row: runs.iter().map(|r| r.out_of_row).sum(),
            found: found.len(),
            wrong: found.iter().filter(|f| !f.2).count(),
            loaded: runs.iter().filter(|r| r.loaded).count(),
        }
    }
}

/// `k` over `n` for counts of trials, which are far below the range where a float is inexact.
fn share(k: usize, n: usize) -> f64 {
    f64::from(u32::try_from(k).unwrap_or(u32::MAX)) / f64::from(u32::try_from(n).unwrap_or(u32::MAX))
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
    g.hits = fx.hits(&findings);
    g.truth = fx.truth.len();
    g.findings = findings.iter().map(|f| (f.location.clone(), f.text.clone(), fx.matches(f))).collect();
}

/// The owner's lines of a result file: kept across a regrade.
pub struct Owner {
    walkthrough: String,
    status: String,
}

impl Owner {
    /// The values in `existing`, or the starting ones: `pending` and `capability`. `Err` names a line the owner broke.
    pub fn of(existing: Option<&str>) -> Result<Self> {
        let get = |key: &str| existing.and_then(|t| t.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(": ")));
        let (walkthrough, status) = (get("walkthrough").unwrap_or("pending"), get("status").unwrap_or("capability"));
        if !STATUSES.contains(&status) {
            bail!("status: {status} is none of {}", STATUSES.join(", "));
        }
        if walkthrough.trim().is_empty() {
            bail!("walkthrough: holds nothing; the owner writes `approved` or the friction");
        }
        Ok(Self { walkthrough: walkthrough.into(), status: status.into() })
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
    let (p, n, z2) = (share(k, n), f64::from(u32::try_from(n).unwrap_or(u32::MAX)), 1.96_f64 * 1.96);
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
        assert_eq!((o.walkthrough.as_str(), o.status.as_str()), ("pending", "capability"));
        assert!(Owner::of(Some("status: keep\n")).is_err());
    }

    #[test]
    fn differences_name_the_line() {
        assert!(differences("a\nb\n", "a\nb\n").is_empty());
        let d = differences("a\nn: 5\n", "a\nn: 9\nextra\n");
        assert_eq!(d.len(), 2);
        assert!(d[0].starts_with("line 2") && d[0].contains("`n: 5`") && d[0].contains("`n: 9`"));
    }
}
