//! `cargo xtask plants`, the planted-fact fixtures of the research eval (31, design changes 2 and 5).
//!
//! ```text
//! cargo xtask plants --list --from <glob> [--from <glob>]... --seed <s> [--base <sha>]
//! cargo xtask plants --from <glob>... --count <k> --seed <s> --out <dir> [--controls <n>] [--parts <m>] [--base <sha>]
//! cargo xtask plants --verify --fixtures <dir>
//! ```
//!
//! It reads the sources from a throwaway clone at `--base` (default `HEAD`), never from the working tree, and lists
//! the candidates of three classes in seed order (`plants_pool.rs`). A `measurement` states a count with no path. A
//! `design` is a component design under `projects/bp/designs/components/` that cites the sentence's file: the layout
//! admits no other design slug. A `two-hop` is a learning that cites a file which names, in backticks, the file that
//! holds the truth.
//!
//! The planter walks the classes in turn, measurement, design, two-hop, and takes each class's next plant, so a
//! class that discards more candidates still gets its share. It settles each candidate at the base and keeps it, or writes one line to `discarded.txt`: `<candidate id> <reason>`
//! and free text. The reason is `no-command` (no command reproduces the figure), `false-at-base` (the command's output
//! lacks the true word or holds the flip), `not-in-clone` (a named file the clone does not hold) or `indistinct`. A
//! candidate is indistinct when the cited file holds the true word, when its settle target is already planted in any
//! `xtask/evals/bp-research-*` set, when its sentence has no clause boundary ([`crate::plants_text`]), or when its title holds fewer than two nouns that a search can find alone. A noun
//! is a word of the seed's title, of four letters or more, that the titles and ids of `$BP_VAULT` and of the seeds
//! kept so far hold at most three times; `bp search` ranks ids and titles first.
//!
//! It stops at `--count` plants. The first `--parts` files each pair two consecutive kept plants of different classes
//! in one question. The first `--controls` plants each get a control: the true seed, at the slug of its true title.
//! `generated.txt` records the command, the seed, the base and the vault's head, so the set can be made again.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::eval::usage;
use crate::eval_plant::{ROW_SHAPE, Seed};
use crate::eval_sandbox::capture;
use crate::plants_pool::{Candidate, URLS, candidates, sources};
use crate::plants_verify::{settles, targets};

const USAGE: &str = "usage: cargo xtask plants --list --from <glob>... --seed <s> [--base <sha>] | cargo xtask plants --from <glob>... --count <k> --seed <s> --out <dir> [--controls <n>] [--parts <m>] [--base <sha>] | cargo xtask plants --verify --fixtures <dir>";

/// Words too common to find a seed, whatever the vault holds: never a noun. A word that every seed of a class holds
/// (`crate`, `module`) is rationed by the count of seeds kept so far instead.
const PLAIN: [&str; 30] = [
    "with", "that", "this", "from", "into", "when", "then", "than", "each", "every", "only", "same", "have", "does",
    "what", "which", "where", "there", "their", "them", "they", "after", "before", "first", "last", "once", "also",
    "other", "holds", "under",
];
/// A noun's limit: titles and ids of the vault and of the seeds kept so far that hold it.
const RARE: usize = 3;

#[derive(Default)]
struct Args {
    from: Vec<String>,
    count: usize,
    seed: Option<u64>,
    out: Option<PathBuf>,
    controls: usize,
    parts: usize,
    base: Option<String>,
    list: bool,
    verify: Option<PathBuf>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self> {
        let mut a = Self::default();
        let mut verify = false;
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| usage(format!("{arg} takes a value")));
            let number = |v: &String| v.parse::<usize>().map_err(|_| usage(format!("{arg} is a count")));
            match arg.as_str() {
                "--list" => a.list = true,
                "--verify" => verify = true,
                "--from" => a.from.push(value()?.clone()),
                "--count" => a.count = number(value()?)?,
                "--seed" => a.seed = Some(value()?.parse().map_err(|_| usage("--seed is a number"))?),
                "--out" => a.out = Some(value()?.into()),
                "--controls" => a.controls = number(value()?)?,
                "--parts" => a.parts = number(value()?)?,
                "--base" => a.base = Some(value()?.clone()),
                "--fixtures" => a.verify = Some(value()?.into()),
                s => return Err(usage(format!("unknown argument {s}"))),
            }
        }
        let planting = !a.from.is_empty() && a.seed.is_some();
        let ok = if verify {
            a.verify.is_some() && a.from.is_empty() && a.seed.is_none() && !a.list && a.out.is_none()
        } else if a.list {
            planting && a.count == 0 && a.out.is_none() && a.controls == 0 && a.parts == 0 && a.verify.is_none()
        } else {
            planting
                && a.count > 0
                && a.out.is_some()
                && a.verify.is_none()
                && a.controls <= a.count
                && 2 * a.parts <= a.count
        };
        if !ok {
            return Err(usage(USAGE));
        }
        Ok(a)
    }
}

pub fn run(args: &[String]) -> Result<Vec<String>> {
    let a = Args::parse(args)?;
    if let Some(dir) = &a.verify {
        return crate::plants_verify::run(dir);
    }
    let root = Path::new(".").canonicalize()?;
    let base = capture(Command::new("git").args([
        "rev-parse",
        "--verify",
        &format!("{}^{{commit}}", a.base.as_deref().unwrap_or("HEAD")),
    ]))?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-plants"), 4)?;
    let clone = work.join("code");
    capture(Command::new("git").args(["clone", "-q", "--no-hardlinks"]).arg(&root).arg(&clone))?;
    capture(Command::new("git").args(["checkout", "-q", "--detach", &base]).current_dir(&clone))?;
    let seed = a.seed.expect("parse requires a seed");
    let files = sources(&clone, &a.from)?;
    if files.is_empty() {
        bail!("no file of the clone at {base} matches {:?}", a.from);
    }
    let pools = candidates(&clone, &files, seed)?;
    if a.list {
        for c in pools.iter().flatten() {
            let genre = if c.genre.is_empty() { "-" } else { c.genre };
            let path = match (c.class, &c.cited) {
                ("two-hop", Some(a)) => format!("{a}>{}", c.source),
                _ => c.source.clone(),
            };
            println!("{}\t{genre}\t{path}\t{}\t{}", c.class, c.claim, c.settle);
        }
        let counts: Vec<String> =
            pools.iter().map(|p| format!("{} {}", p.first().map_or("", |c| c.class), p.len())).collect();
        eprintln!("plants: {} candidates at {base}: {}", pools.iter().map(Vec::len).sum::<usize>(), counts.join(", "));
    } else {
        plant(&a, args, &root, &clone, &base, pools)?;
    }
    std::fs::remove_dir_all(&work)?;
    Ok(Vec::new())
}

/// A kept plant, with its seed and its control's.
struct Kept {
    c: Candidate,
    nouns: Vec<String>,
    node: String,
    control: String,
    title: String,
    true_title: String,
    /// The question of this plant alone, without the row shape.
    question: String,
}

/// Walks the classes in turn to `--count` kept plants and writes the set.
fn plant(a: &Args, args: &[String], root: &Path, clone: &Path, base: &str, pools: [Vec<Candidate>; 3]) -> Result<()> {
    let out = a.out.as_ref().expect("parse requires --out");
    std::fs::create_dir_all(out)?;
    if std::fs::read_dir(out)?.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "md"))) {
        bail!("{} holds fixtures already; plant into an empty directory", out.display());
    }
    let vault = PathBuf::from(
        std::env::var_os("BP_VAULT").context("set BP_VAULT: the nouns are words its titles rarely hold")?,
    );
    let mut words = vault_words(&vault)?;
    let mut ids: BTreeSet<String> = words.1.clone();
    let mut held: BTreeSet<String> = targets(&root.join("xtask/evals"), out)?.into_keys().collect();
    let mut queues: Vec<VecDeque<Candidate>> = pools.into_iter().map(VecDeque::from).collect();
    let (mut kept, mut discarded) = (Vec::<Kept>::new(), String::new());
    let mut turn = 0;
    while kept.len() < a.count && queues.iter().any(|q| !q.is_empty()) {
        let Some(c) = queues[turn % 3].pop_front() else {
            turn += 1;
            continue;
        };
        match judge(c, clone, &held, &words.0) {
            Ok(mut k) => {
                let dir = if k.c.class == "design" { "projects/bp/designs/components" } else { "shared/learnings" };
                k.node = unique(&mut ids, dir, &k.title);
                k.control = unique(&mut ids, dir, &k.true_title);
                for t in [&k.title, &k.true_title] {
                    for w in title_words(t) {
                        *words.0.entry(w).or_default() += 1;
                    }
                }
                held.extend(seed_of(&k.c).target());
                kept.push(k);
                turn += 1;
            }
            Err((id, reason, why)) => writeln!(discarded, "{id} {reason} {why}")?,
        }
    }
    if kept.len() < a.count {
        eprintln!("plants: {} kept of --count {}: the candidates ran out", kept.len(), a.count);
    }
    let (seed, mut rest) = (a.seed.unwrap_or_default(), (0..kept.len()).collect::<VecDeque<usize>>());
    let mut files: Vec<Vec<&Kept>> = Vec::new();
    for _ in 0..a.parts {
        let Some(first) = rest.pop_front() else { break };
        let Some(at) = rest.iter().position(|&k| kept[k].c.class != kept[first].c.class) else {
            rest.push_front(first);
            break;
        };
        let second = rest.remove(at).expect("a position in range");
        files.push(vec![&kept[first], &kept[second]]);
    }
    files.extend(rest.into_iter().map(|i| vec![&kept[i]]));
    for (i, group) in files.iter().enumerate() {
        std::fs::write(out.join(format!("plant-{}.md", i + 1)), plant_text(group, seed, base))?;
    }
    for (i, k) in kept.iter().take(a.controls).enumerate() {
        std::fs::write(out.join(format!("control-{}.md", i + 1)), control_text(k, base))?;
    }
    std::fs::write(out.join("discarded.txt"), &discarded)?;
    let vault_head = capture(Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&vault)).unwrap_or_default();
    let command: Vec<String> =
        args.iter().map(|w| if w.contains(['*', '?', ' ']) { format!("'{w}'") } else { w.clone() }).collect();
    std::fs::write(
        out.join("generated.txt"),
        format!("command: cargo xtask plants {}\nseed: {seed}\nbase: {base}\nvault: {vault_head}\n", command.join(" ")),
    )?;
    let classes: BTreeMap<&str, usize> = kept.iter().fold(BTreeMap::new(), |mut m, k| {
        *m.entry(k.c.class).or_default() += 1;
        m
    });
    eprintln!(
        "plants: {} kept in {} files {classes:?}, {} discarded",
        kept.len(),
        files.len(),
        discarded.lines().count()
    );
    Ok(())
}

/// The plant a candidate makes, its node ids still to give, or the reason the planter discards it.
fn judge(
    c: Candidate,
    clone: &Path,
    held: &BTreeSet<String>,
    words: &BTreeMap<String, usize>,
) -> Result<Kept, (String, &'static str, String)> {
    let id = c.id.clone();
    sound(&c, clone, held).map_err(|(reason, why)| (id.clone(), reason, why))?;
    let t = crate::plants_text::text(&c).map_err(|why| (id.clone(), "indistinct", why))?;
    let nouns = nouns(&t.clause, &c, words);
    if nouns.len() < 2 {
        return Err((id, "indistinct", "fewer than two nouns a search finds alone".into()));
    }
    let (title, true_title, question) = (t.title, t.true_title, t.question);
    let k = Kept { c, nouns, node: String::new(), control: String::new(), title, true_title, question };
    let (genre, class, nouns) = (k.c.genre.into(), k.c.class.into(), k.nouns.clone());
    let seed = Seed { text: format!("---\n{}", seed_text(&k, false)), genre, class, nouns, ..seed_of(&k.c) };
    match crate::plants_text::breaks(&k.question, &seed) {
        Some(why) => Err((id, "indistinct", why)),
        None => Ok(k),
    }
}

/// Why a candidate cannot be a plant before its words are read: no sentence, no command, false at the base, a
/// cited file that holds the truth, or a target already planted.
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
    if let Some(why) = crate::plants_verify::settle(clone, &seed) {
        return Err(("false-at-base", why));
    }
    if c.class == "two-hop" {
        let cited = std::fs::read_to_string(clone.join(c.cited.as_deref().unwrap_or_default())).unwrap_or_default();
        if c.matches.iter().any(|m| settles(&cited, &Seed { matches: vec![m.clone()], ..Seed::default() }).is_none()) {
            return Err(("indistinct", "the cited file holds the true word".into()));
        }
    }
    match seed.target() {
        Some(t) if held.contains(&t) => return Err(("indistinct", format!("the target {t} is planted already"))),
        None => return Err(("no-command", "the command names no .rs or .md path".into())),
        Some(_) => {}
    }
    Ok(())
}

/// The settle part of a candidate, as a fixture group carries it.
fn seed_of(c: &Candidate) -> Seed {
    Seed { settled_by: c.settle.clone(), matches: c.matches.clone(), unless: c.unless.clone(), ..Seed::default() }
}

/// Up to three words of the subject clause that the vault's titles and ids, and the seeds kept so far, hold at most
/// [`RARE`] times, rarest first; none is the true or the flipped word.
fn nouns(clause: &str, c: &Candidate, words: &BTreeMap<String, usize>) -> Vec<String> {
    let tokens: BTreeSet<String> = c.matches.iter().chain(&c.unless).flat_map(|w| title_words(w)).collect();
    let mut seen = BTreeSet::new();
    let mut found: Vec<(usize, usize, String)> = clause
        .split(|ch: char| !ch.is_alphanumeric())
        .enumerate()
        .filter(|(_, w)| w.len() >= 4 && w.chars().all(char::is_alphabetic))
        .filter(|(_, w)| !PLAIN.contains(&w.to_lowercase().as_str()) && !tokens.contains(&w.to_lowercase()))
        .filter(|(_, w)| seen.insert(w.to_lowercase()))
        .map(|(i, w)| (words.get(&w.to_lowercase()).copied().unwrap_or(0), i, w.to_string()))
        .filter(|(n, _, _)| *n <= RARE)
        .collect();
    found.sort();
    found.into_iter().take(3).map(|f| f.2).collect()
}

/// The lowercase words of a title or an id, as the index's `unicode61` tokenizer splits them.
fn title_words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// How many of the vault's nodes hold each word in their id or title, and every node id.
fn vault_words(vault: &Path) -> Result<(BTreeMap<String, usize>, BTreeSet<String>)> {
    let mut counts = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut stack: Vec<PathBuf> = ["projects", "shared"].iter().map(|d| vault.join(d)).collect();
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let path = e?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(id) = path.strip_prefix(vault)?.to_str().and_then(|p| p.strip_suffix(".md")) else { continue };
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let title = text.lines().take_while(|l| !l.is_empty()).find_map(|l| l.strip_prefix("title:")).unwrap_or("");
            for w in title_words(id).union(&title_words(title)) {
                *counts.entry(w.clone()).or_default() += 1;
            }
            ids.insert(id.to_string());
        }
    }
    Ok((counts, ids))
}

/// `<dir>/<slug of title>`, made unique against `ids`, which then holds it.
fn unique(ids: &mut BTreeSet<String>, dir: &str, title: &str) -> String {
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
fn seed_text(k: &Kept, truth: bool) -> String {
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

fn plant_text(group: &[&Kept], seed: u64, base: &str) -> String {
    let parts: Vec<&str> = group.iter().map(|k| k.question.as_str()).collect();
    let question = if let [one, two] = parts.as_slice() {
        format!("This question has two parts. First: {one} Second: {two} {ROW_SHAPE}")
    } else {
        format!("{} {ROW_SHAPE}", parts.join(" "))
    };
    let mut s = format!("kind: plant\nseed: {seed}\nbase: {base}\n");
    if group.len() > 1 {
        writeln!(s, "parts: {}", group.len()).expect("a String takes a write");
    }
    writeln!(s, "question: {question}").expect("a String takes a write");
    for k in group {
        let c = &k.c;
        write!(
            s,
            "plant: {}\ngenre: {}\nclass: {}\nsettled_by: {}\nmatch: {}\nunless: {}\nnouns: {}\n",
            k.node,
            c.genre,
            c.class,
            c.settle,
            c.matches.join(" | "),
            c.unless.join(" | "),
            k.nouns.join(" ")
        )
        .expect("a String takes a write");
    }
    for k in group {
        s.push_str("---\n");
        s.push_str(&seed_text(k, false));
    }
    s
}

fn control_text(k: &Kept, base: &str) -> String {
    let question = format!("{} {ROW_SHAPE}", k.question);
    format!(
        "kind: control\nbase: {base}\nquestion: {question}\nplant: {}\nnouns: {}\n---\n{}",
        k.control,
        k.nouns.join(" "),
        seed_text(k, true)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_fixture::Fixture;
    use crate::eval_plant::Research;
    use crate::plants_flip::{flip_name, flip_negation, flip_number};
    use crate::plants_verify::holds;

    fn candidate(class: &'static str) -> Candidate {
        Candidate {
            id: format!("{class}:xtask/src/a.rs:3:Number:160"),
            class,
            genre: "Number",
            source: "xtask/src/b.rs".into(),
            cited: Some("xtask/src/a.rs".into()),
            kind: "doc",
            claim: "It stops after 160 tested candidates.".into(),
            flipped: "It stops after 320 tested candidates.".into(),
            token: "160".into(),
            at: 15,
            swap: "320".into(),
            form: "clause",
            subject: String::new(),
            unit: String::new(),
            matches: vec!["160".into()],
            unless: vec!["320".into()],
            settle: "grep -nF 'It stops after' xtask/src/b.rs".into(),
        }
    }

    fn kept(class: &'static str, ids: &mut BTreeSet<String>) -> Kept {
        let c = candidate(class);
        let t = crate::plants_text::text(&c).unwrap();
        let (title, true_title, question) = (t.title, t.true_title, t.question);
        let dir = if class == "design" { "projects/bp/designs/components" } else { "shared/learnings" };
        let (node, control) = (unique(ids, dir, &title), unique(ids, dir, &true_title));
        Kept { c, nouns: vec!["stops".into(), "tested".into()], node, control, title, true_title, question }
    }

    #[test]
    fn the_number_flip_is_never_zero_and_keeps_its_rule() {
        for n in 0..300 {
            for r in 0..4 {
                let f = flip_number(n, r);
                assert!(f != 0 && f != n, "{n} -> {f}");
                if n < 10 {
                    assert_eq!(f.abs_diff(n), 1, "{n} -> {f}: one added or taken under ten");
                } else {
                    assert!(f == 2 * n || 2 * f == n, "{n} -> {f}: doubled or halved");
                }
            }
        }
    }

    #[test]
    fn the_negation_flip_swaps_and_never_deletes() {
        let s = "The walk never takes a backward hop.";
        assert_eq!(flip_negation(s, 9, "never"), "The walk always takes a backward hop.");
        let s = "The cap is fixed at build time.";
        let f = flip_negation(s, 8, "is");
        assert_eq!(f, "The cap is not fixed at build time.");
        let words = |t: &str| t.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        for w in words(s) {
            assert!(words(&f).contains(&w), "`{w}` survives the flip");
        }
        assert!(flip_negation("It always runs.", 3, "always").contains("never"));
    }

    #[test]
    fn the_name_flip_draws_from_the_same_file_and_not_the_sentence() {
        let file =
            "//! `bp-design` and `bp-review` load `ROLE_CAP`.\n/// `walk_for`\nconst X: &str = \"`in_a_literal`\";\n";
        let sentence = "The ledger names `bp-design` and `ROLE_CAP`.";
        let mut seen = BTreeSet::new();
        for r in 0..8 {
            let other = flip_name(file, sentence, r).unwrap();
            assert!(file.contains(&format!("`{other}`")) && !sentence.contains(&other), "{other}");
            seen.insert(other);
        }
        assert_eq!(seen, BTreeSet::from(["bp-review".to_string(), "walk_for".to_string()]), "prose only, no literal");
        assert_eq!(flip_name("`bp-design`", "`bp-design`", 0), None, "no other name in the file");
    }

    #[test]
    fn verify_normalises_digit_separators() {
        for (text, word) in
            [("cap is 1,500 tokens", "1500"), ("1_500", "1,500"), ("1500", "1_500"), ("a 300s wait", "300")]
        {
            assert!(holds(text, word), "{text} holds {word}");
        }
        for (text, word) in
            [("Budget: 15000", "1500"), ("Budget: 15,000", "1500"), ("v1.500", "500"), ("x1500", "1500")]
        {
            assert!(!holds(text, word), "{text} does not hold {word}");
        }
        assert!(holds("The walk NEVER takes", "never takes"), "words match in any case");
        assert!(!holds("whenever takes", "never takes"), "a word inside another is another word");
    }

    #[test]
    fn verify_refuses_a_settle_that_prints_the_flip() {
        let dir = tempfile::tempdir().unwrap();
        let seed = |cmd: &str| Seed {
            settled_by: cmd.into(),
            matches: vec!["1500".into()],
            unless: vec!["3000".into()],
            ..Seed::default()
        };
        assert_eq!(crate::plants_verify::settle(dir.path(), &seed("echo 'cap: 1,500'")), None);
        let flip = crate::plants_verify::settle(dir.path(), &seed("echo 'cap: 1,500 or 3_000'")).unwrap();
        assert!(flip.contains("unless word `3000`"), "{flip}");
        let none = crate::plants_verify::settle(dir.path(), &seed("echo 'cap: 15000'")).unwrap();
        assert!(none.contains("no match word"), "{none}");
        assert!(
            crate::plants_verify::settle(dir.path(), &seed("false")).is_some(),
            "a failing command settles nothing"
        );
    }

    #[test]
    fn a_discarded_line_has_the_shape_the_block_reads() {
        let line = |reason: &str, why: &str| format!("{} {reason} {why}", candidate("two-hop").id);
        for reason in ["no-command", "false-at-base", "not-in-clone", "indistinct"] {
            let l = line(reason, "free text");
            let (id, rest) = l.split_once(' ').unwrap();
            assert!(!id.is_empty() && !id.contains(' '));
            assert!(rest == reason || rest.starts_with(&format!("{reason} ")), "{l}");
        }
    }

    #[test]
    fn one_plant_per_settle_target_across_sets() {
        let evals = tempfile::tempdir().unwrap();
        let mut ids = BTreeSet::new();
        for set in ["bp-research-plants", "bp-research-holdout"] {
            let dir = evals.path().join(set);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plant-1.md"), plant_text(&[&kept("design", &mut ids)], 7, "b")).unwrap();
        }
        let held = targets(evals.path(), &evals.path().join("bp-research-holdout")).unwrap();
        assert_eq!(held["xtask/src/b.rs"], ["bp-research-holdout/plant-1", "bp-research-plants/plant-1"]);
        let other = evals.path().join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("plant-1.md"), plant_text(&[&kept("two-hop", &mut ids)], 7, "b")).unwrap();
        assert_eq!(targets(evals.path(), &other).unwrap()["xtask/src/b.rs"].len(), 3, "a set outside evals counts too");
    }

    #[test]
    fn a_control_is_written_at_its_true_slug() {
        let k = kept("two-hop", &mut BTreeSet::new());
        let f = Fixture::parse("control-1", &control_text(&k, "b")).unwrap();
        let r = f.research.unwrap();
        assert!(r.control && f.kind.as_deref() == Some("control"));
        assert_eq!(r.plants[0].node, "shared/learnings/it-stops-after-160-tested-candidates");
        assert_eq!(k.node, "shared/learnings/it-stops-after-320-tested-candidates");
        assert_eq!(r.plants[0].field("title").as_deref(), Some("It stops after 160 tested candidates"));
        assert!(r.plants[0].text.contains("160 tested") && !r.plants[0].text.contains("320"));
    }

    #[test]
    fn a_two_part_fixture_parses_two_groups_and_a_mismatch_refuses() {
        let mut ids = BTreeSet::new();
        let (a, b) = (kept("design", &mut ids), kept("two-hop", &mut ids));
        let text = plant_text(&[&a, &b], 7, "b");
        let f = Fixture::parse("plant-1", &text).unwrap();
        let r = f.research.unwrap();
        assert_eq!(r.plants.iter().map(|p| p.class.as_str()).collect::<Vec<_>>(), ["design", "two-hop"]);
        assert!(r.plants[0].text.starts_with("---\nkind: design\n") && r.plants[1].text.contains("kind: learning"));
        assert!(text.lines().any(|l| l == "parts: 2"));
        assert!(Fixture::parse("x", &text.replace("parts: 2", "parts: 3")).is_err(), "three parts, two groups");
        let one_seed = text.rsplitn(3, "\n---\n").nth(2).unwrap().to_string();
        assert!(Fixture::parse("x", &one_seed).is_err(), "two groups, one seed");
    }

    #[test]
    fn the_parser_refuses_a_plant_that_breaks_the_rules() {
        let k = kept("design", &mut BTreeSet::new());
        let text = plant_text(&[&k], 7, "b");
        assert!(Research::parse("plant-1", &text).unwrap().is_some());
        let drop =
            |key: &str| text.lines().filter(|l| !l.starts_with(&format!("{key}:"))).collect::<Vec<_>>().join("\n");
        for key in ["genre", "class", "plant", "question", "settled_by", "seed", "base", "nouns"] {
            assert!(Fixture::parse("x", &drop(key)).is_err(), "a plant with no {key}:");
        }
        let shapeless = text.replace(&format!(" {ROW_SHAPE}"), "");
        assert!(Fixture::parse("x", &shapeless).is_err(), "a question without the row shape");
        let named = text.replace("what is the figure?", "what is the figure in It stops after 320 tested candidates?");
        assert!(Fixture::parse("x", &named).is_err(), "a question that names the seed's title");
        let id = text.replace("what is the figure?", &format!("what is the figure in {}?", k.node));
        assert!(Fixture::parse("x", &id).is_err(), "a question that names the seed's node id");
        assert!(Fixture::parse("x", &text.replace("nouns: stops tested", "nouns: stops absent")).is_err());
        assert!(Fixture::parse("x", &text.replace("genre: Number", "genre: Size")).is_err());
        let control = control_text(&k, "b");
        assert!(Fixture::parse("c", &control).is_ok());
        let settled = control.replace("nouns:", "settled_by: grep x a.rs\nnouns:");
        assert!(Fixture::parse("c", &settled).is_err(), "a control carries no settled_by:");
        let review = "kind: control\nbase: b\nhead: h\ntitle: T\ntruth:\n---\nbody\n";
        assert!(Fixture::parse("c", review).unwrap().research.is_none(), "a review control keeps its rules");
    }

    #[test]
    fn a_written_plant_round_trips_through_the_parser() {
        let k = kept("measurement", &mut BTreeSet::new());
        let r = Fixture::parse("plant-1", &plant_text(&[&k], 7, "b")).unwrap().research.unwrap();
        let p = &r.plants[0];
        assert_eq!(
            (r.seed, r.base.as_str(), p.genre.as_str(), p.class.as_str()),
            (Some(7), "b", "Number", "measurement")
        );
        assert_eq!((p.matches.clone(), p.unless.clone()), (vec!["160".to_string()], vec!["320".to_string()]));
        assert_eq!(p.target().as_deref(), Some("xtask/src/b.rs"));
        assert!(r.question.ends_with(ROW_SHAPE) && p.nouns.iter().all(|n| r.question.contains(n.as_str())));
        assert!(p.text.contains("https://"), "a seed names a URL");
    }

    #[test]
    fn a_backticked_name_is_a_whole_token_never_a_prefix() {
        let spans = |t: &str| crate::plants_flip::spans(t).into_iter().map(|s| s.1.to_string()).collect::<Vec<_>>();
        assert_eq!(spans("`a b` then `ok_one`, and `two_x`"), ["ok_one", "two_x"]);
        assert_eq!(spans("a stray ` tick, then `whole_token`"), ["whole_token"]);
        assert_eq!(flip_name("//! `bp-design` and `bp-design-x`", "`bp-design-x`", 0), None, "a prefix is no other");
        assert!(
            !crate::plants_flip::ident("Stop:") && crate::plants_flip::ident("bp_vault::Walk"),
            "a label is no name"
        );
    }

    #[test]
    fn the_question_and_title_say_the_fact_in_the_sentences_words() {
        let mut c = candidate("design");
        c.claim = "A chunk keeps the start-up small (#40: about 45 s each inside one call); it holds 8 candidates of one package.".into();
        c.at = c.claim.find(" 8 ").unwrap() + 1;
        (c.token, c.swap) = ("8".into(), "16".into());
        let t = crate::plants_text::text(&c).unwrap();
        assert_eq!(t.title, "It holds 16 candidates of one package");
        assert_eq!(t.true_title, "It holds 8 candidates of one package");
        assert!(
            t.question.starts_with("\"it holds ___ candidates of one package\": what is the figure?"),
            "{}",
            t.question
        );
        let mut joined = c.clone();
        joined.claim = joined.claim.replace("); it", "), so it");
        joined.at += 3;
        assert!(crate::plants_text::text(&joined).is_err(), "a clause that opens with `so` is a fragment");
        let mut k = candidate("design");
        (k.form, k.unit, k.subject) = ("const", "GAP".into(), "the label module of the bp-diagram crate".into());
        k.claim = "`GAP` in the label module of the bp-diagram crate is 8 (the stroke stays this far: 6 px).".into();
        k.at = k.claim.find(" 8 ").unwrap() + 1;
        (k.token, k.swap) = ("8".into(), "9".into());
        let t = crate::plants_text::text(&k).unwrap();
        assert_eq!(t.title, "GAP in the label module of the bp-diagram crate is 9", "cut at ` (`, never inside it");
        assert!(t.question.starts_with("What is the value of `GAP` in the label module of the bp-diagram crate"));
        c.claim = format!("{} (", "word ".repeat(30));
        c.at = 0;
        c.token = "word".into();
        assert_eq!(crate::plants_text::text(&c).err().as_deref(), Some("no clause boundary"));
    }

    #[test]
    fn verify_refuses_a_cut_title_an_unbalanced_bracket_or_an_absent_noun() {
        let k = kept("design", &mut BTreeSet::new());
        let text = plant_text(&[&k], 7, "b");
        let check = |t: &str| {
            let r = Research::parse("p", t).unwrap().unwrap();
            crate::plants_text::breaks(&r.question, &r.plants[0])
        };
        assert_eq!(check(&text), None);
        let cut = text
            .replace("title: \"It stops after 320 tested candidates\"", "title: \"It stops after 320 tested candid\"");
        assert!(check(&cut).unwrap().contains("no clause"), "a mid-word cut");
        let open = text.replace(
            "title: \"It stops after 320 tested candidates\"",
            "title: \"It stops (after 320 tested candidates\"",
        );
        assert!(check(&open).is_some(), "an unbalanced bracket");
        let joined = text.replace("title: \"It stops", "title: \"And it stops");
        assert!(check(&joined).is_some(), "a title that opens with a joining word");
        let asked = text.replace("___ tested candidates", "___ tested items");
        assert!(check(&asked).is_some(), "a question that drops the clause");
        let mut r = Research::parse("p", &text).unwrap().unwrap();
        r.plants[0].nouns.push("absent".into());
        let why = crate::plants_text::breaks(&r.question, &r.plants[0]).unwrap();
        assert!(why.contains("`absent`"), "a noun absent from the question: {why}");
    }
}
