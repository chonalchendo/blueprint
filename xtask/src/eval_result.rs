//! The result file of a grade (29, design change 2; 30, design change 2) and the owner's two lines in it. A past
//! review's set keeps 29's lines; a planted set lists every arm graded and its kill lines (`eval_kills`): `kills:` and its genres are `with`'s kills. The grader
//! refuses a `capability` that the interval does not carry.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use bp_operation::Action;

use crate::eval_grade::{Grade, Stats, float, share};

const STATUSES: [&str; 4] = ["pending", "capability", "regression", "fold"];

impl Grade {
    /// The lower bound of `uplift_ci`, when both arms ran.
    pub fn uplift_low(&self) -> Option<f64> {
        let (with, without) = (Stats::of(&self.arm("with")), Stats::of(&self.arm("without")));
        (with.runs > 0 && without.runs > 0)
            .then(|| newcombe((with.passes, with.runs), (without.passes, without.runs)).1)
    }

    /// The graded arms: `with`, `old`, `without`, `filler`, then the rest as the trials name them.
    pub fn arms(&self) -> Vec<&str> {
        let ran = |a: &&str| self.trials.iter().any(|t| t.arm == *a);
        let mut arms: Vec<&str> = ["with", "old", "without", "filler"].into_iter().filter(ran).collect();
        for t in &self.trials {
            if !arms.contains(&t.arm.as_str()) {
                arms.push(&t.arm);
            }
        }
        arms
    }

    /// Why the owner's `status:` cannot stand: `capability` needs the lower bound of the interval above 0,
    /// `holdout_gain`'s for a planted set and `uplift_ci`'s otherwise.
    pub fn refusal(&self, owner: &Owner) -> Option<String> {
        if self.kill_set() {
            owner.refuses(self.holdout_gain().map(|g| g.1), "holdout_gain")
        } else {
            owner.refuses(self.uplift_low(), "uplift_ci")
        }
    }

    /// The result file: the numbers recomputed, then the owner's two lines as `keep` gives them. A planted set
    /// (30, design change 2) lists every arm graded and its kill lines; a past review's set keeps 29's lines.
    pub fn result(&self, keep: &Owner) -> String {
        let (with, without) = (Stats::of(&self.arm("with")), Stats::of(&self.arm("without")));
        let rate = |k: usize, n: usize| if n == 0 { "n/a".to_string() } else { f2(share(k, n)) };
        let kill = self.kill_set();
        let arms: Vec<&str> = if kill {
            self.arms()
        } else {
            [("with", &with), ("without", &without)].iter().filter(|a| a.1.runs > 0).map(|a| a.0).collect()
        };
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
        ];
        for arm in self.arms() {
            if let Some(sha) = self.trials.iter().find(|t| t.arm == arm).and_then(|t| t.body_ref.as_ref()) {
                lines.extend([format!("{arm}_ref: {sha}"), format!("{arm}_hash: {}", self.refs[sha])]);
            }
        }
        lines.push(format!(
            "model: {}",
            if models.is_empty() {
                "unknown".into()
            } else {
                models.into_iter().cloned().collect::<Vec<_>>().join(", ")
            }
        ));
        if kill {
            lines.extend(self.kill_lines());
        } else {
            lines.extend([
                format!("n: {}", with.runs.min(without.runs)),
                format!("baseline: {}", rate(without.passes, without.runs)),
                format!("with: {}", rate(with.passes, with.runs)),
                format!("uplift: {}", ci.map_or("n/a".into(), |c| f2(c.0))),
                format!("uplift_ci: {}", ci.map_or("n/a".into(), |c| format!("[{}, {}]", f2(c.1), f2(c.2)))),
            ]);
        }
        lines.extend([
            format!("adherence: {}", with.adherence.map_or("n/a".into(), f2)),
            format!("out_of_row: {}", with.out_of_row),
        ]);
        if !kill {
            let review = self.skill.action == Action::Review;
            lines.push(format!("false_findings: {}", if review { rate(with.wrong, with.found) } else { "n/a".into() }));
        }
        lines.extend([
            format!("walkthrough: {}", keep.walkthrough),
            format!("status: {}", keep.status),
            "\n## Trials\n".to_string(),
            "| trial | sha256 | vault log sha256 |\n|---|---|---|".to_string(),
        ]);
        lines.extend(self.trials.iter().map(|t| format!("| {} | {} | {} |", t.trial, t.sha, t.log_sha)));
        lines.join("\n") + "\n"
    }
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

    /// `capability` says the skill adds something; an interval whose lower bound, as the file prints it, is not
    /// above 0 says otherwise (29, decision 7; 30, design change 2). `line` names the interval's line.
    pub fn refuses(&self, low: Option<f64>, line: &str) -> Option<String> {
        let clear = low.is_some_and(|lo| lo >= 0.005);
        (self.status == "capability" && !clear).then(|| {
            format!(
                "status: capability, but the lower bound of {line} is {}, not above 0; the owner decides regression or fold",
                low.map_or("n/a".into(), f2)
            )
        })
    }
}

/// The 95% interval of a difference of two proportions (Newcombe's hybrid score): `(difference, low, high)`. A Wald
/// interval has no width at 0 or 1, and five trials per arm are often there.
pub(crate) fn newcombe((k1, n1): (usize, usize), (k0, n0): (usize, usize)) -> (f64, f64, f64) {
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
        let refuses = |o: &Owner, low: Option<f64>| o.refuses(low, "uplift_ci");
        assert!(refuses(&capability, Some(-0.1)).is_some_and(|r| r.contains("is -0.10, not above 0")));
        assert!(refuses(&capability, None).is_some() && refuses(&capability, Some(0.004)).is_some());
        assert!(refuses(&capability, Some(0.39)).is_none() && refuses(&o, None).is_none());
    }

    #[test]
    fn differences_name_the_line() {
        assert!(differences("a\nb\n", "a\nb\n").is_empty());
        let d = differences("a\nn: 5\n", "a\nn: 9\nextra\n");
        assert_eq!(d.len(), 2);
        assert!(d[0].starts_with("line 2") && d[0].contains("`n: 5`") && d[0].contains("`n: 9`"));
    }
}
