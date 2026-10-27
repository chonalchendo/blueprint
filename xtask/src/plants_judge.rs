//! Whether a candidate of `cargo xtask plants` makes a plant (`plants.rs`), and the plant it makes. The checks run
//! cheapest first: the settle at the base, the candidate's own words, the question's text and its leaks
//! (`plants_leak`), and last the search. A failed check is one line of `discarded.txt`.
//!
//! The search check seeds the plant, and its control when it gets one, into a scratch copy of `$BP_VAULT` that holds
//! every seed kept so far, and runs `bp search <noun>` for each noun alone, as `--verify` does. A noun is a word of
//! the question's subject, three letters or more, tried rarest first by the count of the vault's titles and ids and
//! the seeds kept so far that hold it; it is a noun only when its search returns the seed in its first rows. The
//! search is the test: no count bars a word before it. A seed that pushes a kept seed out of the first rows of one of
//! that seed's nouns is not kept.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::eval_plant::Seed;
use crate::plants_leak::{Corpus, hop_leaks, leaks};
use crate::plants_pool::{Candidate, URLS};
use crate::plants_verify::{key, output, ranked, settle, settles};

/// Words too common to find a seed, whatever the vault holds, and the words every template holds: never a noun.
const PLAIN: [&str; 59] = [
    "the", "and", "for", "has", "how", "its", "not", "are", "all", "any", "one", "two", "can", "may", "per", "out",
    "use", "set", "get", "new", "old", "but", "nor", "yet", "via", "src", "doc", "with", "that", "this", "from",
    "into", "when", "then", "than", "each", "every", "only", "same", "have", "does", "what", "which", "where", "there",
    "their", "them", "they", "after", "before", "first", "last", "once", "also", "other", "holds", "under", "module",
    "crate",
];
/// Negation words a `Negation` flip must not stand beside, so a flip never makes a double negative.
const NEGATIONS: [&str; 7] = ["not", "never", "no", "none", "nothing", "cannot", "without"];

/// A kept plant, with its seed and its control's.
pub struct Kept {
    pub c: Candidate,
    pub nouns: Vec<String>,
    pub node: String,
    /// Empty when the plant gets no control.
    pub control: String,
    pub title: String,
    pub true_title: String,
    /// The question of this plant alone, without the row shape.
    pub question: String,
}

/// What a plant is judged against: the clone, the planted settle lines, the vault's word counts and ids, the clone's
/// runs, and the scratch vault the search runs in.
pub struct Bench<'a> {
    pub clone: &'a Path,
    pub held: &'a BTreeSet<String>,
    pub words: &'a BTreeMap<String, usize>,
    pub ids: &'a mut BTreeSet<String>,
    pub corpus: &'a Corpus,
    pub search: &'a mut Search,
}

/// The plant a candidate makes, or the reason the planter discards it: `(candidate id, reason, free text)`.
pub fn judge(c: Candidate, b: &mut Bench, control: bool) -> Result<Result<Kept, (String, &'static str, String)>> {
    let id = c.id.clone();
    let no = |reason: &'static str, why: String| Ok(Err((id.clone(), reason, why)));
    if let Err((reason, why)) = sound(&c, b.clone, b.held) {
        return no(reason, why);
    }
    if let Some(why) = worded(&c) {
        return no("indistinct", why.into());
    }
    let c = if c.form == "hop" {
        let near: Vec<String> = rare(c.near.iter().map(String::as_str), &c, b.words).into_iter().take(3).collect();
        if near.len() < 2 {
            return no("indistinct", "fewer than two nouns a search finds alone".into());
        }
        crate::plants_text::hop(c, &near)
    } else {
        c
    };
    let t = match crate::plants_text::text(&c) {
        Ok(t) => t,
        Err(why) => return no("indistinct", why),
    };
    let options = rare(t.clause.split(|ch: char| !ch.is_alphanumeric()), &c, b.words);
    if options.len() < 2 {
        return no("indistinct", format!("fewer than two nouns a search finds alone (tried {})", options.join(", ")));
    }
    let line = output(b.clone, &c.settle).unwrap_or_default();
    let leak = leaks(&t.question, &c.matches, b.corpus)
        .or_else(|| if c.class == "two-hop" { hop_leaks(&line, &options, &t.question) } else { None });
    if let Some(why) = leak {
        return no("indistinct", why);
    }
    let dir = if c.class == "design" { "projects/bp/designs/components" } else { "shared/learnings" };
    let node = unique(b.ids, dir, &t.title);
    let ctl = if control { unique(b.ids, dir, &t.true_title) } else { String::new() };
    let mut k =
        Kept { c, nouns: options, node, control: ctl, title: t.title, true_title: t.true_title, question: t.question };
    let seed = Seed {
        text: format!("---\n{}", seed_text(&k, false)),
        genre: k.c.genre.into(),
        class: k.c.class.into(),
        nouns: k.nouns.clone(),
        ..seed_of(&k.c)
    };
    let mut seeds = vec![(k.node.clone(), seed_text(&k, false))];
    if control {
        seeds.push((k.control.clone(), seed_text(&k, true)));
    }
    let found = match crate::plants_text::breaks(&k.question, &seed) {
        Some(why) => Err(why),
        None => b.search.try_keep(&seeds, &k.nouns)?,
    };
    match found {
        Ok(nouns) => {
            k.nouns = nouns;
            Ok(Ok(k))
        }
        Err(why) => {
            for (node, _) in &seeds {
                b.ids.remove(node);
            }
            Ok(Err((id, "indistinct", why)))
        }
    }
}

/// Why a candidate cannot be a plant before its words are read: no sentence, no command, false at the base, a
/// cited file that holds the truth, or a settle line already planted.
fn sound(c: &Candidate, clone: &Path, held: &BTreeSet<String>) -> Result<(), (&'static str, String)> {
    if c.genre.is_empty() {
        return Err((
            "not-in-clone",
            format!("{} names a file the clone does not hold", c.cited.as_deref().unwrap_or("")),
        ));
    }
    if c.settle.is_empty() {
        return Err(("no-command", "no fragment of the line is the file's alone".into()));
    }
    let seed = seed_of(c);
    if let Some(why) = settle(clone, &seed) {
        return Err(("false-at-base", why));
    }
    if c.class == "two-hop" {
        let cited = std::fs::read_to_string(clone.join(c.cited.as_deref().unwrap_or_default())).unwrap_or_default();
        if c.matches.iter().any(|m| settles(&cited, &Seed { matches: vec![m.clone()], ..Seed::default() }).is_none()) {
            return Err(("indistinct", "the cited file holds the true word".into()));
        }
    }
    match key(clone, &seed) {
        Some(t) if held.contains(&t) => Err(("indistinct", format!("the target {t} is planted already"))),
        None => Err(("no-command", "the command names no .rs or .md path".into())),
        Some(_) => Ok(()),
    }
}

/// Why a candidate's own words make a weak or a visibly false plant: a true word of one character, which matches
/// almost any table; a `TODO` line; a flip to `1` before a plural; or a negation flip beside another negation.
pub fn worded(c: &Candidate) -> Option<&'static str> {
    if c.token.chars().count() == 1 {
        return Some("match word too short");
    }
    if c.line.contains("TODO") {
        return Some("the line holds TODO");
    }
    let next = c.claim[c.at + c.token.len()..].split_whitespace().next().unwrap_or_default();
    let before = c.claim[..c.at].split_whitespace().next_back().unwrap_or_default();
    if c.genre == "Number" && c.swap == "1" && next.trim_end_matches(|ch: char| !ch.is_alphabetic()).ends_with('s') {
        return Some("the flip gives 1 before a plural");
    }
    let bare = |w: &str| w.trim_matches(|ch: char| !ch.is_alphabetic()).to_lowercase();
    let beside = [before, next].iter().any(|w| NEGATIONS.contains(&bare(w).as_str()) || bare(w).ends_with("n't"));
    (c.genre == "Negation" && beside).then_some("the flip sits beside a negation")
}

/// The settle part of a candidate, as a fixture group carries it.
pub fn seed_of(c: &Candidate) -> Seed {
    Seed { settled_by: c.settle.clone(), matches: c.matches.clone(), unless: c.unless.clone(), ..Seed::default() }
}

/// The words of `from`, three letters or more, rarest first by the count of the vault's titles and ids and the seeds
/// kept so far that hold them; none is plain, nor the true or the flipped word.
fn rare<'a>(from: impl Iterator<Item = &'a str>, c: &Candidate, words: &BTreeMap<String, usize>) -> Vec<String> {
    let tokens: BTreeSet<String> = c.matches.iter().chain(&c.unless).flat_map(|w| title_words(w)).collect();
    let mut seen = BTreeSet::new();
    let mut found: Vec<(usize, usize, String)> = from
        .enumerate()
        .filter(|(_, w)| w.len() >= 3 && w.chars().all(char::is_alphabetic))
        .filter(|(_, w)| !PLAIN.contains(&w.to_lowercase().as_str()) && !tokens.contains(&w.to_lowercase()))
        .filter(|(_, w)| seen.insert(w.to_lowercase()))
        .map(|(i, w)| (words.get(&w.to_lowercase()).copied().unwrap_or(0), i, w.to_string()))
        .collect();
    found.sort();
    found.into_iter().map(|f| f.2).collect()
}

/// The lowercase words of a title or an id, as the index's `unicode61` tokenizer splits them.
pub fn title_words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// `<dir>/<slug of title>`, made unique against `ids`, which then holds it.
pub fn unique(ids: &mut BTreeSet<String>, dir: &str, title: &str) -> String {
    let mut slug = String::new();
    for w in title.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        if slug.len() + w.len() > 60 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&w.to_lowercase());
    }
    let mut id = format!("{dir}/{slug}");
    for n in 2.. {
        if !ids.contains(&id) {
            break;
        }
        id = format!("{dir}/{slug}-{n}");
    }
    ids.insert(id.clone());
    id
}

/// The seed node of a kept plant: the flipped claim, or with `truth` the true one, and the URL of its source kind.
pub fn seed_text(k: &Kept, truth: bool) -> String {
    let c = &k.c;
    let (claim, title) = if truth { (&c.claim, &k.true_title) } else { (&c.flipped, &k.title) };
    let url = URLS.iter().find(|u| u.0 == c.kind).map_or("", |u| u.1);
    // A title may hold `: `, which YAML reads as a mapping: it is written as a JSON string, which YAML reads too.
    let title = serde_json::to_string(title).expect("a string serialises");
    let front = if c.class == "design" {
        format!("kind: design\ntitle: {title}\nstatus: active\nunder: projects/bp/designs/system")
    } else {
        format!("kind: learning\ntitle: {title}\nstatus: verified\ncites: [projects/bp/designs/system]")
    };
    let body = match &c.cited {
        Some(path) => format!("{} (`{path}`). Further reading: {url}", claim.trim_end_matches('.')),
        None => format!("{claim} Measured on the repository's master branch. Further reading: {url}"),
    };
    format!("{front}\n---\n{body}\n")
}

/// The scratch vault the search check runs in, the seeds kept in it, and the `bp` that searches.
pub struct Search {
    bp: PathBuf,
    vault: PathBuf,
    /// Each kept seed's node id and nouns.
    kept: Vec<(String, Vec<String>)>,
}

impl Search {
    pub fn new(root: &Path) -> Result<Self> {
        Ok(Self { bp: crate::plants_verify::build_bp(root)?, vault: crate::scratch::run()?, kept: Vec::new() })
    }

    /// Writes `seeds` (node id, file text) and returns the nouns of `options` whose search returns the first seed in
    /// its first rows; or removes them and says why: fewer than two such nouns, or a kept seed pushed out.
    pub fn try_keep(&mut self, seeds: &[(String, String)], options: &[String]) -> Result<Result<Vec<String>, String>> {
        for (node, text) in seeds {
            let file = self.vault.join(format!("{node}.md"));
            std::fs::create_dir_all(file.parent().expect("a node id has a parent"))?;
            std::fs::write(file, format!("---\n{text}"))?;
        }
        let mut found = Vec::new();
        for noun in options {
            if seeds.iter().all(|(node, _)| self.finds(noun, node).unwrap_or(false)) {
                found.push(noun.clone());
            }
        }
        let text: String = seeds.iter().map(|s| s.1.to_lowercase()).collect();
        let mut crowded = None;
        for (node, nouns) in &self.kept {
            for noun in nouns.iter().filter(|n| title_words(&text).contains(&n.to_lowercase())) {
                if crowded.is_none() && !self.finds(noun, node)? {
                    crowded = Some(format!("it pushes {node} out of `bp search {noun}`"));
                }
            }
        }
        let why = match crowded {
            Some(why) => why,
            None if found.len() >= 2 => {
                found.truncate(2);
                self.kept.extend(seeds.iter().map(|(node, _)| (node.clone(), found.clone())));
                return Ok(Ok(found));
            }
            None => format!("fewer than two nouns a search finds alone (tried {})", options.join(", ")),
        };
        for (node, _) in seeds {
            std::fs::remove_file(self.vault.join(format!("{node}.md")))?;
        }
        Ok(Err(why))
    }

    /// Whether `node` sits in the first rows of `bp search <noun>` however its ties fall: every row ranked as high
    /// counts before it. The index orders a tie by insertion, which differs between this vault, filled seed by seed,
    /// and `--verify`'s, filled at once.
    fn finds(&self, noun: &str, node: &str) -> Result<bool> {
        let rows = ranked(&self.bp, &self.vault, noun)?;
        let Some(r) = rows.iter().find(|row| row.0 == node).map(|row| row.1) else { return Ok(false) };
        Ok(rows.iter().filter(|row| row.1 >= r - 1e-9 * r.abs()).count() <= crate::plants_verify::TOP)
    }
}
