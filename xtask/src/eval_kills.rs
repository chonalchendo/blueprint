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
/// convention id. A command is a backtick span of two words or more that opens a command the trial ran: the whole
/// command, or it with trailing arguments trimmed. So a command the reviewer only advises is no reproduction, and nor
/// is the tail of one. A read ([`READS`]) reproduces nothing. An id is a vault path under `projects/…/designs/` or
/// `shared/conventions/`, or a numbered cite ([`numbered`]).
pub fn evidence(text: &str, proof: &Proof) -> bool {
    let ran = |span: &str| {
        let span = span.trim();
        let words: Vec<&str> = span.split_whitespace().take(2).collect();
        let read = READS.iter().any(|r| r.split(' ').eq(words.iter().copied().take(r.split(' ').count())));
        words.len() == 2 && !span.starts_with('-') && !read && proof.commands.iter().any(|c| opens(c, span))
    };
    let id = text.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_')));
        (w.starts_with("projects/") && w.contains("/designs/")) || w.starts_with("shared/conventions/")
    });
    text.split('`').skip(1).step_by(2).any(ran) || id || numbered(text, proof.designs)
}

/// Programs that only show what is there: a span that runs one reproduces no defect.
const READS: [&str; 12] =
    ["git diff", "git log", "git show", "cat", "ls", "head", "tail", "grep", "sed", "rg", "find", "wc"];

/// Words a section name can open with that name no section.
const STOP: [&str; 9] = ["the", "a", "an", "of", "in", "on", "its", "this", "not"];

/// Whether `span` opens a command of `command`, after its start or a `;`, `&`, `|` or newline, past the assignments
/// and keywords the trace reader skips ([`crate::eval_trace::program_at`]), and ends at a word.
fn opens(command: &str, span: &str) -> bool {
    let starts = command.char_indices().filter(|(_, c)| matches!(c, ';' | '&' | '|' | '\n')).map(|(i, _)| i + 1);
    std::iter::once(0).chain(starts).any(|i| {
        let segment = &command[i..];
        segment[crate::eval_trace::program_at(segment)..].strip_prefix(span).is_some_and(|rest| {
            rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|'))
        })
    })
}

/// A design cited by its two-digit number, where `NN-*.md` exists: `design 21`, or `(17, check 10)`. In the second
/// form the first word of the section that is not a [`STOP`] word must start a heading word of the design, or be a
/// word of its text. So `(13, the old value)` and `(12, not 9)` name no section.
fn numbered(text: &str, designs: &BTreeMap<String, String>) -> bool {
    let lower = text.to_lowercase();
    let two = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    let design = lower.match_indices("design ").any(|(at, _)| {
        let rest = &lower[at + "design ".len()..];
        rest.get(..2).is_some_and(|n| two(n) && designs.contains_key(n))
            && !rest[2..].starts_with(|c: char| c.is_alphanumeric())
    });
    let cite = lower.match_indices('(').any(|(at, _)| {
        let inner = &lower[at + 1..];
        let Some((number, words)) = inner.split_once(')').and_then(|(i, _)| i.split_once(", ")) else { return false };
        let mut cited = words.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()));
        let Some(first) = cited.find(|w| !w.is_empty() && !STOP.contains(w)) else { return false };
        let heading = |d: &str| {
            let heads = d.lines().filter(|l| l.starts_with('#')).flat_map(str::split_whitespace);
            heads.map(|w| w.trim_start_matches(|c: char| !c.is_alphanumeric())).any(|w| w.starts_with(first))
        };
        two(number)
            && first.starts_with(char::is_alphabetic)
            && designs.get(number).is_some_and(|d| heading(d) || crate::eval_fixture::has_word(d, first))
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
            ("13", "## responsibility\nthe vault holds nodes"),
            ("15", "## interfaces\nthe tool container"),
            ("21", "## actions, one row each"),
        ]
        .map(|(n, t)| (n.to_string(), t.to_string()))
        .into();
        let commands = vec![
            "cd /w && cargo test -p xtask -- eval_trace 2>&1 | tail -5",
            "T=$(mktemp -d) && git archive HEAD | tar -x -C $T && cd $T",
            "./target/debug/bp walk x --json; echo $?",
            "head -40 crates/bp-diagram/src/svg.rs",
            "cd $H && T=$D/target; CARGO_TARGET_DIR=$T cargo xtask sandbox claim > ../sb.txt 2>&1; echo $?",
        ];
        let proof = Proof { commands, designs: &designs };
        for yes in [
            "`cargo test -p xtask -- eval_trace` fails: 0 calls",
            "`T=$(mktemp -d) && git archive HEAD | tar -x -C $T` then the probe",
            "`./target/debug/bp walk x --json` exits 2",
            "`cargo xtask sandbox claim` exits 1 with the inverted check",
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
            "`head -40 crates/bp-diagram/src/svg.rs` shows the divide",
            "It returns the old value (13, the old value).",
            "The bound is off (15, a typo).",
            "see design 99 for this",
        ] {
            assert!(!evidence(no, &proof), "{no}");
        }
        let diffed = Proof {
            commands: vec!["git diff e5d8973 fc5178f -- crates/bp-diagram", "cargo test -p bp-diagram drill"],
            designs: &designs,
        };
        assert!(evidence("`cargo test -p bp-diagram` fails: 3 for 9", &diffed), "trailing arguments trimmed");
        for no in ["`git diff e5d8973 fc5178f` shows `w / 2`", "`-- crates/bp-diagram` holds the divide"] {
            assert!(!evidence(no, &diffed), "{no}");
        }
    }
}
