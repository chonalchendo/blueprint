//! The kill grade of a planted set (30, design change 2): a set whose fixtures carry `kind:`. A row kills a planted
//! item when it points into the item's span and carries evidence; a control's rows are false unless a `known:` entry
//! covers them (`eval_grade::verdicts`). The unit of the result is the planted item, not the trial: an arm kills an
//! item when more than half of its trials on that fixture killed it, and `holdout_gain` compares the `with` and `old`
//! arms over the items.

use std::collections::BTreeMap;
use std::path::Path;

use crate::eval_grade::{Grade, Stats, Verdict, float};
use crate::eval_result::{f2, newcombe};
use crate::eval_score::Graded;

/// What a row's evidence is checked against: the trial's own Bash commands, and the text of each numbered design by
/// its number (`NN-*.md` under `$BP_REDESIGN`), lowercased.
pub struct Proof<'a> {
    pub commands: Vec<&'a str>,
    pub designs: &'a BTreeMap<String, String>,
}

/// The numbered designs under `dir`: `NN-*.md`, by `NN`, lowercased. No directory holds none.
pub fn designs(dir: Option<&Path>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for entry in dir.and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let number =
            name.split_once('-').map(|n| n.0).filter(|n| n.len() == 2 && n.bytes().all(|b| b.is_ascii_digit()));
        if let (Some(n), true) = (number, Path::new(&name).extension().is_some_and(|e| e == "md")) {
            out.insert(n.to_string(), std::fs::read_to_string(entry.path()).unwrap_or_default().to_lowercase());
        }
    }
    out
}

/// Whether a row's text carries evidence (30, design changes 2 and 3): a command the trial ran, or a design or
/// convention id. A command is a backtick span of two words or more that one of the trial's Bash commands holds, so a
/// command the reviewer only advises is no reproduction. An id is a vault path under `projects/…/designs/` or
/// `shared/conventions/`, `design NN`, or `(NN, <words>)` where design `NN` exists and holds the words.
pub fn evidence(text: &str, proof: &Proof) -> bool {
    let ran = |span: &str| {
        let span = span.trim();
        span.split_whitespace().nth(1).is_some() && proof.commands.iter().any(|c| c.contains(span))
    };
    let id = text.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_')));
        (w.starts_with("projects/") && w.contains("/designs/")) || w.starts_with("shared/conventions/")
    });
    text.split('`').skip(1).step_by(2).any(ran) || id || numbered(text, proof.designs)
}

/// A design cited by its two-digit number: `design 21`, or `(17, check 10)` where design 17 holds `check 10` or a
/// heading word that starts with `check`. So `(12, not 9)` names no section of design 12.
fn numbered(text: &str, designs: &BTreeMap<String, String>) -> bool {
    let lower = text.to_lowercase();
    let two = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    let design = lower.match_indices("design ").any(|(at, _)| {
        let rest = &lower[at + "design ".len()..];
        rest.get(..2).is_some_and(two) && !rest[2..].starts_with(|c: char| c.is_alphanumeric())
    });
    let cite = lower.match_indices('(').any(|(at, _)| {
        let inner = &lower[at + 1..];
        let Some((number, words)) = inner.split_once(')').and_then(|(i, _)| i.split_once(", ")) else { return false };
        let words = words.trim();
        let first = words.split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric());
        let heading = |d: &str| {
            d.lines().filter(|l| l.starts_with('#')).flat_map(str::split_whitespace).any(|w| w.starts_with(first))
        };
        two(number)
            && first.starts_with(char::is_alphabetic)
            && designs.get(number).is_some_and(|d| d.contains(words) || heading(d))
    });
    design || cite
}

/// One planted item: a fixture, the item's position in its truth, and its genre.
struct Planted<'a> {
    fixture: &'a str,
    item: usize,
    genre: &'a str,
}

impl Grade {
    /// Whether the fixtures are a planted set: mutants, controls and escapes.
    pub fn kill_set(&self) -> bool {
        self.fixtures.iter().any(|f| f.kind.is_some())
    }

    fn planted(&self) -> Vec<Planted<'_>> {
        let mut out = Vec::new();
        for f in &self.fixtures {
            if let Some(genre) = f.genre() {
                out.extend((0..f.truth.len()).map(|item| Planted { fixture: &f.id, item, genre }));
            }
        }
        out
    }

    /// Whether more than half of `arm`'s trials on the item's fixture killed it. No trial kills nothing.
    fn killed(&self, arm: &str, p: &Planted) -> bool {
        let trials: Vec<&Graded> = self.arm(arm).into_iter().filter(|t| t.fixture == p.fixture).collect();
        let kills = trials.iter().filter(|t| t.killed.contains(&p.item)).count();
        2 * kills > trials.len()
    }

    /// The planted items `arm` killed, and the planted items.
    pub fn kills(&self, arm: &str) -> (usize, usize) {
        let planted = self.planted();
        (planted.iter().filter(|p| self.killed(arm, p)).count(), planted.len())
    }

    /// False rows per control trial of `arm`; `None` when it ran no control.
    pub fn control_false(&self, arm: &str) -> Option<f64> {
        let control = |t: &&Graded| self.fixture(&t.fixture).kind.as_deref() == Some("control");
        let controls: Vec<&Graded> = self.arm(arm).into_iter().filter(control).collect();
        (!controls.is_empty()).then(|| float(Stats::of(&controls).wrong) / float(controls.len()))
    }

    /// `with`'s kills less `old`'s over the planted items, with the Newcombe 95% interval of the difference: `None`
    /// until both arms ran.
    pub fn holdout_gain(&self) -> Option<(f64, f64, f64)> {
        let ((with, n), (old, _)) = (self.kills("with"), self.kills("old"));
        let ran = |arm: &str| !self.arm(arm).is_empty();
        (ran("with") && ran("old") && n > 0).then(|| newcombe((with, n), (old, n)))
    }

    /// The kill lines of the result file. `mutants:` counts the items of `kind: mutant` fixtures and `planted:` every
    /// planted item, escapes too; `kills:`, `kills_<genre>:`, `control_false:` and `duplicates:` are the `with` arm's,
    /// and `holdout_gain` is over every planted item.
    pub(crate) fn kill_lines(&self) -> Vec<String> {
        let planted = self.planted();
        let (killed, n) = self.kills("with");
        let mutants = planted.iter().filter(|p| self.fixture(p.fixture).kind.as_deref() == Some("mutant")).count();
        let mut lines = vec![format!("mutants: {mutants}"), format!("planted: {n}"), format!("kills: {killed}/{n}")];
        let mut genres: Vec<&str> = planted.iter().map(|p| p.genre).collect();
        genres.sort_unstable();
        genres.dedup();
        for genre in genres {
            let of: Vec<&Planted> = planted.iter().filter(|p| p.genre == genre).collect();
            let k = of.iter().filter(|p| self.killed("with", p)).count();
            lines.push(format!("kills_{genre}: {k}/{}", of.len()));
        }
        lines.push(format!("control_false: {}", self.control_false("with").map_or("n/a".into(), f2)));
        lines.push(format!("duplicates: {}", Stats::of(&self.arm("with")).duplicates));
        let gain = self.holdout_gain();
        lines.push(format!(
            "holdout_gain: {}",
            gain.map_or("n/a".into(), |(d, lo, hi)| format!("{} [{}, {}]", f2(d), f2(lo), f2(hi)))
        ));
        lines
    }
}

/// The verdict a row earned, as the report prints it.
pub fn label(v: Verdict) -> String {
    match v {
        Verdict::Match(i) => format!("match, truth item {}", i + 1),
        Verdict::Duplicate(i) => format!("duplicate, truth item {}", i + 1),
        Verdict::Known(i) => format!("known, entry {}", i + 1),
        Verdict::False => "FALSE".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_is_a_command_the_trial_ran_or_a_design_id() {
        let designs: BTreeMap<String, String> = [
            ("30", "## design change\n5. **the reader defects**"),
            ("17", "## checks this design carries\n10. the eval"),
            ("12", "## responsibility\nthe vault container"),
        ]
        .map(|(n, t)| (n.to_string(), t.to_string()))
        .into();
        let commands = vec![
            "cd /w && cargo test -p xtask -- eval_trace 2>&1 | tail -5",
            "T=$(mktemp -d) && git archive HEAD | tar -x -C $T && cd $T",
            "./target/debug/bp walk x --json; echo $?",
            "head -40 crates/bp-diagram/src/svg.rs",
        ];
        let proof = Proof { commands, designs: &designs };
        for yes in [
            "`cargo test -p xtask -- eval_trace` fails: 0 calls",
            "`T=$(mktemp -d) && git archive HEAD | tar -x -C $T` then the probe",
            "`./target/debug/bp walk x --json` exits 2",
            "`head -40 crates/bp-diagram/src/svg.rs` shows the divide",
            "breaks projects/bp/designs/system, the check of 10",
            "against shared/conventions/convention.testing, item 6",
            "the reader must count it (30, design change 5)",
            "the eval of (17, check 10) says so",
            "the row of design 21 says so",
        ] {
            assert!(evidence(yes, &proof), "{yes}");
        }
        for no in [
            "the span `drill_box` returns the wrong width",
            "`n.rect.right() - 12 / w` is wrong",
            "`cargo` is not run",
            "Run `cargo test -p bp-diagram` to see it fail.",
            "line 113 subtracts where it should divide (12 lines)",
            "the count is wrong",
            "in (2026) it changed",
            "The width is off by one (12, not 9).",
            "The drill box shrinks (10) pixels too far.",
            "`$width - 12` divides here",
            "the cap (22)",
            "the designer 21 times",
            "(31, check 10) names no design",
        ] {
            assert!(!evidence(no, &proof), "{no}");
        }
    }
}
