//! The leak rule of a research question (31, design change 2): the question must send the reader to the vault and
//! the hop, so no part of it may find the source by itself. A question leaks when it holds a run of four words
//! ([`RUN`]) that the clone at `base:` holds, a `match:` word, or, for a two-hop, a noun or a backticked word that sits
//! on the settle line of the second file. The planter discards such a candidate (`indistinct`, "question leaks") and
//! `--verify` refuses such a fixture.
//!
//! The clone's text is read as a run is ([`words`]): lowercase, backticks and comment markers dropped, words joined
//! across lines, so a run that a backtick or a line break splits in the source is still found. The research sets and
//! the planter's own source are left out: they hold the questions and the templates, not a source of a fact. The
//! fixed sentences of every question (the row shape, the two-part frame, the `Give the …` request) are no run.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::Command;

use anyhow::Result;

use crate::eval_plant::{ROW_SHAPE, RUN, words};
use crate::eval_sandbox::capture;
use crate::plants_verify::holds;

/// The clone's paths that hold questions and templates, not facts.
const OWN: [&str; 3] = ["xtask/evals/bp-research-", "xtask/src/plants", "xtask/src/eval_plant.rs"];

/// Every run of [`RUN`] words of the clone's text, hashed.
pub struct Corpus(HashSet<u64>);

impl Corpus {
    pub fn of(clone: &Path) -> Result<Self> {
        let listed = capture(Command::new("git").args(["ls-files"]).current_dir(clone))?;
        let texts = listed
            .lines()
            .filter(|f| !OWN.iter().any(|o| f.starts_with(o)))
            .filter_map(|f| std::fs::read_to_string(clone.join(f)).ok());
        Ok(Self::from_texts(texts.collect::<Vec<_>>().iter().map(String::as_str)))
    }

    pub fn from_texts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Self {
        Self(texts.into_iter().flat_map(|t| words(t).windows(RUN).map(hash).collect::<Vec<_>>()).collect())
    }
}

fn hash(run: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    run.hash(&mut h);
    h.finish()
}

/// Why `question` leaks: a run of the clone, or a word of `matches`. `None` when it does not.
pub fn leaks(question: &str, matches: &[String], corpus: &Corpus) -> Option<String> {
    let mut q = question.replace(ROW_SHAPE, "\n");
    for fixed in ["This question has two parts. First:", "Second:"] {
        q = q.replace(fixed, "\n");
    }
    for what in ["number", "name", "rule"] {
        q = q.replace(&format!("Give the {what} and its source, at the repository's current commit."), "\n");
    }
    for part in q.split('\n') {
        if let Some(run) =
            words(part).windows(RUN).find(|w| !w.contains(&"___".to_string()) && corpus.0.contains(&hash(w)))
        {
            return Some(format!("question leaks: the clone holds `{}`", run.join(" ")));
        }
    }
    matches.iter().find(|m| holds(question, m)).map(|m| format!("question leaks: it holds the match word `{m}`"))
}

/// Why a two-hop's question leaks through the settle line of its second file: a noun, or a word of a backticked
/// token of the question, sits on that line.
pub fn hop_leaks(line: &str, nouns: &[String], question: &str) -> Option<String> {
    let split = |t: &str| {
        t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect::<Vec<_>>()
    };
    let on: HashSet<String> = split(line).into_iter().collect();
    let asked = question.replace(ROW_SHAPE, "");
    let ticked = crate::plants_flip::spans(&asked).into_iter().flat_map(|(_, t)| split(t));
    nouns
        .iter()
        .map(|n| n.to_lowercase())
        .chain(ticked)
        .find(|w| on.contains(w))
        .map(|w| format!("question leaks: `{w}` sits on the settle line of the second file"))
}
