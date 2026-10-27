//! The three flips of `cargo xtask plants` (31, design change 2), applied to a true sentence of the clone. `Number`
//! doubles or halves, or adds or takes one under ten, never to zero. `Negation` swaps `always` and `never`, or puts
//! `not` after `is`: a word changes or one comes, none goes. `Name` swaps a backticked identifier for another of the
//! same file that the sentence does not hold. `match:` words come from the true sentence, `unless:` words from the
//! flip.

use std::collections::BTreeSet;

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

/// The `Name` flip: another backticked identifier of `file` that `sentence` does not hold, picked by `r`.
pub fn flip_name(file: &str, sentence: &str, r: u64) -> Option<String> {
    let pool: Vec<&str> =
        backticked(file).into_iter().map(|(_, n)| n).filter(|n| ident(n) && !sentence.contains(n)).collect();
    let pool: Vec<&str> = pool.into_iter().collect::<BTreeSet<_>>().into_iter().collect();
    (!pool.is_empty()).then(|| pool[usize::try_from(r % pool.len() as u64).expect("an index")].to_string())
}

/// The spans between backticks, with the byte offset of each span's text.
pub fn backticked(text: &str) -> Vec<(usize, &str)> {
    let ticks: Vec<usize> = text.match_indices('`').map(|m| m.0).collect();
    ticks.chunks_exact(2).map(|p| (p[0] + 1, &text[p[0] + 1..p[1]])).collect()
}

/// An identifier a `Name` plant may swap: one word, a letter first, and a mark that makes it a name in the code.
pub fn ident(s: &str) -> bool {
    s.len() >= 4
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':'))
        && (s.contains(['_', '-'])
            || s.contains("::")
            || s.chars().any(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// The flipped sentence and the match and unless words of one token.
pub fn flip(
    sentence: &str,
    at: usize,
    token: &str,
    genre: &'static str,
    file: &str,
    r: u64,
) -> Option<(String, Vec<String>, Vec<String>)> {
    match genre {
        "Number" => {
            let n: u64 = token.replace([',', '_'], "").parse().ok()?;
            let sep = token.chars().find(|c| matches!(c, ',' | '_'));
            let f = group(flip_number(n, r), sep);
            let flipped = format!("{}{f}{}", &sentence[..at], &sentence[at + token.len()..]);
            Some((flipped, vec![token.to_string()], vec![f]))
        }
        "Negation" => {
            let flipped = flip_negation(sentence, at, token);
            let next = sentence[at + token.len()..].split_whitespace().next()?;
            let next = next.trim_end_matches(|c: char| !c.is_alphanumeric());
            let swapped = &flipped[at..flipped.len() - (sentence.len() - at - token.len())];
            (!next.is_empty())
                .then(|| (flipped.clone(), vec![format!("{token} {next}")], vec![format!("{swapped} {next}")]))
        }
        _ => {
            let other = flip_name(file, sentence, r)?;
            let flipped = format!("{}{other}{}", &sentence[..at], &sentence[at + token.len()..]);
            Some((flipped, vec![token.to_string()], vec![other]))
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
