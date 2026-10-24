//! Grades the stored trials of a skill (29, design change 2). Code only: it reads each trial's transcript and vault
//! log from the store, the fixture and the bundle, and it recomputes every number of the result file. `--grade`
//! writes the file and the report. `--grade --check` compares the file with the recomputation, so a number typed by
//! hand, a swapped transcript or log, a dropped trial or an edited skill shows. The owner's two lines, `walkthrough:`
//! and `status:`, are kept.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use bp_operation::Action;

use crate::eval::Skill;
use crate::eval_arms::{FILLER, PROTOCOL, body_of};
use crate::eval_fixture::Fixture;
use crate::eval_score::{Graded, score};
use crate::eval_trace::{Trace, sha256};

const STATUSES: [&str; 4] = ["pending", "capability", "regression", "fold"];

pub struct Grade {
    pub skill: Skill,
    pub trials: Vec<Graded>,
    holdout: usize,
    /// The hash of the role file and skill body as the files hold them now.
    current: String,
    pub fixtures: Vec<Fixture>,
}

/// The hash of the role file and the bodies of every skill that role preloads for the measured one,
/// `git hash-object --stdin`: the key of check 10. For `bp-review` that is `bp-reviewer.md` and `bp-review`'s body;
/// for both worker skills it is `bp-worker.md`, then `bp-complete`'s body, then `bp-respond`'s, because the `with`
/// arm of each preloads both and a sibling edit changes what was measured. A run writes the hash beside its
/// transcript, and a grade compares the traces' hash with the files'.
pub fn bundle_hash(root: &Path, skill: &Skill) -> Result<String> {
    let mut text = std::fs::read_to_string(root.join(format!("bundle/roles/{}.md", skill.role_file())))?;
    for name in skill.preloaded() {
        text += &std::fs::read_to_string(root.join("bundle/skills").join(&name).join("SKILL.md"))
            .with_context(|| format!("read the body of {name}"))?;
    }
    hash_object(&text)
}

/// What a trial saw besides the bundle: the fixture's base, head, title, issue, rules and stack, and the arm
/// protocol. The truth, the match words and the probe are left out, so a regrade after a grader fix keeps its trials.
pub(crate) fn stamp(fx: &Fixture) -> Result<String> {
    let seen = format!("{}\n{}\n{}\n{}\n{:?}\n{:?}", fx.base, fx.head, fx.title, fx.issue, fx.rules, fx.stack);
    Ok(format!("{} {PROTOCOL}", hash_object(&seen)?))
}

/// A trial's `.hash` file: the bundle hash, then `stamp: <stamp>`. A file from before the stamp has an empty one.
pub(crate) fn read_hash(store: &Path, trial: &str) -> Option<(String, String)> {
    let text = std::fs::read_to_string(store.join(format!("{trial}.hash"))).ok()?;
    let mut lines = text.lines();
    let hash = lines.next()?.trim().to_string();
    Some((hash, lines.find_map(|l| l.strip_prefix("stamp: ")).unwrap_or_default().to_string()))
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
                "without" => !trace.mentions(&marker) && !trace.mentions(FILLER),
                "filler" => trace.preloaded(FILLER) && !trace.mentions(&marker),
                // `with`, or an arm that ran with its own `--bin`: an edited body keeps the body's first line.
                _ => trace.preloaded(&marker),
            };
            let (hash, stamp) = read_hash(store, &trial).unzip();
            let mut graded = Graded {
                arm: arm.into(),
                fixture: fixture.into(),
                number: number.parse().with_context(|| format!("{trial}: the trial number is a count"))?,
                trial: trial.clone(),
                sha: sha256(&path)?,
                log_sha: if log_path.is_file() { sha256(&log_path)? } else { String::new() },
                hash,
                stamp: stamp.unwrap_or_default(),
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
        let mut stamps = std::collections::BTreeMap::<&String, BTreeSet<&String>>::new();
        for t in &self.trials {
            stamps.entry(&t.fixture).or_default().insert(&t.stamp);
        }
        for (fixture, seen) in stamps.iter().filter(|s| s.1.len() > 1) {
            out.push(format!(
                "{fixture}: the trials ran on {} fixture or arm protocols: {seen:?}; move the store aside and rerun",
                seen.len()
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

    pub fn fixture(&self, id: &str) -> &Fixture {
        self.fixtures.iter().find(|f| f.id == id).expect("a graded trial names a loaded fixture")
    }

    pub fn arm(&self, arm: &str) -> Vec<&Graded> {
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
            format!(
                "false_findings: {}",
                if self.skill.action == Action::Review { rate(with.wrong, with.found) } else { "n/a".into() }
            ),
            format!("walkthrough: {}", keep.walkthrough),
            format!("status: {}", keep.status),
            "\n## Trials\n".to_string(),
            "| trial | sha256 | vault log sha256 |\n|---|---|---|".to_string(),
        ];
        lines.extend(self.trials.iter().map(|t| format!("| {} | {} | {} |", t.trial, t.sha, t.log_sha)));
        lines.join("\n") + "\n"
    }
}

/// One arm's trials, counted.
pub(crate) struct Stats {
    pub runs: usize,
    pub passes: usize,
    /// Ground-truth items named in the final texts, over all runs, and the items the runs could have named.
    pub prose: usize,
    pub truth: usize,
    pub reached: usize,
    /// The mean share of steps done, over the runs that reached the tool; `None` when none did.
    pub adherence: Option<f64>,
    pub out_of_row: usize,
    pub found: usize,
    pub wrong: usize,
    /// Runs whose table held no finding: a reviewer that returns nothing is not precise.
    pub silent: usize,
    pub loaded: usize,
}

impl Stats {
    pub fn of(runs: &[&Graded]) -> Self {
        let reached: Vec<&&Graded> = runs.iter().filter(|r| r.reached).collect();
        let found: Vec<&(String, String, Option<usize>)> = runs.iter().flat_map(|r| &r.findings).collect();
        Self {
            runs: runs.len(),
            passes: runs.iter().filter(|r| r.pass).count(),
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
pub(crate) fn float(n: usize) -> f64 {
    n as f64
}

/// `k` over `n`, for `n` above 0.
pub(crate) fn share(k: usize, n: usize) -> f64 {
    float(k) / float(n)
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

pub(crate) fn f2(x: f64) -> String {
    format!("{:.2}", if x.abs() < 0.005 { 0.0 } else { x })
}

pub(crate) fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// `git hash-object --stdin`: the hash of the role file and the skill body together, the key of check 10.
pub(crate) fn hash_object(text: &str) -> Result<String> {
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
