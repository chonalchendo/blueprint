//! The candidates of `cargo xtask plants` (`plants.rs`): what the clone at `base:` holds that a plant can state
//! falsely, per class, with the flip and the settle command. Nothing here writes; the tests in `plants.rs` drive the
//! flips and the sentence scan on text.
//!
//! A sentence is a module doc's (`//!`, before the test module), a bundle line's (`.md`), or a named constant's (an
//! integer literal, with its `///` doc). Its token is one of three. A `Number` is an integer that a word or a unit
//! follows, outside backticks and not a reference (`design change 2`, `issue 9`). A `Negation` is `always`, `never`
//! or `is` before a word other than `not`. A `Name` is a backticked identifier with a `_`, `-`, `::`, a capital or a
//! digit in it, or a file name, of a kind the clone declares (`plants_flip::Kinds`), and another of that kind in the
//! same file that the sentence does not hold. A measurement states a count the clone gives only by a command: a
//! crate's Rust files or `#[test]` functions, a module's lines, or a bundle directory's markdown files. A constant
//! states its value in template words. A two-hop's file A names file B in a doc comment (`//!` or `///`); B's code
//! holds one integer constant, so the figure has one source. B is the name beside A, at the clone's root, or the one
//! file of the clone with that name.
//!
//! The settle command of a sentence greps a fragment of its token's line, the words before the token, that the file
//! holds on that line alone (`grep -F '<fragment>' <path>`; no line number, which could match a figure). Its output
//! then holds the true token and, by the planter's check, not the flip. A measurement's is the count itself.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::eval_sandbox::capture;
use crate::lines::non_test;
use crate::mutants_walk::splitmix;
use crate::plants_count::{measurements, module, place};
use crate::plants_flip::{Kinds, flip, flip_number, group, ident, spans};

/// One URL per source kind, on its topic, that the clone at the base does not hold (`--verify` checks it). A seed
/// names it, so a report that repeats the seed has an unread cite to count (31, design change 2).
pub const URLS: [(&str, &str); 4] = [
    ("count", "https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html"),
    ("const", "https://doc.rust-lang.org/reference/items/constant-items.html"),
    ("doc", "https://doc.rust-lang.org/rustdoc/how-to-write-documentation.html"),
    ("bundle", "https://code.claude.com/docs/en/skills#where-skills-live"),
];

/// Words before a number that make it a reference, not a measure.
const REFS: [&str; 22] = [
    "change",
    "changes",
    "issue",
    "issues",
    "milestone",
    "decision",
    "check",
    "rule",
    "row",
    "delta",
    "finding",
    "step",
    "part",
    "run",
    "round",
    "page",
    "section",
    "line",
    "lines",
    "draft",
    "item",
    "trial",
];
/// Words after a number that do not name what it counts.
const STOP: [&str; 14] = ["to", "and", "or", "of", "the", "in", "a", "an", "for", "is", "are", "at", "on", "by"];
const UNITS: [&str; 5] = ["s", "ms", "KB", "MB", "GB"];

#[derive(Clone, Debug)]
pub struct Candidate {
    /// One word: the class, the source and the place.
    pub id: String,
    pub class: &'static str,
    /// Empty when the candidate has no sentence (a two-hop name the clone does not hold).
    pub genre: &'static str,
    /// The file that holds the truth, or the directory a count runs over.
    pub source: String,
    /// The file the seed cites: none for a measurement.
    pub cited: Option<String>,
    /// The URL table's key.
    pub kind: &'static str,
    pub claim: String,
    pub flipped: String,
    /// The true token, its byte offset in `claim`, and the text the flip puts in its place.
    pub token: String,
    pub at: usize,
    pub swap: String,
    /// How the question asks: `count` (how many `unit` does `subject` have), `const` (the value of the constant
    /// `unit` in `subject`), `hop` (the figure of the file that `subject`'s doc names beside the `near` words), or
    /// `clause` (the sentence's own clause with a blank).
    pub form: &'static str,
    pub subject: String,
    pub unit: String,
    /// The title of a template form, `{}` in the figure's place; empty for a clause.
    pub label: String,
    /// The source line of the token; empty for a count.
    pub line: String,
    /// A two-hop's words of A's sentence around the name of B: the nouns come from them.
    pub near: Vec<String>,
    pub matches: Vec<String>,
    pub unless: Vec<String>,
    /// Empty when no command reproduces the figure.
    pub settle: String,
}

/// The clone's files at the base that one of `globs` matches, sorted.
pub fn sources(clone: &Path, globs: &[String]) -> Result<Vec<String>> {
    let all = capture(Command::new("git").args(["ls-files"]).current_dir(clone))?;
    Ok(all.lines().filter(|f| globs.iter().any(|g| glob(g, f))).map(str::to_string).collect())
}

/// Whether `path` matches `pattern`: `*` within one part, `**` across parts, `?` one character.
pub fn glob(pattern: &str, path: &str) -> bool {
    fn at(p: &[u8], s: &[u8]) -> bool {
        match p {
            [] => s.is_empty(),
            [b'*', b'*', b'/', rest @ ..] => at(rest, s) || (!s.is_empty() && at(p, &s[1..])),
            [b'*', b'*', rest @ ..] => (0..=s.len()).any(|i| at(rest, &s[i..])),
            [b'*', rest @ ..] => (0..=s.len()).take_while(|&i| i == 0 || s[i - 1] != b'/').any(|i| at(rest, &s[i..])),
            [b'?', rest @ ..] => !s.is_empty() && s[0] != b'/' && at(rest, &s[1..]),
            [c, rest @ ..] => s.first() == Some(c) && at(rest, &s[1..]),
        }
    }
    at(pattern.as_bytes(), path.as_bytes())
}

/// Every candidate of the three classes over `files`, each class in seed order.
pub fn candidates(clone: &Path, files: &[String], seed: u64) -> Result<[Vec<Candidate>; 3]> {
    let listed = capture(Command::new("git").args(["ls-files"]).current_dir(clone))?;
    let texts: Vec<(&str, String)> =
        listed.lines().map(|f| (f, std::fs::read_to_string(clone.join(f)).unwrap_or_default())).collect();
    let kinds = Kinds::of(texts.iter().map(|(f, t)| (*f, t.as_str())));
    let all: BTreeSet<String> = listed.lines().map(str::to_string).collect();
    let mut state = seed;
    let (mut measurement, mut design, mut two_hop) = (measurements(clone, files), Vec::new(), Vec::new());
    for a in files {
        let text = std::fs::read_to_string(clone.join(a))?;
        design.extend(sentences(a, &text, &kinds, &mut state).into_iter().map(|c| c.cite("design", a)));
        if rust(a) {
            two_hop.extend(crate::plants_hop::hops(clone, a, &text, &all)?);
        }
    }
    for list in [&mut measurement, &mut design, &mut two_hop] {
        list.sort_by(|x, y| x.id.cmp(&y.id));
        list.dedup_by(|x, y| x.id == y.id);
        shuffle(list, seed);
    }
    Ok([measurement, design, two_hop])
}

fn shuffle<T>(list: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..list.len()).rev() {
        list.swap(i, usize::try_from(splitmix(&mut state) % (i as u64 + 1)).expect("at most i"));
    }
}

impl Candidate {
    pub fn bare(a: &str, name: &str) -> Self {
        let empty = Vec::new;
        Self {
            id: format!("two-hop:{a}>{name}"),
            class: "two-hop",
            genre: "",
            source: name.into(),
            cited: Some(a.into()),
            kind: "doc",
            claim: String::new(),
            flipped: String::new(),
            token: String::new(),
            at: 0,
            swap: String::new(),
            form: "hop",
            subject: String::new(),
            unit: String::new(),
            label: String::new(),
            line: String::new(),
            near: empty(),
            matches: empty(),
            unless: empty(),
            settle: String::new(),
        }
    }

    fn cite(mut self, class: &'static str, cited: &str) -> Self {
        let via = if class == "two-hop" { format!("{cited}>") } else { String::new() };
        self.id = format!("{class}:{via}{}", self.id);
        (self.class, self.cited) = (class, Some(cited.to_string()));
        self
    }
}

pub fn rust(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs")
}

/// Whether `path` names a `.rs` or a `.md` file: a settle target, or a second file of a two-hop.
pub fn source_file(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs" || e == "md")
}

/// The file names that `text` names in backticks: `.rs` or `.md`, a trailing `:<line>` dropped.
pub fn siblings(text: &str) -> BTreeSet<String> {
    spans(text)
        .into_iter()
        .map(|(_, s)| s.split(':').next().unwrap_or(s).trim_start_matches("./"))
        .filter(|s| source_file(s) && !s.contains(' '))
        .map(str::to_string)
        .collect()
}

/// The paragraphs a file's sentences come from, as `(line number, text)` per line: a Rust file's module doc before
/// its test module, outside code fences, and its constants with their docs; a markdown file's lines outside fences
/// and frontmatter.
fn paragraphs(path: &str, text: &str) -> Vec<Vec<(usize, String)>> {
    let lines: Vec<&str> = text.lines().take(non_test(text)).collect();
    let (mut out, mut para, mut fenced) = (Vec::new(), Vec::new(), false);
    let rust = rust(path);
    let front = if !rust && lines.first() == Some(&"---") {
        lines.iter().skip(1).position(|l| *l == "---").map_or(0, |i| i + 2)
    } else {
        0
    };
    for (i, line) in lines.iter().enumerate().skip(front) {
        let body = if rust { line.strip_prefix("//!").map(str::trim) } else { Some(line.trim()) };
        let fence = body.is_some_and(|b| b.starts_with("```") || b.starts_with("~~~"));
        fenced ^= fence;
        match body.filter(|b| !b.is_empty() && !fenced && !fence && !b.starts_with('#') && !b.starts_with('|')) {
            Some(b) => para.push((i + 1, b.trim_start_matches("- ").to_string())),
            None => out.extend((!para.is_empty()).then(|| std::mem::take(&mut para))),
        }
    }
    out.extend((!para.is_empty()).then_some(para));
    out
}

/// The candidates of one file's sentences and constants: one per token, with its flip and settle command.
fn sentences(path: &str, text: &str, kinds: &Kinds, state: &mut u64) -> Vec<Candidate> {
    let raw: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let kind = if rust(path) { "doc" } else { "bundle" };
    for para in paragraphs(path, text) {
        let mut joined = String::new();
        let mut starts = Vec::new();
        for (no, t) in &para {
            starts.push((joined.len(), *no));
            joined.push_str(t);
            joined.push(' ');
        }
        for (from, sentence) in split_sentences(&joined) {
            for (at, token, genre) in tokens(sentence) {
                let abs = from + at;
                let Some(&(_, no)) = starts.iter().rev().find(|(s, _)| *s <= abs) else { continue };
                if abs + token.len() > starts.iter().find(|(s, _)| *s > abs).map_or(joined.len(), |s| s.0) {
                    continue;
                }
                let line = raw[no - 1];
                let r = splitmix(state);
                let Some((flipped, swap, matches, unless)) = flip(sentence, at, token, genre, text, kinds, r) else {
                    continue;
                };
                let lead = sentence.len() - sentence.trim_start().len();
                out.push(Candidate {
                    id: format!("{path}:{no}:{genre}:{token}"),
                    class: "design",
                    genre,
                    source: path.into(),
                    cited: None,
                    kind,
                    claim: sentence.trim().to_string(),
                    flipped: flipped.trim().to_string(),
                    token: token.to_string(),
                    at: at - lead,
                    swap,
                    form: "clause",
                    subject: String::new(),
                    unit: String::new(),
                    label: String::new(),
                    line: line.to_string(),
                    near: Vec::new(),
                    matches,
                    unless,
                    settle: settle(text, line, token, path),
                });
            }
        }
    }
    if rust(path) {
        out.extend(constants(path, text));
    }
    out
}

/// The sentences of a paragraph, with each one's byte offset: a sentence ends at `. ` before a capital or a backtick.
pub fn split_sentences(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    let b = text.as_bytes();
    for (i, _) in text.match_indices(". ") {
        let next = b.get(i + 2).copied().unwrap_or(b' ');
        if next.is_ascii_uppercase() || next == b'`' {
            out.push((from, &text[from..=i]));
            from = i + 2;
        }
    }
    if from < text.trim_end().len() {
        out.push((from, text[from..].trim_end()));
    }
    out
}

/// The tokens of a sentence a flip can change: `(offset, token, genre)`.
fn tokens(sentence: &str) -> Vec<(usize, &str, &'static str)> {
    let mut out = Vec::new();
    let ticked = |at: usize| sentence[..at].matches('`').count() % 2 == 1;
    let words: Vec<(usize, &str)> = sentence
        .split(' ')
        .scan(0, |at, w| {
            let here = *at;
            *at += w.len() + 1;
            Some((here, w))
        })
        .filter(|(_, w)| !w.is_empty())
        .collect();
    for (i, &(at, w)) in words.iter().enumerate() {
        let lead = w.len() - w.trim_start_matches('(').len();
        let core = w[lead..].trim_end_matches([',', ';', ':', '.', ')']);
        let at = at + lead;
        let next = words.get(i + 1).map_or("", |(_, n)| n.trim_end_matches([',', ';', ':', '.', ')']));
        let before = i.checked_sub(1).map(|j| words[j].1.to_lowercase()).unwrap_or_default();
        let groups =
            |(j, g): (usize, &str)| !g.is_empty() && g.bytes().all(|c| c.is_ascii_digit()) && (j == 0 || g.len() == 3);
        let number = core.len() + lead == w.len() && core.split([',', '_']).enumerate().all(groups);
        if number
            && !ticked(at)
            && (UNITS.contains(&next)
                || (next.len() >= 3 && next.chars().all(|c| c.is_ascii_lowercase()) && !STOP.contains(&next)))
            && !REFS.contains(&before.trim_matches(|c: char| !c.is_alphanumeric()))
            && !REFS.contains(&next)
            && next != "design"
            && !before.starts_with('#')
        {
            out.push((at, core, "Number"));
        }
        if matches!(core, "always" | "never") && !ticked(at) && core.len() == w.len() - lead {
            out.push((at, core, "Negation"));
        }
        let named = next.len() >= 3 && next.chars().all(char::is_alphabetic) && !STOP.contains(&next) && next != "not";
        if core == "is" && w == "is" && !ticked(at) && named {
            out.push((at, core, "Negation"));
        }
    }
    for (at, span) in spans(sentence) {
        if ident(span) {
            out.push((at, span, "Name"));
        }
    }
    out
}

/// The settle command of a token on `line`: `grep -F` on the words before it, or after it, that the file holds on
/// that line alone. Empty when no such fragment exists.
fn settle(file: &str, line: &str, token: &str, path: &str) -> String {
    let Some(at) = line.find(token) else { return String::new() };
    let (before, after) = (&line[..at], &line[at + token.len()..]);
    let fragments = [last_words(before, 5), first_words(after, 5)];
    for frag in fragments.into_iter().flatten() {
        let ok = frag.len() >= 12
            && frag.split_whitespace().count() >= 2
            && !frag.contains(['\'', '\\'])
            && !frag.contains(".rs")
            && !frag.contains(".md")
            && file.lines().filter(|l| l.contains(frag)).count() == 1;
        if ok {
            return format!("grep -F -e '{frag}' {path}");
        }
    }
    String::new()
}

fn last_words(s: &str, n: usize) -> Option<&str> {
    let s = s.trim_end();
    let starts: Vec<usize> = s.match_indices(' ').map(|m| m.0 + 1).collect();
    let from = if starts.len() >= n { starts[starts.len() - n] } else { s.len() - s.trim_start().len() };
    Some(&s[from..]).filter(|f| !f.is_empty())
}

fn first_words(s: &str, n: usize) -> Option<&str> {
    let s = s.trim_start();
    let end = s.match_indices(' ').nth(n - 1).map_or(s.len(), |m| m.0);
    Some(&s[..end]).filter(|f| !f.is_empty())
}

/// The named constants of a Rust file with an integer literal, in template words: `` `NAME` in <subject> is <value>
/// ``, the subject `the <stem> module of the <crate> crate` for a file under `src/` itself, or where the file is. The
/// title puts the facts in another order (`In <crate>'s src, the <stem> module sets NAME to <value>`), so the
/// question shares no run of four words with it.
pub fn constants(path: &str, text: &str) -> Vec<Candidate> {
    let lines: Vec<&str> = text.lines().take(non_test(text)).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        let Some(rest) = t.strip_prefix("const ") else { continue };
        let Some((name, rest)) = rest.split_once(':') else { continue };
        let Some(value) = rest.split_once('=').map(|(_, v)| v.trim().trim_end_matches(';')) else { continue };
        let Ok(n) = value.replace('_', "").parse::<u64>() else { continue };
        let sep = value.chars().find(|c| *c == '_');
        let f = group(flip_number(n, n ^ (i as u64)), sep);
        let (at, stem, kind) = place(path);
        // A file under `src/` itself takes 31's template; any other says where it is, so no two files share one.
        let subject = if at.ends_with("'s src") { module(path) } else { format!("the {stem} {kind} in {at}") };
        let lead = format!("`{name}` in {subject} is ");
        out.push(Candidate {
            id: format!("{path}:{}:Number:{name}", i + 1),
            class: "design",
            genre: "Number",
            source: path.into(),
            cited: None,
            kind: "const",
            claim: format!("{lead}{value}."),
            flipped: format!("{lead}{f}."),
            token: value.to_string(),
            at: lead.len(),
            swap: f.clone(),
            form: "const",
            subject,
            unit: name.to_string(),
            label: format!("In {at}, the {stem} {kind} sets {name} to {{}}"),
            line: (*line).to_string(),
            near: Vec::new(),
            matches: vec![value.to_string()],
            unless: vec![f],
            settle: format!("grep -F 'const {name}:' {path}"),
        });
    }
    out
}
