//! The kill grade of a planted set (30, design change 2): a set whose fixtures carry `kind:`. A row kills a planted
//! item when it points into the item's span and carries evidence; a control's rows are false unless a `known:` entry
//! covers them (`eval_grade::verdicts`). The unit of the result is the planted item, not the trial: an arm kills an
//! item when more than half of its trials on that fixture killed it, and `holdout_gain` compares the `with` and `old`
//! arms over the items.

use crate::eval_grade::{Grade, Stats, Verdict, float};
use crate::eval_result::{f2, newcombe};
use crate::eval_score::Graded;

/// The first words of a command in backticks: a row that names one shows its reproduction.
const PROGRAMS: [&str; 26] = [
    "awk", "bash", "bp", "cargo", "cat", "cd", "cmp", "diff", "echo", "env", "find", "git", "grep", "jq", "just",
    "mkdir", "printf", "python3", "rg", "sed", "sh", "tar", "test", "timeout", "wc", "xargs",
];

/// Whether a row's text carries evidence (30, design change 3): a command in backticks, or a design or convention
/// id. A command is a backtick span of two words or more in which a word that opens a command is one of [`PROGRAMS`]
/// or a path (`./`, `/`, `target/`, `$`). An id is a vault path under `projects/…/designs/` or `shared/conventions/`,
/// or a numbered design cite: `(22)`, `(17, check 10)`, `design 21`.
pub fn evidence(text: &str) -> bool {
    // A word opens a command at the span's start, after `&&`, `||`, `|` or `;`, after an assignment, and inside `$(`.
    let command = |span: &str| {
        let mut open = true;
        for word in span.split_whitespace() {
            let word = word.trim_start_matches("$(");
            let assignment = word.split_once('=').is_some_and(|(k, _)| is_name(k));
            let path = ["./", "/", "target/", "$"].iter().any(|p| word.len() > p.len() && word.starts_with(p));
            if open && !assignment && (PROGRAMS.contains(&word) || path) {
                return span.split_whitespace().count() > 1;
            }
            open = assignment || matches!(word, "&&" | "||" | "|" | ";") || word.ends_with(';');
        }
        false
    };
    let spans = text.split('`').skip(1).step_by(2);
    let id = text.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_')));
        (w.starts_with("projects/") && w.contains("/designs/")) || w.starts_with("shared/conventions/")
    });
    spans.into_iter().any(command) || id || numbered(text)
}

fn is_name(word: &str) -> bool {
    word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A design cited by its two-digit number: `(22)`, `(17, …)` or `design 21`.
fn numbered(text: &str) -> bool {
    let lower = text.to_lowercase();
    let two = |rest: &str| {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        (digits.len() == 2).then(|| rest[2..].chars().next())
    };
    let cited = |opener: &str, ends: &[char]| {
        lower
            .match_indices(opener)
            .any(|(at, _)| two(&lower[at + opener.len()..]).is_some_and(|next| next.is_none_or(|c| ends.contains(&c))))
    };
    cited("(", &[')', ',']) || cited("design ", &[' ', ')', ',', '.', ';', ':'])
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

    /// The kill lines of the result file, for the `with` arm.
    pub(crate) fn kill_lines(&self) -> Vec<String> {
        let planted = self.planted();
        let (killed, n) = self.kills("with");
        let mut lines = vec![format!("mutants: {n}"), format!("kills: {killed}/{n}")];
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
    fn evidence_is_a_command_or_a_design_id() {
        for yes in [
            "`cargo test -p xtask -- eval_trace` fails: 0 calls",
            "`T=$(mktemp -d) && git archive HEAD | tar -x -C $T` then the probe",
            "`./target/debug/bp walk x --json` exits 2",
            "breaks projects/bp/designs/system, the check of 10",
            "against shared/conventions/convention.testing, item 6",
            "the reader must count it (30, design change 5)",
            "the row of design 21 says so",
            "the cap (22)",
        ] {
            assert!(evidence(yes), "{yes}");
        }
        for no in [
            "the span `drill_box` returns the wrong width",
            "`n.rect.right() - 12 / w` is wrong",
            "`cargo` is not run",
            "line 113 subtracts where it should divide (12 lines)",
            "the count is wrong",
            "in (2026) it changed",
        ] {
            assert!(!evidence(no), "{no}");
        }
    }
}
