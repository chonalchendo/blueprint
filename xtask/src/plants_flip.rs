//! The three flips of `cargo xtask plants` (31, design change 2), applied to a true sentence of the clone. `Number`
//! doubles or halves, or adds or takes one under ten, never to zero. `Negation` swaps `always` and `never`, or puts
//! `not` after `is`: a word changes or one comes, none goes. `Name` swaps a backticked identifier for another of the
//! same file and the same kind ([`Kinds`]) that the sentence does not hold. A backticked identifier is a whole token:
//! a backtick, no space or backtick, a backtick, at least four characters ([`spans`]); a prefix of a token is never
//! one. The pool of a `Name` flip is the identifiers of the file's prose (`//!` and `///` lines, or a markdown
//! file's lines), not its string literals. `match:` words come from the true sentence, `unless:` words from the flip.

use std::collections::{BTreeMap, BTreeSet};

/// The kind of each name the clone declares: `const <type>`, `fn`, `mod` (a `mod` line or a `.rs` file's stem),
/// `crate` (a directory under `crates/`) or `file` (a path or a file name the clone holds). A name of two kinds has
/// none, so a `Name` flip never swaps a constant for a function, and a name of no kind is no `Name` candidate.
#[derive(Default)]
pub struct Kinds(BTreeMap<String, String>);

impl Kinds {
    /// The kinds of the names in `files`, each a path and its text.
    pub fn of<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut add = |name: &str, kind: String| {
            seen.entry(name.to_string()).or_default().insert(kind);
        };
        for (path, text) in files {
            add(path, "file".into());
            add(path.rsplit('/').next().unwrap_or(path), "file".into());
            if let ["crates", c, ..] = path.split('/').collect::<Vec<_>>().as_slice() {
                add(c, "crate".into());
            }
            if !crate::plants_pool::rust(path) {
                continue;
            }
            add(path.rsplit('/').next().unwrap_or(path).trim_end_matches(".rs"), "mod".into());
            for line in text.lines().map(str::trim_start) {
                let line = line.strip_prefix("pub(crate) ").or_else(|| line.strip_prefix("pub ")).unwrap_or(line);
                let head = |p: &str| line.strip_prefix(p).map(|r| r.split(|c: char| !c.is_alphanumeric() && c != '_'));
                if let Some(rest) = line.strip_prefix("const ").and_then(|r| r.split_once(':')) {
                    let ty = rest.1.split('=').next().unwrap_or("").trim();
                    add(rest.0.trim(), format!("const {ty}"));
                } else if let Some(name) = head("fn ").and_then(|mut w| w.next()) {
                    add(name, "fn".into());
                } else if let Some(name) = head("mod ").and_then(|mut w| w.next()) {
                    add(name, "mod".into());
                }
            }
        }
        Self(
            seen.into_iter()
                .filter(|(_, k)| k.len() == 1)
                .filter_map(|(n, k)| Some((n, k.into_iter().next()?)))
                .collect(),
        )
    }

    /// The kind of a backticked name: of a path's last part, `a::b` read as `b`.
    pub fn kind(&self, name: &str) -> Option<&str> {
        self.0.get(name.rsplit("::").next().unwrap_or(name)).map(String::as_str)
    }
}

/// The `Number` flip: doubled or halved at ten and above (halved only when even), one added or taken under ten, and
/// never zero.
pub fn flip_number(n: u64, r: u64) -> u64 {
    match n {
        0 | 1 => n + 1,
        2..=9 if r.is_multiple_of(2) => n + 1,
        2..=9 => n - 1,
        _ if n.is_multiple_of(2) && r % 2 == 1 => n / 2,
        _ => n * 2,
    }
}

/// The `Negation` flip of the word at `at` in `sentence`: `always` and `never` swap, `is` gains `not`. No word goes.
pub fn flip_negation(sentence: &str, at: usize, word: &str) -> String {
    let swapped = match word {
        "always" => "never",
        "never" => "always",
        _ => "is not",
    };
    format!("{}{swapped}{}", &sentence[..at], &sentence[at + word.len()..])
}

/// The `Name` flip of `token`: another backticked identifier of the prose of `file`, of the token's kind, that
/// `sentence` does not hold, picked by `r`. None when the token's kind is unknown.
pub fn flip_name(file: &str, sentence: &str, token: &str, kinds: &Kinds, r: u64) -> Option<String> {
    let kind = kinds.kind(token)?;
    let markdown = !file.lines().any(|l| l.trim_start().starts_with("//"));
    let prose =
        file.lines().filter(|l| markdown || l.trim_start().starts_with("//!") || l.trim_start().starts_with("///"));
    let held: BTreeSet<&str> = spans(sentence).into_iter().map(|s| s.1).collect();
    let pool: BTreeSet<&str> = prose
        .flat_map(spans)
        .map(|(_, n)| n)
        .filter(|n| ident(n) && !held.contains(n) && !sentence.contains(n) && kinds.kind(n) == Some(kind))
        .collect();
    let pool: Vec<&str> = pool.into_iter().collect();
    (!pool.is_empty()).then(|| pool[usize::try_from(r % pool.len() as u64).expect("an index")].to_string())
}

/// The whole backticked tokens of `text`, with the byte offset of each token's text: a backtick, then no space and no
/// backtick up to the next backtick. A backtick that opens no token is passed over, so a stray one shifts nothing.
pub fn spans(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = text[at..].find('`').map(|i| at + i) {
        let rest = &text[open + 1..];
        let end = rest.find(|c: char| c == '`' || c.is_whitespace());
        match end {
            Some(e) if e > 0 && rest[e..].starts_with('`') => {
                out.push((open + 1, &rest[..e]));
                at = open + e + 2;
            }
            _ => at = open + 1,
        }
    }
    out
}

/// An identifier a `Name` plant may swap: one word, a letter first, and a mark that makes it a name in the code, or
/// a file name.
pub fn ident(s: &str) -> bool {
    let file = crate::plants_pool::source_file(s);
    s.len() >= 4
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '/' | '.'))
        && !s.replace("::", "").contains(':')
        && (file || !s.contains(['/', '.']))
        && (file
            || s.contains(['_', '-'])
            || s.contains("::")
            || s.chars().any(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// The flipped sentence, the word that replaced the token, and the match and unless words of one token.
pub fn flip(
    sentence: &str,
    at: usize,
    token: &str,
    genre: &'static str,
    file: &str,
    kinds: &Kinds,
    r: u64,
) -> Option<(String, String, Vec<String>, Vec<String>)> {
    match genre {
        "Number" => {
            let n: u64 = token.replace([',', '_'], "").parse().ok()?;
            let sep = token.chars().find(|c| matches!(c, ',' | '_'));
            let f = group(flip_number(n, r), sep);
            let flipped = format!("{}{f}{}", &sentence[..at], &sentence[at + token.len()..]);
            Some((flipped, f.clone(), vec![token.to_string()], vec![f]))
        }
        "Negation" => {
            let flipped = flip_negation(sentence, at, token);
            let next = sentence[at + token.len()..].split_whitespace().next()?;
            let next = next.trim_end_matches(|c: char| !c.is_alphanumeric());
            let swapped = &flipped[at..flipped.len() - (sentence.len() - at - token.len())];
            let words = (vec![format!("{token} {next}")], vec![format!("{swapped} {next}")]);
            (!next.is_empty()).then(|| (flipped.clone(), swapped.to_string(), words.0, words.1))
        }
        _ => {
            let other = flip_name(file, sentence, token, kinds, r)?;
            let flipped = format!("{}{other}{}", &sentence[..at], &sentence[at + token.len()..]);
            Some((flipped, other.clone(), vec![token.to_string()], vec![other]))
        }
    }
}

/// `n` with `sep` between groups of three digits, as the source wrote its own figure.
pub fn group(n: u64, sep: Option<char>) -> String {
    let digits = n.to_string();
    let Some(sep) = sep else { return digits };
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}
