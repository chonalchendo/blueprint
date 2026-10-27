//! The question and the title of a plant (31, design change 2). The question asks for the fact by its subject and
//! its nouns; `plants_leak` refuses one that holds a run of four words of the clone, so a question never quotes its
//! source.
//!
//! A count, a constant and a two-hop take template words. A count asks `How many <unit> does <subject> have?`, a
//! constant `What is the value of `<NAME>` in <subject>, and what does it set?`, a two-hop `The doc of <subject>
//! names a file on <near words>. What figure does that file's constant hold, and what does it set?`. Each title puts
//! the facts in another order, so the question shares no run of four words with it (`eval_plant::RUN`).
//!
//! A sentence asks with its subject clause, the token blanked: `"<clause>": what is the figure?`, `Which identifier
//! does "<clause>" name?`, or `Does "<clause>" hold with `always` or with `never` in the blank?`. The clause is the
//! shortest span that holds the token, cut only at a clause boundary: the sentence's ends, or `, `, `; `, `: `, `. `,
//! `) ` or ` (`. It has four words or more, balanced brackets and backticks, no opening joining word ([`JOINS`]), no
//! backticked label (`` `genre:` ``) that reads as a cut once the backticks drop, and a title of at most
//! [`TITLE_MAX`] characters; else the sentence gives no plant (`indistinct`, "no clause boundary"). The title is the
//! clause with the flip in place. [`breaks`] checks a written fixture by the same rules.

use crate::eval_plant::Seed;
use crate::plants_pool::Candidate;

pub const TITLE_MAX: usize = 80;
/// Words that open a dependent or a joined clause: a span that starts with one is a fragment, not a fact.
const JOINS: [&str; 14] =
    ["and", "or", "so", "but", "nor", "yet", "which", "that", "while", "because", "then", "if", "unless", "whereas"];

/// Whether `text` opens with a word of [`JOINS`].
fn joined(text: &str) -> bool {
    let first = text.split(|c: char| !c.is_alphanumeric()).find(|w| !w.is_empty()).unwrap_or_default();
    JOINS.contains(&first.to_lowercase().as_str())
}

/// The question (without the row shape), the plant's title, the control's title, and the true clause.
pub struct Text {
    pub question: String,
    pub title: String,
    pub true_title: String,
    pub clause: String,
}

/// The question and titles of a candidate, or why it has none.
pub fn text(c: &Candidate) -> Result<Text, String> {
    let ask = match c.form {
        "count" => format!("How many {} does {} have?", c.unit, c.subject),
        "const" => format!("What is the value of `{}` in {}, and what does it set?", c.unit, c.subject),
        "hop" => format!(
            "The doc of {} names a file on {}. What figure does that file's constant hold, and what does it set?",
            c.subject,
            list(&c.near)
        ),
        _ => return sentence(c),
    };
    let clause = match c.form {
        "const" => format!("{} {}", c.subject, c.unit),
        "hop" => c.near.join(" "),
        _ => c.subject.clone(),
    };
    Ok(Text {
        question: format!("{ask} Give the number and its source, at the repository's current commit."),
        title: c.label.replace("{}", &c.swap),
        true_title: c.label.replace("{}", &c.token),
        clause,
    })
}

/// `a`, `a and b`, or `a, b and c`.
fn list(words: &[String]) -> String {
    match words {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A two-hop candidate with its `near` words cut to `nouns`, and its title and seed sentences in template words.
pub fn hop(mut c: Candidate, nouns: &[String]) -> Candidate {
    let a = c.cited.clone().unwrap_or_default();
    let (at, stem, kind) = crate::plants_count::place(&a);
    c.near = nouns.to_vec();
    let lead =
        format!("In {at}, the {stem} {kind}'s doc names a file on {}; the constant of that file is", list(nouns));
    (c.claim, c.flipped) = (format!("{lead} {}.", c.token), format!("{lead} {}.", c.swap));
    c.label =
        format!("In {at}, the {stem} {kind} names a {} file set to {{}}", nouns.first().map_or("", String::as_str));
    c
}

/// The question and titles of a sentence candidate: its subject clause, the token blanked.
fn sentence(c: &Candidate) -> Result<Text, String> {
    let (s, e) = clause(c).ok_or("no clause boundary")?;
    let (head, tail) = (&c.claim[s..c.at], &c.claim[c.at + c.token.len()..e]);
    let title = plain(&format!("{head}{}{tail}", c.swap));
    let true_title = plain(&c.claim[s..e]);
    let blank = format!("{head}___{tail}");
    let ask = match c.genre {
        "Number" => format!("\"{blank}\": what is the figure?"),
        "Name" => format!("Which identifier does \"{blank}\" name?"),
        _ if c.token == "is" => format!("Does \"{blank}\" hold with `is` or with `is not` in the blank?"),
        _ => format!("Does \"{blank}\" hold with `always` or with `never` in the blank?"),
    };
    let what = match c.genre {
        "Number" => "number",
        "Name" => "name",
        _ => "rule",
    };
    let question = format!("{ask} Give the {what} and its source, at the repository's current commit.");
    Ok(Text { question, title, true_title, clause: c.claim[s..e].to_string() })
}

/// The subject clause of the candidate's token in its claim, as a byte range.
fn clause(c: &Candidate) -> Option<(usize, usize)> {
    let claim = c.claim.as_str();
    let (mut starts, mut ends) = (vec![0], vec![claim.trim_end_matches('.').len()]);
    for sep in [", ", "; ", ": ", ". ", ") ", " ("] {
        for (i, _) in claim.match_indices(sep) {
            let keep = usize::from(sep == ") ");
            ends.push(i + keep);
            starts.push(i + 2);
        }
    }
    let (lo, hi) = (c.at, c.at + c.token.len());
    let mut spans: Vec<(usize, usize)> =
        starts.iter().flat_map(|&s| ends.iter().map(move |&e| (s, e))).filter(|&(s, e)| s <= lo && hi <= e).collect();
    spans.sort_unstable();
    spans.into_iter().find(|&(s, e)| {
        let text = &claim[s..e];
        let title = plain(&format!("{}{}{}", &claim[s..lo], c.swap, &claim[hi..e]));
        let labels = |t: &str| t.matches(": ").count();
        balanced(text)
            && !joined(text)
            && labels(text) == labels(&text.replace('`', ""))
            && text.split_whitespace().count() >= 4
            && title.len() <= TITLE_MAX
    })
}

/// `text` without backticks, trimmed, its first letter a capital.
fn plain(text: &str) -> String {
    let t = text.trim().replace('`', "");
    if text.trim().starts_with('`') {
        return t;
    }
    let mut c = t.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Whether every bracket closes in order and the backticks pair.
pub fn balanced(text: &str) -> bool {
    let mut open = Vec::new();
    for ch in text.chars() {
        match ch {
            '(' | '[' | '{' => open.push(ch),
            ')' | ']' | '}' => {
                let want = match ch {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if open.pop() != Some(want) {
                    return false;
                }
            }
            _ => {}
        }
    }
    open.is_empty() && text.matches('`').count().is_multiple_of(2)
}

/// The byte offset of `word` in `text` as a whole word, in any case.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let (text, word) = (text.to_lowercase(), word.to_lowercase());
    let part = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    text.match_indices(word.as_str())
        .map(|(at, _)| at)
        .find(|&at| !part(text[..at].chars().next_back()) && !part(text[at + word.len()..].chars().next()))
}

/// Why a written plant's or control's question or title breaks the rules of [`text`], or `None`. The title fits
/// [`TITLE_MAX`] with balanced brackets, opens with no joining word, and holds the flip (a control's: the true word).
/// The question is balanced and holds each noun. A sentence's title also sits in the seed's body at clause
/// boundaries, and its question holds the title's clause with `___` in the slot.
pub fn breaks(question: &str, seed: &Seed) -> Option<String> {
    let title = seed.field("title").unwrap_or_default();
    let body = seed.text.splitn(3, "---\n").nth(2).unwrap_or_default().replace('`', "").to_lowercase();
    if title.len() > TITLE_MAX || !balanced(&title) || !balanced(question) || joined(&title) {
        return Some(format!("the title `{title}` or the question is too long, unbalanced or a joined fragment"));
    }
    let lower = title.to_lowercase();
    // A sentence's settle greps a fragment (`grep -F -e`); a count's or a constant's does not, on a control too.
    let template = !seed.settled_by.starts_with("grep -F -e ");
    let flip = seed.unless.first().or(seed.matches.first()).map(String::as_str).unwrap_or_default();
    let slot = if seed.genre == "Negation" { flip.rsplit_once(' ').map_or(flip, |f| f.0) } else { flip };
    let Some(at) = find_word(&title, slot) else { return Some(format!("the title does not hold the slot `{slot}`")) };
    if !crate::plants_verify::holds(&body, flip) {
        return Some(format!("the seed's body does not state `{flip}`"));
    }
    let missing = seed.nouns.iter().find(|n| !question.contains(n.as_str()));
    if template {
        return missing.map(|n| format!("the question does not hold `{n}`"));
    }
    let cut = |p: usize| {
        let before = body[..p].trim_end().chars().next_back();
        let after = &body[p + lower.len()..];
        let opens = before.is_none_or(|c| ".;:,()".contains(c));
        let closes = after.is_empty()
            || after.starts_with(['.', ';', ':', ',', ')'])
            || after.starts_with(" (")
            || (lower.ends_with(')') && after.starts_with(' '));
        opens && closes
    };
    if !body.match_indices(lower.as_str()).any(|(p, _)| cut(p)) {
        return Some(format!("the title `{title}` is no clause of the seed"));
    }
    let held = format!("{}___{}", &lower[..at], &lower[at + slot.len()..]);
    if !question.replace('`', "").to_lowercase().contains(&held) {
        return Some(format!("the question does not hold `{held}`"));
    }
    missing.map(|n| format!("the question does not hold `{n}`"))
}
