//! The candidates of `cargo xtask plants` (`plants.rs`): what the clone at `base:` holds that a plant can state
//! falsely, per class, with the flip and the settle command. Nothing here writes; the tests in `plants.rs` drive the
//! flips and the sentence scan on text.
//!
//! A sentence is a module doc's (`//!`, before the test module), a bundle line's (`.md`), or a named constant's (an
//! integer literal, with its `///` doc). Its token is one of three. A `Number` is an integer that a word or a unit
//! follows, outside backticks and not a reference (`design change 2`, `issue 9`). A `Negation` is `always`, `never`
//! or `is` before a word other than `not`. A `Name` is a backticked identifier with a `_`, `-`, `::`, a capital or a
//! digit in it, and another such identifier of the same file that the sentence does not hold. A measurement states a
//! count the clone gives only by a command: a crate's Rust files or `#[test]` functions, a module's lines, or a bundle
//! directory's markdown files. A two-hop's second file is the name beside the first file, at the clone's root, or
//! the one file of the clone with that name.
//!
//! The settle command of a sentence greps a fragment of its token's line, the words before the token, that the file
//! holds on that line alone (`grep -nF '<fragment>' <path>`). Its output then holds the true token and, by the
//! planter's check, not the flip. A measurement's is the count itself, run at `base:`.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::eval_sandbox::capture;
use crate::lines::non_test;
use crate::mutants_walk::splitmix;
use crate::plants_flip::{backticked, flip, flip_number, group, ident};

/// One URL per source kind, on its topic, that the clone at the base does not hold (`--verify` checks it). A seed
/// names it, so a report that repeats the seed has an unread cite to count (31, design change 2).
pub const URLS: [(&str, &str); 7] = [
    ("doc", "https://doc.rust-lang.org/reference/comments.html#doc-comments"),
    ("const", "https://doc.rust-lang.org/reference/items/constant-items.html"),
    ("bundle", "https://code.claude.com/docs/en/sub-agents#write-subagent-files"),
    ("files", "https://doc.rust-lang.org/cargo/reference/cargo-targets.html#target-auto-discovery"),
    ("tests", "https://doc.rust-lang.org/rustc/tests/index.html"),
    ("lines", "https://www.gnu.org/software/coreutils/manual/html_node/wc-invocation.html"),
    ("bundle-files", "https://code.claude.com/docs/en/skills#where-skills-live"),
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
    let all: BTreeSet<String> =
        capture(Command::new("git").args(["ls-files"]).current_dir(clone))?.lines().map(str::to_string).collect();
    let mut state = seed;
    let (mut measurement, mut design, mut two_hop) = (measurements(clone, files), Vec::new(), Vec::new());
    for a in files {
        let text = std::fs::read_to_string(clone.join(a))?;
        design.extend(sentences(a, &text, &mut state).into_iter().map(|c| c.cite("design", a)));
        let own: String = text.lines().take(non_test(&text)).collect::<Vec<_>>().join("\n");
        let me = Path::new(a).file_name().and_then(|n| n.to_str()).unwrap_or_default();
        for name in siblings(&own).into_iter().filter(|n| n != me) {
            let dir = Path::new(a).parent().map(|d| d.join(&name).display().to_string()).unwrap_or_default();
            let named: Vec<&String> = all.iter().filter(|f| f.ends_with(&format!("/{name}"))).collect();
            let alone = (named.len() == 1).then(|| named[0].clone());
            let b = [Some(dir), Some(name.clone()), alone].into_iter().flatten().find(|b| all.contains(b) && b != a);
            let Some(b) = b else {
                two_hop.push(Candidate::bare(a, &name));
                continue;
            };
            let inner = std::fs::read_to_string(clone.join(&b))?;
            two_hop.extend(sentences(&b, &inner, &mut state).into_iter().map(|c| c.cite("two-hop", a)));
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
    fn bare(a: &str, name: &str) -> Self {
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

/// The counts: each crate's Rust files and `#[test]` functions, each module's lines, each bundle directory's
/// markdown files. The settle command is the count; the planter runs it at the base for the figure.
fn measurements(clone: &Path, files: &[String]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut crates = BTreeSet::new();
    let mut bundles = BTreeSet::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        match parts.as_slice() {
            ["crates", c, ..] => {
                crates.insert(format!("crates/{c}"));
            }
            ["xtask", ..] => {
                crates.insert("xtask".to_string());
            }
            ["bundle", d, _, ..] => {
                bundles.insert(format!("bundle/{d}"));
            }
            _ => {}
        }
        let stem = Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let krate = if parts[0] == "crates" { parts[1] } else { parts[0] };
        if rust(f) && !["lib", "main", "mod"].contains(&stem) {
            let what = format!("The {} module of the {krate} crate has", stem.replace('_', " "));
            out.push(count(
                clone,
                format!("measurement:lines:{f}"),
                f,
                "lines",
                &format!("wc -l < {f}"),
                &what,
                "lines",
            ));
        }
    }
    for c in &crates {
        let name = c.rsplit('/').next().unwrap_or(c);
        let cmd = format!("git ls-files '{c}/src/*.rs' | wc -l");
        let what = format!("The {name} crate has");
        out.push(count(clone, format!("measurement:files:{c}"), c, "files", &cmd, &what, "Rust source files"));
        let cmd = format!("git grep -c '#\\[test\\]' -- '{c}/*.rs' | awk -F: '{{n += $2}} END {{print n}}'");
        out.push(count(clone, format!("measurement:tests:{c}"), c, "tests", &cmd, &what, "test functions"));
    }
    for d in &bundles {
        let cmd = format!("git ls-files '{d}/*.md' | wc -l");
        let what = format!("The bundle's {} directory holds", d.rsplit('/').next().unwrap_or(d));
        out.push(count(clone, format!("measurement:md:{d}"), d, "bundle-files", &cmd, &what, "markdown files"));
    }
    out
}

/// A count candidate: the command run at the base gives the figure, and the claim states it with no path.
fn count(clone: &Path, id: String, source: &str, kind: &'static str, cmd: &str, what: &str, things: &str) -> Candidate {
    let out = capture(Command::new("bash").args(["-c", cmd]).current_dir(clone)).unwrap_or_default();
    let n: Option<u64> = out.trim().parse().ok().filter(|n| *n > 0);
    let mut state = n.unwrap_or(0) ^ id.len() as u64;
    let (claim, flipped, token, flip) = match n {
        Some(n) => {
            let f = flip_number(n, splitmix(&mut state));
            (format!("{what} {n} {things}."), format!("{what} {f} {things}."), n.to_string(), f.to_string())
        }
        None => (String::new(), String::new(), String::new(), String::new()),
    };
    Candidate {
        id,
        class: "measurement",
        genre: "Number",
        source: source.into(),
        cited: None,
        kind,
        claim,
        flipped,
        matches: vec![token],
        unless: vec![flip],
        settle: if n.is_some() { cmd.to_string() } else { String::new() },
    }
}

fn rust(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs")
}

/// Whether `path` names a `.rs` or a `.md` file: a settle target, or a second file of a two-hop.
pub fn source_file(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e == "rs" || e == "md")
}

/// The file names that `text` names in backticks: `.rs` or `.md`, a trailing `:<line>` dropped.
fn siblings(text: &str) -> BTreeSet<String> {
    backticked(text)
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
fn sentences(path: &str, text: &str, state: &mut u64) -> Vec<Candidate> {
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
                let Some((flipped, matches, unless)) = flip(sentence, at, token, genre, text, r) else { continue };
                out.push(Candidate {
                    id: format!("{path}:{no}:{genre}:{token}"),
                    class: "design",
                    genre,
                    source: path.into(),
                    cited: None,
                    kind,
                    claim: sentence.trim().to_string(),
                    flipped: flipped.trim().to_string(),
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
fn split_sentences(text: &str) -> Vec<(usize, &str)> {
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
    for (at, span) in backticked(sentence) {
        if ident(span) {
            out.push((at, span, "Name"));
        }
    }
    out
}

/// The settle command of a token on `line`: `grep -nF` on the words before it, or after it, that the file holds on
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
            return format!("grep -nF '{frag}' {path}");
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

/// The named constants of a Rust file with an integer literal: `` `NAME` is <value> `` and the first sentence of its
/// doc.
fn constants(path: &str, text: &str) -> Vec<Candidate> {
    let lines: Vec<&str> = text.lines().take(non_test(text)).collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
        let Some(rest) = t.strip_prefix("const ") else { continue };
        let Some((name, rest)) = rest.split_once(':') else { continue };
        let Some(value) = rest.split_once('=').map(|(_, v)| v.trim().trim_end_matches(';')) else { continue };
        let Ok(n) = value.replace('_', "").parse::<u64>() else { continue };
        let doc: Vec<&str> =
            lines[..i].iter().rev().map_while(|l| l.trim_start().strip_prefix("///")).map(str::trim).collect();
        let doc = doc.into_iter().rev().collect::<Vec<_>>().join(" ");
        let first = doc.split(". ").next().unwrap_or("").trim_end_matches('.');
        let tail = if first.is_empty() { ".".to_string() } else { format!(" ({}).", lower_first(first)) };
        let sep = value.chars().find(|c| *c == '_');
        let f = group(flip_number(n, n ^ (i as u64)), sep);
        out.push(Candidate {
            id: format!("{path}:{}:Number:{name}", i + 1),
            class: "design",
            genre: "Number",
            source: path.into(),
            cited: None,
            kind: "const",
            claim: format!("`{name}` is {value}{tail}"),
            flipped: format!("`{name}` is {f}{tail}"),
            matches: vec![value.to_string()],
            unless: vec![f],
            settle: format!("grep -nF 'const {name}:' {path}"),
        });
    }
    out
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().chain(c).collect()).unwrap_or_default()
}
