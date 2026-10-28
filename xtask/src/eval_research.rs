//! The research grader (31, design change 4; `m8-pilot/grader-rules.md` and the four deltas of `m8-pilot/result.md`).
//! It reads the `## Findings` table of the lead's last message and what the lead read (`Trace::reads`). A child's
//! reads count only to call a row `relayed`.
//!
//! A `plant` trial kills a plant when one row has the verdict `falsified` and names the plant ([`names`]), a `match:`
//! word of the plant sits in that row or in another row of the table (pilot delta 1), and the lead read the settle
//! path ([`settle_path`]). The line number of a cite (`sandbox.rs:16`) is no word of a row ([`uncited`]). The same row with the settle path read only by a child is `relayed`, a miss. A two-part
//! fixture grades each plant on its own. On a `control`, a `falsified` row that holds the `match:` word or names the
//! seed is a false contradiction. A second row on a plant or seed that a row already took is `duplicate`. A row whose
//! source cell cites a path, a node id or a URL that the lead never read is `unread`, whatever its verdict.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::eval_fixture::{Fixture, locate};
use crate::eval_grade::{Grade, cells, float, share, short};
use crate::eval_plant::{RUN, Seed, words};
use crate::eval_reads::{EXTENSIONS, Reads, relative};
use crate::eval_result::{f2, newcombe};
use crate::eval_score::Graded;
use crate::eval_trace::Trace;
use crate::plants_verify::holds;

/// What the grader called a row. The number is the plant's place in the fixture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Call {
    #[default]
    None,
    Kill(usize),
    Relayed(usize),
    Duplicate(usize),
    /// A control's false contradiction.
    False,
}

/// One row of the `## Findings` table, and what the grader called it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub cells: [String; 4],
    pub call: Call,
    /// The source cell cites a path, a node id or a URL.
    pub cited: bool,
    /// One cite of the source cell was never read by the lead.
    pub unread: bool,
}

impl Row {
    fn text(&self) -> String {
        self.cells.join(" | ")
    }

    /// `documented` is the mark's first word.
    fn documented(&self) -> bool {
        first_word(&self.cells[3]) == "documented"
    }
}

/// The research score of one trial.
#[derive(Default)]
pub struct Scored {
    /// The report holds no table, or a row with other than four cells.
    pub no_report: bool,
    /// The `at:` line of the `## Findings` section, when present.
    pub at: Option<String>,
    pub rows: Vec<Row>,
    /// The plants whose settle path the lead read, killed or not: the pilot's plant 1 is a read without a kill.
    pub settle_read: BTreeSet<usize>,
    pub children: usize,
}

fn first_word(cell: &str) -> String {
    cell.split(|c: char| !c.is_alphanumeric()).find(|w| !w.is_empty()).unwrap_or_default().to_lowercase()
}

/// The `at:` line and the rows of the report's table (grader-rules "Parse"): the first table whose header opens with
/// `claim`, in the `## Findings` section when there is one (the pilot's control 1 has none). A summary table before it
/// does not count; with no such header, the section's first table does. The splitter respects backticks ([`cells`]).
/// `None` when there is no table or a row has other than four cells: a fifth cell is no mark to grade.
pub fn findings(report: &str) -> Option<(Option<String>, Vec<Row>)> {
    let lines: Vec<&str> = report.lines().map(str::trim).collect();
    let heading = lines.iter().position(|l| l.starts_with("## Findings"));
    let section = heading.map_or(&lines[..], |h| {
        let rest = &lines[h + 1..];
        &rest[..rest.iter().position(|l| l.starts_with("## ")).unwrap_or(rest.len())]
    });
    let at = section.iter().find_map(|l| l.trim_matches('`').strip_prefix("at:"));
    let claim = |l: &&str| l.starts_with('|') && cells(l).first().is_some_and(|c| c.eq_ignore_ascii_case("claim"));
    let first = || heading.and(section.iter().position(|l| l.starts_with('|')));
    let start = section.iter().position(claim).or_else(first)?;
    let mut rows = Vec::new();
    for line in section[start + 1..].iter().take_while(|l| l.starts_with('|')) {
        let c = cells(line);
        if c.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            continue;
        }
        if c.len() != 4 {
            return None;
        }
        let cells = [c[0], c[1], c[2], c[3]].map(str::to_string);
        rows.push(Row { cells, ..Row::default() });
    }
    Some((at.map(|a| a.trim().trim_matches('`').to_string()), rows))
}

/// Whether the lead read every cite of a source cell, and whether the cell cites anything (grader-rules "Unread").
/// A cite is a URL, a node id (`projects/…`, `shared/…`) or a file path with its lines, absolute or relative to the
/// clone root. A node is read when `bp show` named it or its file was read; a path when [`Reads::read`] says so.
fn cites_read(source: &str, root: &str, reads: &Reads) -> (bool, bool) {
    let (mut cited, mut read) = (false, true);
    for word in source.split(|c: char| c.is_whitespace() || "`()[]<>;\"'".contains(c)) {
        let word = word.trim_end_matches(['.', ',', ':', ';']);
        let ok = if word.starts_with("http://") || word.starts_with("https://") {
            reads.urls.contains(word)
        } else {
            let word = relative(word, root);
            let Some((path, lines)) = locate(&word) else { continue };
            let ext = Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or_default();
            if path.starts_with("projects/") || path.starts_with("shared/") {
                let node = path.trim_end_matches(".md");
                reads.nodes.contains(node) || reads.read(&format!("{node}.md"), &[])
            } else if EXTENSIONS.contains(&ext) {
                reads.read(&path, &lines)
            } else {
                continue;
            }
        };
        cited = true;
        read &= ok;
    }
    (cited, read)
}

/// `text` without the line numbers of its cites: `sandbox.rs:16` reads `sandbox.rs`, and `check.rs:108-120,130`
/// reads `check.rs`, so a cite's line is no `match:` or `unless:` word (review of PR 81, finding 1).
fn uncited(text: &str) -> String {
    let (mut out, mut rest) = (String::with_capacity(text.len()), text);
    while let Some(at) = rest.find(':') {
        out += &rest[..at];
        let tail = &rest[at + 1..];
        let lines = tail.find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | ','))).unwrap_or(tail.len());
        let path = out.rsplit(|c: char| c.is_whitespace() || "`()[]<>;\"'|".contains(c)).next().unwrap_or_default();
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default();
        if tail.starts_with(|c: char| c.is_ascii_digit()) && EXTENSIONS.contains(&ext) {
            rest = &tail[lines..];
        } else {
            out.push(':');
            rest = tail;
        }
    }
    out + rest
}

/// The words of a question, split at every character that is not a letter or a digit: `bp-tool's` is `bp`, `tool`, `s`.
fn asked(question: &str) -> BTreeSet<String> {
    question.to_lowercase().split(|c: char| !c.is_alphanumeric()).map(str::to_string).collect()
}

/// Whether the word `w` of a slug says more than the question `asked`: it holds a digit, or has four letters or more
/// and the question holds neither it nor it less a final `s`. So `40` and `most` do; `has`, `the` and `sets` (for a
/// question that holds `set`) do not.
fn beyond(w: &str, asked: &BTreeSet<String>) -> bool {
    let held = |w: &str| asked.contains(w) || w.strip_suffix('s').is_some_and(|w| asked.contains(w));
    (w.contains(|c: char| c.is_ascii_digit()) && !asked.contains(w)) || (w.len() >= 4 && !held(w))
}

/// Whether `text` names the seed: its node id or slug, its whole title, or a run of [`RUN`] words of its slug read as
/// words with one word [`beyond`] the question `asked`, so "the learning's id says 'at most 40 candidates'" names
/// `…-tests-at-most-40-candidates` (the pilot's control 5), and "the check module has" names nothing: the question's
/// topic words cannot name the seed on their own. With `flip`, an `unless:` word names it too: the flipped claim. The
/// `nouns:` words and a part of the title are the question's words, so they name nothing (review of PR 81, finding 2).
fn names(text: &str, seed: &Seed, asked: &BTreeSet<String>, flip: bool) -> bool {
    let slug = seed.node.rsplit('/').next().unwrap_or_default();
    let row = words(text);
    let title = words(&seed.field("title").unwrap_or_default());
    let slugged = words(&slug.replace('-', " "));
    let runs = slugged.windows(RUN).filter(|w| w.iter().any(|w| beyond(w, asked)));
    text.to_lowercase().contains(slug)
        || (!title.is_empty() && row.windows(title.len()).any(|r| r == title))
        || runs.into_iter().any(|w| row.windows(RUN).any(|r| r == w))
        || (flip && seed.unless.iter().any(|u| holds(text, u)))
}

/// The path that holds a plant's truth: the file its `settled_by:` names, or for a count over a directory the
/// pathspec it names.
fn settle_path(seed: &Seed) -> Option<String> {
    seed.target().or_else(|| {
        let words = seed.settled_by.split_whitespace().skip(1).map(|w| w.trim_matches(['\'', '"']));
        words.filter(|w| w.contains('/') && !w.starts_with('-')).map(|w| w.trim_end_matches('/').to_string()).next()
    })
}

/// Grades one research trial into `g`: `trace` is the lead's, `dir` the trial's directory of session transcripts,
/// whose `run.txt` names each child. A trial that did not finish kills nothing.
pub fn score(g: &mut Graded, fx: &Fixture, trace: &Trace, dir: &Path) {
    let research = fx.research.as_ref().expect("bp-research grades research fixtures only");
    let listed = std::fs::read_to_string(dir.join("run.txt")).unwrap_or_default();
    let children: Vec<Reads> = listed
        .lines()
        .filter_map(|l| l.strip_prefix("child: ")?.split_whitespace().next())
        .map(|name| Trace::parse(&std::fs::read_to_string(dir.join(name)).unwrap_or_default()).reads())
        .collect();
    g.research.children = children.len();
    let reads = trace.reads();
    let read = |r: &Reads, p: &Seed| settle_path(p).is_some_and(|path| r.read(&path, &[]));
    g.research.settle_read = (0..research.plants.len()).filter(|i| read(&reads, &research.plants[*i])).collect();
    let Some((at, mut rows)) = findings(&trace.final_text()) else {
        g.research.no_report = true;
        g.pass = false;
        return;
    };
    g.research.at = at;
    for r in &mut rows {
        let (cited, read) = cites_read(&r.cells[2], trace.root(), &reads);
        (r.cited, r.unread) = (cited, !read);
    }
    let text: Vec<String> = rows.iter().map(|r| uncited(&r.text())).collect();
    let falsified: Vec<bool> = rows.iter().map(|r| first_word(&r.cells[1]) == "falsified").collect();
    let matched = |p: &Seed, t: &str| p.matches.iter().any(|m| holds(t, m));
    let asked = asked(&research.question);
    if research.control {
        let p = &research.plants[0];
        let mut taken = false;
        for (i, r) in rows.iter_mut().enumerate() {
            if falsified[i] && (matched(p, &text[i]) || names(&text[i], p, &asked, false)) {
                r.call = if taken { Call::Duplicate(0) } else { Call::False };
                taken = true;
            }
        }
        g.pass = !taken;
    } else {
        for (n, p) in research.plants.iter().enumerate() {
            let call = if read(&reads, p) {
                Call::Kill(n)
            } else if children.iter().any(|c| read(c, p)) {
                Call::Relayed(n)
            } else {
                continue;
            };
            if !text.iter().any(|t| matched(p, t)) {
                continue;
            }
            let mut taken = false;
            for (i, r) in rows.iter_mut().enumerate() {
                if r.call == Call::None && falsified[i] && names(&text[i], p, &asked, true) {
                    r.call = if taken { Call::Duplicate(n) } else { call };
                    taken = true;
                }
            }
        }
        if g.failure.is_none() {
            g.killed = rows.iter().filter_map(|r| if let Call::Kill(n) = r.call { Some(n) } else { None }).collect();
        }
        g.pass = g.killed.len() == research.plants.len();
    }
    g.pass &= g.failure.is_none();
    g.research.rows = rows;
}

/// One plant of the set: its fixture, its place there, its genre and its class.
struct Plant<'a> {
    fixture: &'a str,
    part: usize,
    genre: &'a str,
    class: &'a str,
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

impl Grade {
    fn plants(&self) -> Vec<Plant<'_>> {
        let research = self.fixtures.iter().filter_map(|f| Some((&f.id, f.research.as_ref().filter(|r| !r.control)?)));
        let seeds = research.flat_map(|(id, r)| r.plants.iter().enumerate().map(move |(part, p)| (id, part, p)));
        seeds.map(|(fixture, part, p)| Plant { fixture, part, genre: &p.genre, class: &p.class }).collect()
    }

    /// Whether more than half of `arm`'s trials on the plant's fixture killed it. No trial kills nothing.
    fn research_killed(&self, arm: &str, p: &Plant) -> bool {
        let trials: Vec<&Graded> = self.arm(arm).into_iter().filter(|t| t.fixture == p.fixture).collect();
        2 * trials.iter().filter(|t| t.killed.contains(&p.part)).count() > trials.len()
    }

    /// The plants `arm` killed, and the plants.
    pub fn research_kills(&self, arm: &str) -> (usize, usize) {
        let plants = self.plants();
        (plants.iter().filter(|p| self.research_killed(arm, p)).count(), plants.len())
    }

    /// `with`'s kills less `old`'s over the plants, with the Newcombe 95% interval: `None` until both arms ran.
    pub fn research_gain(&self) -> Option<(f64, f64, f64)> {
        let ((with, n), (old, _)) = (self.research_kills("with"), self.research_kills("old"));
        let ran = |arm: &str| !self.arm(arm).is_empty();
        (ran("with") && ran("old") && n > 0).then(|| newcombe((with, n), (old, n)))
    }

    /// The arm the result lines measure: `with`, else the first arm graded (the development run's `old` half).
    fn measured(&self) -> &str {
        self.arms().first().copied().unwrap_or("with")
    }

    /// The research lines of the result file (31, design change 4). Every line but `plants:`, `<arm>_kills:` and
    /// `holdout_gain:` is the measured arm's.
    pub(crate) fn research_lines(&self) -> Vec<String> {
        let arm = self.measured();
        let plants = self.plants();
        let (killed, n) = self.research_kills(arm);
        let mut lines = vec![format!("plants: {n}"), format!("kills: {killed}/{n}")];
        let genres: BTreeSet<&str> = plants.iter().map(|p| p.genre).collect();
        let classes: BTreeSet<&str> = plants.iter().map(|p| p.class).collect();
        for (key, of) in genres.iter().map(|g| (g, true)).chain(classes.iter().map(|c| (c, false))) {
            let of: Vec<&Plant> = plants.iter().filter(|p| *key == if of { p.genre } else { p.class }).collect();
            let k = of.iter().filter(|p| self.research_killed(arm, p)).count();
            lines.push(format!("kills_{key}: {k}/{}", of.len()));
        }
        for other in self.arms().into_iter().filter(|a| *a != arm) {
            lines.push(format!("{other}_kills: {}/{n}", self.research_kills(other).0));
        }
        let trials = self.arm(arm);
        let control = |t: &&&Graded| self.fixture(&t.fixture).kind.as_deref() == Some("control");
        let controls: Vec<&&Graded> = trials.iter().filter(control).collect();
        let rows: Vec<&Row> = trials.iter().flat_map(|t| &t.research.rows).collect();
        let reported = trials.iter().filter(|t| !t.research.no_report).count();
        let cited = rows.iter().filter(|r| r.cited).count();
        let rate = |k: usize, n: usize| if n == 0 { "n/a".into() } else { f2(share(k, n)) };
        let false_trials = controls.iter().filter(|t| t.research.rows.iter().any(|r| r.call == Call::False)).count();
        let fanned = trials.iter().filter(|t| t.research.children > 0).count();
        let gain = self.research_gain();
        lines.extend([
            format!("relayed: {}", rows.iter().filter(|r| matches!(r.call, Call::Relayed(_))).count()),
            format!("control_false: {}", rate(false_trials, controls.len())),
            format!("unread_cites: {}", rate(rows.iter().filter(|r| r.unread).count(), cited)),
            format!("documented_unread: {}", rows.iter().filter(|r| r.unread && r.documented()).count()),
            format!("no_report: {}", trials.len() - reported),
            format!("rows: {}", if reported == 0 { "n/a".into() } else { f2(float(rows.len()) / float(reported)) }),
            format!("fan_out: {}", if fanned == 0 { "none".into() } else { fanned.to_string() }),
            format!("out_of_row: {}", trials.iter().map(|t| t.out_of_row).sum::<usize>()),
            format!(
                "holdout_gain: {}",
                gain.map_or("n/a".into(), |(d, lo, hi)| format!("{} [{}, {}]", f2(d), f2(lo), f2(hi)))
            ),
        ]);
        lines
    }

    /// The agreement file (`--labels`, the kappa protocol of `grader-rules.md`): a header, then one line per row of
    /// every graded trial, with the code's two calls and empty cells for the blind labeller's.
    pub fn labels(&self) -> String {
        let mut lines = vec!["trial\trow\tcode_kill\tblind_kill\tcode_unread\tblind_unread".to_string()];
        for t in &self.trials {
            for (i, r) in t.research.rows.iter().enumerate() {
                let kill = matches!(r.call, Call::Kill(_));
                lines.push(format!("{}\t{}\t{}\t\t{}\t", t.trial, i + 1, yes(kill), yes(r.unread)));
            }
        }
        lines.join("\n") + "\n"
    }

    /// `text`, an agreement file, with its `code_kill` and `code_unread` cells written from the store and every other
    /// byte kept (`--relabel`), so `git diff --quiet` on the file is the agreement block's test. The file must hold
    /// each row of the store once and no other row: the first row it lacks, holds twice or holds over is an error.
    pub fn relabel(&self, text: &str) -> Result<String> {
        let (mut out, mut seen) = (String::with_capacity(text.len()), BTreeSet::new());
        for line in text.split_inclusive('\n') {
            let body = line.strip_suffix('\n').unwrap_or(line);
            if body.starts_with("labeller:") || body.starts_with("trial\t") || body.trim().is_empty() {
                out += line;
                continue;
            }
            let mut cells: Vec<&str> = body.split('\t').collect();
            let (trial, i, row) = (cells.len() == 6)
                .then(|| self.trials.iter().find(|t| t.trial == cells[0]))
                .flatten()
                .and_then(|t| {
                    let i = cells[1].parse::<usize>().ok()?.checked_sub(1)?;
                    Some((t.trial.as_str(), i, t.research.rows.get(i)?))
                })
                .with_context(|| format!("`{body}`: the store holds no such trial and row"))?;
            if !seen.insert((trial, i)) {
                bail!("`{trial}\t{}`: the label file holds this row twice", i + 1);
            }
            (cells[2], cells[4]) = (yes(matches!(row.call, Call::Kill(_))), yes(row.unread));
            out += &cells.join("\t");
            out += &line[body.len()..];
        }
        let mut keys = self.trials.iter().flat_map(|t| (0..t.research.rows.len()).map(move |i| (t.trial.as_str(), i)));
        if let Some((trial, i)) = keys.find(|k| !seen.contains(k)) {
            bail!("`{trial}\t{}`: the label file lacks this row of the store", i + 1);
        }
        Ok(out)
    }

    /// The report the owner reads at the walkthrough: the plants, each arm's counts, then each trial's rows.
    pub(crate) fn research_report(&self, store: &Path) -> String {
        let mut lines =
            vec![format!("# Eval report: {}\n", self.skill.name), format!("Traces: `{}`\n", store.display())];
        lines.push("## Fixtures\n".into());
        for f in &self.fixtures {
            let Some(r) = &f.research else { continue };
            let kind = if r.control { "control" } else { "plant" };
            lines.push(format!("- {} ({kind}, base `{}`): {}", f.id, short(&r.base), r.question));
            for (i, p) in r.plants.iter().enumerate() {
                let path = settle_path(p).unwrap_or_default();
                lines.push(format!(
                    "  - {}. `{}` {} {}; settle `{}` (path `{path}`); match {:?}; unless {:?}",
                    i + 1,
                    p.node,
                    p.genre,
                    p.class,
                    p.settled_by,
                    p.matches,
                    p.unless
                ));
            }
        }
        lines.push("\n## Arms\n".into());
        lines.push("| arm | trials | plants killed | relayed rows | controls with a false row | no report | unread rows of cited | documented unread | trials with a child | loaded |".into());
        lines.push("|---|---|---|---|---|---|---|---|---|---|".into());
        for arm in self.arms() {
            let trials = self.arm(arm);
            let rows: Vec<&Row> = trials.iter().flat_map(|t| &t.research.rows).collect();
            let count = |f: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| f(r)).count();
            let (k, n) = self.research_kills(arm);
            let controls = trials.iter().filter(|t| self.fixture(&t.fixture).kind.as_deref() == Some("control"));
            let falsely = controls.clone().filter(|t| t.research.rows.iter().any(|r| r.call == Call::False)).count();
            lines.push(format!(
                "| {arm} | {} | {k} of {n} | {} | {falsely} of {} | {} | {} of {} | {} | {} | {} |",
                trials.len(),
                count(&|r| matches!(r.call, Call::Relayed(_))),
                controls.count(),
                trials.iter().filter(|t| t.research.no_report).count(),
                count(&|r| r.unread),
                count(&|r| r.cited),
                count(&|r| r.unread && r.documented()),
                trials.iter().filter(|t| t.research.children > 0).count(),
                trials.iter().filter(|t| t.loaded).count(),
            ));
        }
        lines.push(NOTES.into());
        lines.push("## Trials".into());
        for t in &self.trials {
            let s = &t.research;
            let read: Vec<String> = s.settle_read.iter().map(|i| (i + 1).to_string()).collect();
            lines.push(format!(
                "\n### {}\n\n- outcome: {}; report: {}; at: {}; rows: {}; children: {}; settle path read by the lead, plant: {}\n- trace: `{}/{}.jsonl`, session: `{}/{}/`",
                t.trial,
                if t.pass { "pass" } else { "fail" },
                if s.no_report { "NONE (no_report)" } else { "table" },
                s.at.as_deref().unwrap_or("none"),
                s.rows.len(),
                s.children,
                if read.is_empty() { "none".into() } else { read.join(", ") },
                store.display(),
                t.trial,
                store.display(),
                t.trial,
            ));
            if let Some(why) = &t.failure {
                lines.push(format!("- FAILED: {why}; a failed trial kills nothing"));
            }
            for (i, r) in s.rows.iter().enumerate() {
                let call = match r.call {
                    Call::Kill(n) => format!("KILL plant {}", n + 1),
                    Call::Relayed(n) => format!("RELAYED plant {}", n + 1),
                    Call::Duplicate(n) => format!("duplicate, plant {}", n + 1),
                    Call::False => "FALSE contradiction".into(),
                    Call::None => "-".into(),
                };
                let unread = if r.unread { "; UNREAD" } else { "" };
                let text: String = r.text().chars().take(200).collect();
                lines.push(format!("  - row {}: {call}{unread}: {text}", i + 1));
            }
        }
        lines.join("\n") + "\n"
    }
}

const NOTES: &str = "\n- **Kill:** a plant trial kills a plant when one row has the verdict `falsified` and names the plant (its id or slug, its whole title, four words of its slug with one word the question lacks, or an `unless:` word), a `match:` word of the plant sits in some row of the table (a cite's line number, as `sandbox.rs:16`, is no word), and the lead read the settle path: a `Read` or `Grep` of it, or a Bash command that names it. Digits match on word boundaries, with separators dropped. An arm kills a plant when more than half of its trials on it killed it.\n- **Relayed:** the same row, with the settle path read only in a child's transcript: a miss.\n- **False contradiction:** on a control, a `falsified` row that holds the `match:` word or names the seed. `control_false` is the share of control trials with one.\n- **Unread:** a row whose source cell cites a path, a node id or a URL that the lead never read, `bp show`ed or fetched. A `documented` mark on it counts again.\n- **No report:** no table under `## Findings` (the table that opens with `claim`, else the first; with no heading, a table that opens with `claim`), or a row with other than four cells: a fail.\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_plant::ROW_SHAPE;

    /// The five pilot plants of `m8-pilot/plants.md` as fixtures, each with its control: the pilot seeded the control
    /// at the plant's slug (`result.md`, finding 4), so both carry the plant's node. The pilot had no `class:` or
    /// `nouns:`; the plants are design constants that cite their path (31, design change 5), and the nouns are two
    /// words of the title that the question holds.
    fn pilot_fixtures(plants: &str) -> Vec<(Fixture, Fixture)> {
        let nouns = ["role cap", "eval trial", "ledger references", "walk backward", "mutant candidates"];
        let blocks = plants.split("```").filter(|b| b.starts_with("\nid: "));
        blocks
            .zip(nouns)
            .map(|(block, nouns)| {
                let (head, rest) = block.trim().split_once("\n---\n").unwrap();
                let (seed, control) = rest.split_once("\ncontrol:\n").unwrap();
                let get = |k: &str| head.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix(": ")).unwrap();
                let common = format!(
                    "base: {}\nquestion: {} {ROW_SHAPE}\nplant: {}\n",
                    get("base"),
                    get("question"),
                    get("plant").trim_end_matches(".md")
                );
                let truth = format!("settled_by: {}\nmatch: {}\n", get("settled_by"), get("match"));
                let plant = format!(
                    "kind: plant\nseed: {}\n{common}genre: {}\nclass: design\n{truth}unless: {}\nnouns: {nouns}\n---\n{seed}",
                    get("seed"),
                    get("genre"),
                    get("unless")
                );
                let control = format!("kind: control\n{common}{truth}nouns: {nouns}\n---\n{control}");
                let id = get("id");
                (Fixture::parse(id, &plant).unwrap(), Fixture::parse(&format!("{id}-control"), &control).unwrap())
            })
            .collect()
    }

    /// A run of four words of the slug names the seed only with one word [`beyond`] the question: the question's topic
    /// words cannot name it on their own. `sets KEEP to 8` runs the slug of `plant-9` with `8`, which its question
    /// lacks; `the sandbox module sets` and the reviewer's `the check module has` on `control-1` hold only the
    /// question's words and short ones.
    #[test]
    fn a_slug_run_names_the_seed_only_with_a_word_outside_the_question() {
        let parse = |id: &str| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("evals/bp-research-plants/{id}.md"));
            Fixture::parse(id, &std::fs::read_to_string(path).unwrap()).unwrap().research.unwrap()
        };
        let keep = parse("plant-9");
        let seed = &keep.plants[0];
        assert!(
            names("The id's \"sets KEEP to 8\" is stale", seed, &asked(&keep.question), false),
            "8 is beyond the question"
        );
        assert!(
            !names("The sandbox module sets KEEP from the env", seed, &asked(&keep.question), false),
            "sets is set"
        );
        let check = parse("control-1");
        let row = "The check module has no unit tests of its own";
        assert!(
            !names(row, &check.plants[0], &asked(&check.question), false),
            "the check module has: the question's words"
        );
        assert!(
            names("The module has 108 lines", &check.plants[0], &asked(&check.question), false),
            "108 is beyond it"
        );
    }

    /// The pilot as the regression set (`m8-pilot/result.md`, the hand grade): kills on plants 2 to 5, with the match
    /// word in another row on 2, 4 and 5 (the pairing); a miss on plant 1, which read its settle path and wrote no
    /// `falsified` row; no `no_report`; false contradictions on controls 2, 4 and 5 and none on 1 (`15000` is not
    /// `1500`) and 3; nothing relayed. Each lead row sits in the trial's `report.md`. Needs the designs repository at
    /// `$BP_REDESIGN`, and says so when it is unset.
    #[test]
    fn the_pilot_trials_grade_as_the_hand_grade() {
        let Some(redesign) = std::env::var_os("BP_REDESIGN") else {
            eprintln!("BP_REDESIGN is unset: the pilot regression test reads its trials there, and is skipped");
            return;
        };
        let pilot = Path::new(&redesign).join("analysis/m8-pilot");
        let fixtures = pilot_fixtures(&std::fs::read_to_string(pilot.join("plants.md")).unwrap());
        assert_eq!(fixtures.len(), 5);
        for (n, (plant, control)) in fixtures.iter().enumerate() {
            for (fx, arm) in [(plant, "plant"), (control, "control")] {
                let dir = pilot.join(format!("trials/plant-{}-{arm}", n + 1));
                let trace = Trace::parse(&std::fs::read_to_string(dir.join("transcript.jsonl")).unwrap());
                let mut g = Graded::default();
                score(&mut g, fx, &trace, &dir);
                let s = &g.research;
                let report = std::fs::read_to_string(dir.join("report.md")).unwrap();
                for line in trace.final_text().lines().filter(|l| l.trim().starts_with('|')) {
                    assert!(report.contains(line.trim()), "{}: the lead's row is in report.md: {line}", dir.display());
                }
                let at = |c: fn(&Call) -> bool| {
                    s.rows.iter().position(|r| c(&r.call)).map_or("none".into(), |i| (i + 1).to_string())
                };
                println!(
                    "plant-{}-{arm}: rows {}; kill row {}; false row {}; relayed {}; unread {} of {} cited; documented unread {}; settle path read {}; children {}; pass {}",
                    n + 1,
                    s.rows.len(),
                    at(|c| matches!(c, Call::Kill(_))),
                    at(|c| *c == Call::False),
                    s.rows.iter().filter(|r| matches!(r.call, Call::Relayed(_))).count(),
                    s.rows.iter().filter(|r| r.unread).count(),
                    s.rows.iter().filter(|r| r.cited).count(),
                    s.rows.iter().filter(|r| r.unread && r.documented()).count(),
                    yes(s.settle_read.contains(&0)),
                    s.children,
                    g.pass
                );
                assert!(!s.no_report && s.children == 0, "plant-{}-{arm}", n + 1);
                assert!(!s.rows.iter().any(|r| matches!(r.call, Call::Relayed(_))), "plant-{}-{arm}", n + 1);
                if arm == "plant" {
                    assert_eq!(g.killed.contains(&0), n > 0, "plant-{}: the hand grade", n + 1);
                    assert!(s.settle_read.contains(&0), "plant-{}: the lead read the settle path", n + 1);
                } else {
                    let falsely = s.rows.iter().any(|r| r.call == Call::False);
                    assert_eq!(falsely, [1, 3, 4].contains(&n), "control {}: the hand grade", n + 1);
                }
            }
        }
    }
}
