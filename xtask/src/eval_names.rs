//! What a row of the research grader's `## Findings` table names (31, design change 4; review of PR 81): the row's
//! words without its cite lines ([`uncited`]), and whether they name a seed ([`names`]).

use std::collections::BTreeSet;
use std::path::Path;

use crate::eval_plant::{RUN, Seed, words};
use crate::eval_reads::EXTENSIONS;
use crate::plants_verify::holds;

/// Whether `w` is a file path of a kind a row cites ([`EXTENSIONS`]).
fn pathy(w: &str) -> bool {
    EXTENSIONS.contains(&Path::new(w).extension().and_then(|e| e.to_str()).unwrap_or_default())
}

/// Whether `w` is a line or a list of lines: `16`, `116-122`, `1-2,302`, `16-L20`.
fn line_list(w: &str) -> bool {
    w.starts_with(|c: char| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | ',' | 'L'))
}

/// `text` without the line numbers of its cites, so a cite's line is no `match:` or `unless:` word (review of PR 81,
/// finding 1 and fold finding N1). It drops `:<lines>` after a path (`sandbox.rs:16`, `check.rs:108-120,130`), a bare
/// `:<lines>` after a path cite earlier in the row (`` `:302` ``, `, :116-122`), and after such a cite the prose
/// forms `line 16`, `lines 12-20`, `line ~118` and `L16`.
pub(crate) fn uncited(text: &str) -> String {
    let sep = |c: char| c.is_whitespace() || "`()[]<>;\"'|,".contains(c);
    let (mut out, mut rest) = (String::with_capacity(text.len()), text);
    while let Some(at) = rest.find(':') {
        out += &rest[..at];
        let tail = &rest[at + 1..];
        let lines = tail.find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | ','))).unwrap_or(tail.len());
        let before = out.rsplit(sep).next().unwrap_or_default();
        let cite = pathy(before) || (before.is_empty() && out.split(sep).any(pathy));
        if tail.starts_with(|c: char| c.is_ascii_digit()) && cite {
            rest = &tail[lines..];
        } else {
            out.push(':');
            rest = tail;
        }
    }
    out += rest;
    let (mut kept, mut seen, mut after_line) = (Vec::new(), false, false);
    for w in out.split_whitespace() {
        let bare = w.trim_matches(|c: char| "`()[]<>;\"'|,.:~#".contains(c));
        let prose = (after_line && line_list(bare)) || (seen && bare.strip_prefix('L').is_some_and(line_list));
        after_line = seen && !w.ends_with(':') && matches!(bare.to_lowercase().as_str(), "line" | "lines");
        seen |= pathy(bare);
        if !prose {
            kept.push(w);
        }
    }
    kept.join(" ")
}

/// The words of `text`, split at every character that is not a letter or a digit: `bp-tool's` is `bp`, `tool`, `s`.
pub(crate) fn terms(text: &str) -> BTreeSet<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).map(str::to_string).collect()
}

/// Whether the slug word `w` is a subject word the question `asked` lacks: four letters or more, no digit, and the
/// question holds neither it nor it less a final `s`. So `most` is; `has`, `the`, `40` and `sets` (for a question
/// that holds `set`) are not.
fn unasked(w: &str, asked: &BTreeSet<String>) -> bool {
    let held = asked.contains(w) || w.strip_suffix('s').is_some_and(|w| asked.contains(w));
    w.len() >= 4 && w.chars().all(char::is_alphabetic) && !held
}

/// Whether the word `w` of a slug says more than the question `asked`: a word with a digit that the question lacks, or
/// an [`unasked`] word. So `40` and `most` do; `has`, `the` and `sets` do not.
fn beyond(w: &str, asked: &BTreeSet<String>) -> bool {
    (w.contains(|c: char| c.is_ascii_digit()) && !asked.contains(w)) || unasked(w, asked)
}

/// Whether `text` names the seed: its node id or slug, its whole title, or a run of [`RUN`] words of its slug read as
/// words with one word [`beyond`] the question `asked`, so "the learning's id says 'at most 40 candidates'" names
/// `…-tests-at-most-40-candidates` (the pilot's control 5), and "the check module has" names nothing: the question's
/// topic words cannot name the seed on their own. With `flip`, an `unless:` word names it too, when the row also
/// holds an [`unasked`] word of the slug: a figure alone ("the forge keeps 8 runs") names no plant (fold finding N4).
/// The `nouns:` words and a part of the title are the question's words, so they name nothing (finding 2).
pub(crate) fn names(text: &str, seed: &Seed, asked: &BTreeSet<String>, flip: bool) -> bool {
    let slug = seed.node.rsplit('/').next().unwrap_or_default();
    let row = words(text);
    let title = words(&seed.field("title").unwrap_or_default());
    let slugged = words(&slug.replace('-', " "));
    let runs = slugged.windows(RUN).filter(|w| w.iter().any(|w| beyond(w, asked)));
    let held = terms(text);
    let subject = slugged.iter().any(|w| held.contains(w) && unasked(w, asked));
    text.to_lowercase().contains(slug)
        || (!title.is_empty() && row.windows(title.len()).any(|r| r == title))
        || runs.into_iter().any(|w| row.windows(RUN).any(|r| r == w))
        || (flip && subject && seed.unless.iter().any(|u| holds(text, u)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_fixture::Fixture;

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
            names("The id's \"sets KEEP to 8\" is stale", seed, &terms(&keep.question), false),
            "8 is beyond the question"
        );
        assert!(
            !names("The sandbox module sets KEEP from the env", seed, &terms(&keep.question), false),
            "sets is set"
        );
        let check = parse("control-1");
        let row = "The check module has no unit tests of its own";
        assert!(
            !names(row, &check.plants[0], &terms(&check.question), false),
            "the check module has: the question's words"
        );
        assert!(
            names("The module has 108 lines", &check.plants[0], &terms(&check.question), false),
            "108 is beyond it"
        );
    }
}
