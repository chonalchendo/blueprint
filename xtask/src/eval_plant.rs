//! The fixture of the research eval (31, design changes 2 and 5): a `plant` seeds one false node, a `control` seeds
//! the true one. `cargo xtask plants` writes them (`plants.rs`); `Fixture::parse` reads them here when the header holds
//! a `question:` line, so the review fixtures' `kind: control` keeps its own rules.
//!
//! ```text
//! kind: plant | control
//! seed: <s>                                (plant: the planter's seed)
//! base: <full sha>                         (the clone every settle command and every cited path refers to)
//! parts: 1 | 2                             (plant, optional: absent means 1)
//! question: <one line, ending with the row shape>
//! plant: <node id>                         (one group per part; a control has one)
//! genre: Number | Negation | Name          (plant only)
//! class: measurement | design | two-hop    (plant only)
//! settled_by: <one shell command, run at the clone's root at base>
//! target: <path>:<line> | <path>           (plant only: the settle line at base; a count names its file or pathspec)
//! match: <word> | <word>                   (from the true sentence)
//! unless: <word> | <word>                  (plant only: from the flip)
//! nouns: <word> <word> [<word>]            (each verbatim in the question)
//! ---
//! <seed 1 frontmatter>
//! ---
//! <seed 1 body>
//! ---
//! <seed 2 frontmatter>                     (a two-part fixture only)
//! ---
//! <seed 2 body>
//! ```
//!
//! A group opens at its `plant:` line and holds the keys below it until the next `plant:`. The seeds follow the first
//! `---` line in group order: each is its frontmatter, a `---` line and a one-paragraph body, and a `---` line parts
//! two seeds. So a body holds no `---` line, and the seed file is `---`, the frontmatter, `---`, the body. The parser
//! refuses a file whose group count or seed count differs from `parts:`. A question names no seed's node id, and
//! shares no run of four words ([`words`]) with a seed's title, so the reader must search; `bp search` needs every
//! term and has no stemming, so each noun is the source's own word form. A control carries its plant's settle
//! command and the true `match:` word, so `--verify` settles it as it settles the plant. The planter computes `target:`
//! with `plants_verify::key`, and the holdout seal compares those lines across sets, so `--verify` refuses a plant
//! whose `target:` differs from the line its `settled_by:` finds.

use anyhow::{Context, Result, bail};

/// The sentence every research question ends with, so every arm is asked for the same table (31, design change 2).
pub const ROW_SHAPE: &str = "Report under `## Findings` as a table with one row per claim: `| claim | verdict (holds, falsified, unchecked) | source | mark (measured, documented, inferred) |`.";

/// The length of a word run that names a text: a question shares none with a seed's title, nor with the clone.
pub const RUN: usize = 4;

/// The words of `text` as a run is read: lowercase, backticks dropped, punctuation trimmed from each word's ends,
/// so `` `bp-diagram`, `` reads `bp-diagram` and a comment marker (`//!`) reads nothing. A blank (`___`) stays a word.
pub fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            if w.contains("___") {
                return "___".to_string();
            }
            w.replace('`', "").trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

pub const GENRES: [&str; 3] = ["Number", "Negation", "Name"];
pub const CLASSES: [&str; 3] = ["measurement", "design", "two-hop"];

/// A research fixture: one question over one or two seeded nodes.
#[derive(Debug, PartialEq)]
pub struct Research {
    pub control: bool,
    pub question: String,
    pub base: String,
    /// The planter's seed; none on a control.
    pub seed: Option<u64>,
    pub plants: Vec<Seed>,
}

/// One seeded node and how its truth is settled. A control's carries no unless, genre or class.
#[derive(Debug, Default, PartialEq)]
pub struct Seed {
    pub node: String,
    pub genre: String,
    pub class: String,
    pub settled_by: String,
    /// The `target:` line: empty on a control and on a fixture written before the planter wrote it.
    pub settle_line: String,
    pub matches: Vec<String>,
    pub unless: Vec<String>,
    pub nouns: Vec<String>,
    /// The node file: `---`, frontmatter, `---`, body.
    pub text: String,
}

impl Seed {
    /// The value of the seed's frontmatter `key`, unquoted.
    pub fn field(&self, key: &str) -> Option<String> {
        let front = self.text.strip_prefix("---\n")?.split("\n---\n").next()?;
        let value = front.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')).map(str::trim)?;
        Some(serde_json::from_str(value).unwrap_or_else(|_| value.to_string()))
    }

    /// The path of the seed's node file under a vault root.
    pub fn file(&self) -> String {
        format!("{}.md", self.node)
    }

    /// The settle target: the one `.rs` or `.md` path that `settled_by:` names, as 31's holdout block reads it.
    pub fn target(&self) -> Option<String> {
        self.settled_by.split_whitespace().find_map(|w| {
            let w = w.trim_matches(|c| c == '\'' || c == '"');
            crate::plants_pool::source_file(w).then(|| w.to_string())
        })
    }
}

impl Research {
    /// `Ok(None)` when `text` is no research fixture: its header holds no `kind: plant`, `plant:` or `question:` line.
    pub fn parse(id: &str, text: &str) -> Result<Option<Self>> {
        let (head, seeds) = text.split_once("\n---\n").with_context(|| format!("{id}: no `---` line"))?;
        if !head.lines().any(|l| l == "kind: plant" || l.starts_with("plant:") || l.starts_with("question:")) {
            return Ok(None);
        }
        let (mut kind, mut question, mut base, mut seed, mut parts) = (None, String::new(), String::new(), None, 1);
        let mut plants: Vec<Seed> = Vec::new();
        for line in head.lines().filter(|l| !l.trim().is_empty()) {
            let (key, value) = line.split_once(':').with_context(|| format!("{id}: `{line}` is no `key: value`"))?;
            let value = value.trim().to_string();
            let words = |sep: char| value.split(sep).map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect();
            let group = plants.last_mut();
            match (key, group) {
                ("kind", _) => kind = Some(value),
                ("question", _) => question = value,
                ("base", _) => base = value,
                ("seed", _) => seed = Some(value.parse().with_context(|| format!("{id}: seed `{value}`"))?),
                ("parts", _) => parts = value.parse().with_context(|| format!("{id}: parts `{value}`"))?,
                ("plant", _) => plants.push(Seed { node: value, ..Seed::default() }),
                ("genre", Some(g)) => g.genre = value,
                ("class", Some(g)) => g.class = value,
                ("settled_by", Some(g)) => g.settled_by = value,
                ("target", Some(g)) => g.settle_line = value,
                ("match", Some(g)) => g.matches = words('|'),
                ("unless", Some(g)) => g.unless = words('|'),
                ("nouns", Some(g)) => g.nouns = words(' '),
                ("genre" | "class" | "settled_by" | "target" | "match" | "unless" | "nouns", None) => {
                    bail!("{id}: `{key}:` before the first `plant:`")
                }
                _ => bail!("{id}: unknown key `{key}`"),
            }
        }
        let control = match kind.as_deref() {
            Some("plant") => false,
            Some("control") => true,
            _ => bail!("{id}: a research fixture is kind: plant or kind: control"),
        };
        let mut texts = seeds.trim_end().split("\n---\n");
        for p in &mut plants {
            let (Some(front), Some(body)) = (texts.next(), texts.next()) else { break };
            p.text = format!("---\n{}\n---\n{}\n", front.trim(), body.trim());
        }
        if plants.len() != parts || texts.next().is_some() || plants.iter().any(|p| p.text.is_empty()) {
            bail!("{id}: parts: {parts} needs {parts} plant: groups and {parts} seeds");
        }
        let r = Self { control, question, base, seed, plants };
        r.check(id, parts)?;
        Ok(Some(r))
    }

    fn check(&self, id: &str, parts: usize) -> Result<()> {
        if self.question.is_empty() || self.base.is_empty() {
            bail!("{id}: question and base are required");
        }
        if !self.question.ends_with(ROW_SHAPE) {
            bail!("{id}: the question does not end with the row shape");
        }
        if self.control {
            let p = &self.plants[0];
            if parts != 1 || self.seed.is_some() || !p.genre.is_empty() || !p.class.is_empty() {
                bail!("{id}: a control is one seed, with no seed:, genre: or class:");
            }
            if p.settled_by.is_empty() || p.matches.is_empty() || !p.unless.is_empty() {
                bail!("{id}: a control carries settled_by: and match:, and no unless:");
            }
        } else if self.seed.is_none() {
            bail!("{id}: a plant needs seed:");
        }
        for p in &self.plants {
            let node = &p.node;
            if !self.control && (!GENRES.contains(&p.genre.as_str()) || !CLASSES.contains(&p.class.as_str())) {
                bail!("{id}: {node}: genre is one of {GENRES:?} and class one of {CLASSES:?}");
            }
            if !self.control && p.settled_by.is_empty() {
                bail!("{id}: {node}: a plant needs settled_by:");
            }
            if !(2..=3).contains(&p.nouns.len()) || p.nouns.iter().any(|n| !self.question.contains(n.as_str())) {
                bail!("{id}: {node}: nouns are two or three words, each in the question");
            }
            let q = self.question.to_lowercase();
            let slug = node.rsplit('/').next().unwrap_or(node);
            let asked: Vec<String> = words(&self.question).windows(RUN).map(|w| w.join(" ")).collect();
            let shared =
                words(&p.field("title").unwrap_or_default()).windows(RUN).any(|w| asked.contains(&w.join(" ")));
            if q.contains(&node.to_lowercase()) || q.contains(slug) || shared {
                bail!("{id}: the question names the seed {node}");
            }
        }
        Ok(())
    }
}
