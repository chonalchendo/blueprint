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
//! admits no other design slug. A `two-hop` is a learning that cites a file whose doc names, in backticks, the file
//! whose one constant holds the truth.
//!
//! The planter walks the classes in turn, measurement, design, two-hop, and takes each class's next plant, so a
//! class that discards more candidates still gets its share. It judges each candidate (`plants_judge.rs`) and keeps
//! it, or writes one line to `discarded.txt`: `<candidate id> <reason>` and free text. The reason is `no-command` (no
//! command reproduces the figure), `false-at-base` (the command's output lacks the true word or holds the flip),
//! `not-in-clone` (a named file the clone does not hold) or `indistinct`: the cited file holds the true word, the
//! settle line is planted in a `bp-research-*` set already, the words are weak or false (`match word too short`),
//! the sentence has no clause boundary, the question leaks (`plants_leak.rs`), or fewer than two nouns find the seed.
//!
//! It stops at `--count` plants. The first `--parts` files each pair two consecutive kept plants of different classes
//! in one question. The first `--controls` plants each get a control: the true seed, at the slug of its true title,
//! with the plant's settle command and its true word. It writes into `--out` in place: the files of an earlier run go
//! first, and the `--out` set's own settle lines are not held, so the command `generated.txt` records (with the
//! seed, the base and the vault's head) makes the set again byte for byte. Each plant group carries its settle line as
//! `target:`, so the holdout seal compares lines across sets, not files.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::eval::usage;
use crate::eval_plant::ROW_SHAPE;
use crate::eval_sandbox::capture;
use crate::plants_judge::{Bench, Kept, Search, judge, seed_text, title_words};
use crate::plants_leak::Corpus;
use crate::plants_pool::{Candidate, candidates, sources};
use crate::plants_verify::{Clones, targets};

const USAGE: &str = "usage: cargo xtask plants --list --from <glob>... --seed <s> [--base <sha>] | cargo xtask plants --from <glob>... --count <k> --seed <s> --out <dir> [--controls <n>] [--parts <m>] [--base <sha>] | cargo xtask plants --verify --fixtures <dir>";

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
    let mut clones = Clones::new(&root, &work);
    let clone = clones.at(&base)?;
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
        plant(&a, args, &root, &mut clones, &base, pools)?;
    }
    std::fs::remove_dir_all(&work)?;
    Ok(Vec::new())
}

/// Walks the classes in turn to `--count` kept plants and writes the set.
fn plant(
    a: &Args,
    args: &[String],
    root: &Path,
    clones: &mut Clones,
    base: &str,
    pools: [Vec<Candidate>; 3],
) -> Result<()> {
    let out = a.out.as_ref().expect("parse requires --out");
    std::fs::create_dir_all(out)?;
    let vault = PathBuf::from(
        std::env::var_os("BP_VAULT").context("set BP_VAULT: the nouns are words its titles rarely hold")?,
    );
    let (mut words, mut ids) = vault_words(&vault)?;
    let mut held: BTreeSet<String> = targets(&root.join("xtask/evals"), out, false, clones)?.into_keys().collect();
    let clone = clones.at(base)?;
    let corpus = Corpus::of(&clone)?;
    let mut search = Search::new(root)?;
    let mut queues: Vec<VecDeque<Candidate>> = pools.into_iter().map(VecDeque::from).collect();
    let (mut kept, mut discarded) = (Vec::<Kept>::new(), String::new());
    let mut turn = 0;
    while kept.len() < a.count && queues.iter().any(|q| !q.is_empty()) {
        let Some(c) = queues[turn % 3].pop_front() else {
            turn += 1;
            continue;
        };
        let control = kept.len() < a.controls;
        let mut bench =
            Bench { clone: &clone, held: &held, words: &words, ids: &mut ids, corpus: &corpus, search: &mut search };
        match judge(c, &mut bench, control)? {
            Ok(k) => {
                let titles = if control { vec![&k.title, &k.true_title] } else { vec![&k.title] };
                for w in titles.into_iter().flat_map(|t| title_words(t)) {
                    *words.entry(w).or_default() += 1;
                }
                held.insert(k.target.clone());
                kept.push(k);
                turn += 1;
            }
            Err((id, reason, why)) => writeln!(discarded, "{id} {reason} {why}")?,
        }
    }
    // The set is made again in place: the files of an earlier run go first, so a smaller set leaves none behind.
    for e in std::fs::read_dir(out)? {
        let path = e?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let md = path.extension().is_some_and(|x| x == "md");
        let ours = (name.starts_with("plant-") || name.starts_with("control-")) && md;
        if ours || name == "discarded.txt" || name == "generated.txt" {
            std::fs::remove_file(&path)?;
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
    for (i, k) in kept.iter().filter(|k| !k.control.is_empty()).enumerate() {
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
            "plant: {}\ngenre: {}\nclass: {}\nsettled_by: {}\ntarget: {}\nmatch: {}\nunless: {}\nnouns: {}\n",
            k.node,
            c.genre,
            c.class,
            c.settle,
            k.target,
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
        "kind: control\nbase: {base}\nquestion: {question}\nplant: {}\nsettled_by: {}\nmatch: {}\nnouns: {}\n---\n{}",
        k.control,
        k.c.settle,
        k.c.matches.join(" | "),
        k.nouns.join(" "),
        seed_text(k, true)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_fixture::Fixture;
    use crate::eval_plant::Research;
    use crate::eval_plant::{Seed, words};
    use crate::plants_flip::{Kinds, flip_name, flip_negation, flip_number};
    use crate::plants_judge::{seed_of, unique, worded};
    use crate::plants_leak::{Corpus, hop_leaks, leaks};
    use crate::plants_verify::{holds, key};

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
            label: String::new(),
            line: "//! It stops after 160 tested candidates.".into(),
            near: Vec::new(),
            matches: vec!["160".into()],
            unless: vec!["320".into()],
            settle: "grep -F -e 'It stops after' xtask/src/b.rs".into(),
        }
    }

    fn kept(class: &'static str, ids: &mut BTreeSet<String>) -> Kept {
        let c = candidate(class);
        let t = crate::plants_text::text(&c).unwrap();
        let (title, true_title, question) = (t.title, t.true_title, t.question);
        let dir = if class == "design" { "projects/bp/designs/components" } else { "shared/learnings" };
        let (node, control) = (unique(ids, dir, &title), unique(ids, dir, &true_title));
        let target = "xtask/src/b.rs:2".into();
        Kept { c, nouns: vec!["stops".into(), "tested".into()], node, control, title, true_title, question, target }
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
    fn the_name_flip_draws_from_the_same_file_and_kind_and_not_the_sentence() {
        let file = "//! `bp-design` and `bp-review` load `ROLE_CAP`, `ROUTE_CAP` and `NAME_MAX`.\n/// `walk_for` and `in_a_literal`\nconst X: &str = \"`walk_back`\";\n";
        let code = "const ROLE_CAP: usize = 1;\nconst ROUTE_CAP: usize = 2;\nconst NAME_MAX: u32 = 3;\nfn walk_for() {}\nfn walk_back() {}\n";
        let kinds = Kinds::of([
            ("crates/bp-design/src/lib.rs", code),
            ("crates/bp-review/src/lib.rs", ""),
            ("crates/bp-ledger/src/lib.rs", ""),
        ]);
        let sentence = "The ledger names `bp-ledger` and `ROLE_CAP` and `bp_read::walk_for`.";
        let swaps =
            |token: &str| (0..8).filter_map(|r| flip_name(file, sentence, token, &kinds, r)).collect::<BTreeSet<_>>();
        let set = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<BTreeSet<_>>();
        assert_eq!(swaps("ROLE_CAP"), set(&["ROUTE_CAP"]), "a usize const for a usize const, never `NAME_MAX: u32`");
        assert_eq!(swaps("bp-ledger"), set(&["bp-design", "bp-review"]), "a crate for a crate");
        assert_eq!(swaps("bp_read::walk_for"), BTreeSet::new(), "`walk_back` is a literal's, not the prose's");
        assert_eq!(kinds.kind("bp_read::walk_for"), Some("fn"), "a path reads as its last part");
        assert_eq!(swaps("quokka-agent"), BTreeSet::new(), "a name of no kind is no Name candidate");
        assert_eq!(kinds.kind("scratch.rs"), None, "a name the clone does not hold has no kind");
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
    fn one_plant_per_settle_line_across_sets() {
        let (evals, code) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::create_dir_all(code.path().join("xtask/src")).unwrap();
        std::fs::write(code.path().join("xtask/src/b.rs"), "//! A line.\n//! It stops after 160 tested candidates.\n")
            .unwrap();
        let mut clones = Clones::new(code.path(), code.path());
        clones.insert("b", code.path());
        let mut ids = BTreeSet::new();
        for set in ["bp-research-plants", "bp-research-holdout"] {
            let dir = evals.path().join(set);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("plant-1.md"), plant_text(&[&kept("design", &mut ids)], 7, "b")).unwrap();
        }
        let holdout = evals.path().join("bp-research-holdout");
        let held = targets(evals.path(), &holdout, true, &mut clones).unwrap();
        assert_eq!(held["xtask/src/b.rs:2"], ["bp-research-holdout/plant-1", "bp-research-plants/plant-1"]);
        let made = targets(evals.path(), &holdout, false, &mut clones).unwrap();
        assert_eq!(made["xtask/src/b.rs:2"], ["bp-research-plants/plant-1"], "the --out set is the one made again");
        let other = evals.path().join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("plant-1.md"), plant_text(&[&kept("two-hop", &mut ids)], 7, "b")).unwrap();
        let all = targets(evals.path(), &other, true, &mut clones).unwrap();
        assert_eq!(all["xtask/src/b.rs:2"].len(), 3, "a set outside evals counts too");
        let seed = |cmd: &str| Seed { settled_by: cmd.into(), ..Seed::default() };
        assert_eq!(key(code.path(), &seed("grep -F 'A line' xtask/src/b.rs")).as_deref(), Some("xtask/src/b.rs:1"));
        assert_eq!(key(code.path(), &seed("wc -l < xtask/src/b.rs")).as_deref(), Some("xtask/src/b.rs"), "a count");
        let out = crate::plants_verify::output(code.path(), &kept("design", &mut ids).c.settle).unwrap();
        assert_eq!(out, "//! It stops after 160 tested candidates.\n", "no line number to match a figure");
    }

    #[test]
    fn verify_refuses_a_target_off_the_settle_line() {
        let code = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(code.path().join("xtask/src")).unwrap();
        std::fs::write(code.path().join("xtask/src/b.rs"), "//! A line.\n//! It stops after 160 tested candidates.\n")
            .unwrap();
        let mut p = seed_of(&kept("design", &mut BTreeSet::new()).c);
        let placed = |p: &Seed| crate::plants_verify::placed(code.path(), p);
        for (line, ok) in
            [("xtask/src/b.rs:2", true), ("xtask/src/b.rs:1", false), ("xtask/src/b.rs", false), ("", false)]
        {
            p.settle_line = line.into();
            assert_eq!(placed(&p).is_ok(), ok, "target: {line}");
        }
        p.settled_by = "wc -l < xtask/src/b.rs".into();
        p.settle_line = "xtask/src/b.rs".into();
        assert_eq!(placed(&p).as_deref(), Ok("xtask/src/b.rs"), "a count's target is its file");
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
        assert_eq!(
            (r.plants[0].settled_by.as_str(), r.plants[0].matches.clone()),
            (k.c.settle.as_str(), vec!["160".into()])
        );
        assert!(r.plants[0].unless.is_empty(), "a control carries the truth, not the flip");
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
        let four = text.replace("what is the figure?", "what is the figure, which stops after 320 tested ones?");
        assert!(Fixture::parse("x", &four).is_err(), "a question that shares four words with the title");
        let three = text.replace("what is the figure?", "what is the figure, which stops after 320?");
        assert!(Fixture::parse("x", &three).is_ok(), "three shared words are no name");
        let id = text.replace("what is the figure?", &format!("what is the figure in {}?", k.node));
        assert!(Fixture::parse("x", &id).is_err(), "a question that names the seed's node id");
        assert!(Fixture::parse("x", &text.replace("nouns: stops tested", "nouns: stops absent")).is_err());
        assert!(Fixture::parse("x", &text.replace("genre: Number", "genre: Size")).is_err());
        let control = control_text(&k, "b");
        assert!(Fixture::parse("c", &control).is_ok());
        let flipped = control.replace("nouns:", "unless: 320\nnouns:");
        assert!(Fixture::parse("c", &flipped).is_err(), "a control carries no unless:");
        let bare = control.lines().filter(|l| !l.starts_with("settled_by:")).collect::<Vec<_>>().join("\n");
        assert!(Fixture::parse("c", &bare).is_err(), "a control carries its settled_by:");
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
        assert_eq!(p.settle_line, "xtask/src/b.rs:2", "the group carries its settle line as target:");
        assert!(r.question.ends_with(ROW_SHAPE) && p.nouns.iter().all(|n| r.question.contains(n.as_str())));
        assert!(p.text.contains("https://"), "a seed names a URL");
    }

    #[test]
    fn a_backticked_name_is_a_whole_token_never_a_prefix() {
        let spans = |t: &str| crate::plants_flip::spans(t).into_iter().map(|s| s.1.to_string()).collect::<Vec<_>>();
        assert_eq!(spans("`a b` then `ok_one`, and `two_x`"), ["ok_one", "two_x"]);
        assert_eq!(spans("a stray ` tick, then `whole_token`"), ["whole_token"]);
        let kinds = Kinds::of([("crates/bp-design/src/lib.rs", ""), ("crates/bp-design-x/src/lib.rs", "")]);
        let file = "//! `bp-design` and `bp-design-x`";
        assert_eq!(flip_name(file, "`bp-design-x`", "bp-design-x", &kinds, 0), None, "a prefix is no other");
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
        let k =
            crate::plants_pool::constants("crates/bp-diagram/src/label.rs", "/// The gap.\npub const GAP: u32 = 18;\n");
        assert_eq!(k[0].claim, "`GAP` in the label module of the bp-diagram crate is 18.", "template words, no doc");
        assert_eq!(k[0].settle, "grep -F 'const GAP:' crates/bp-diagram/src/label.rs");
        let t = crate::plants_text::text(&k[0]).unwrap();
        assert_eq!(t.title, format!("In bp-diagram's src, the label module sets GAP to {}", k[0].swap));
        let tests =
            crate::plants_pool::constants("crates/bp-install/tests/ledger.rs", "const ROLE_CAP: usize = 750;\n");
        assert_eq!(tests[0].subject, "the ledger file in bp-install's tests", "a test file is no module of the crate");
        assert!(t.question.starts_with("What is the value of `GAP` in the label module of the bp-diagram crate, and"));
        assert!(!shares_four(&t.question, &t.title), "the question names the seed by no run of its title");
        c.claim = format!("{} (", "word ".repeat(30));
        c.at = 0;
        c.token = "word".into();
        assert_eq!(crate::plants_text::text(&c).err().as_deref(), Some("no clause boundary"));
    }

    fn shares_four(a: &str, b: &str) -> bool {
        let runs = |t: &str| words(t).windows(4).map(|w| w.join(" ")).collect::<BTreeSet<_>>();
        !runs(a).is_disjoint(&runs(b))
    }

    #[test]
    fn a_measurement_names_where_its_file_is_and_no_title_run() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["xtask/src/scratch.rs", "xtask/tests/scratch.rs", "crates/bp-tool/src/eval_result.rs"] {
            std::fs::create_dir_all(dir.path().join(f).parent().unwrap()).unwrap();
            std::fs::write(dir.path().join(f), "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\nl\n").unwrap();
        }
        let files =
            ["xtask/src/scratch.rs", "xtask/tests/scratch.rs", "crates/bp-tool/src/eval_result.rs"].map(String::from);
        let lines: Vec<Candidate> = crate::plants_count::measurements(dir.path(), &files)
            .into_iter()
            .filter(|c| c.id.contains(":lines:"))
            .collect();
        let subjects: Vec<&str> = lines.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(
            subjects,
            [
                "the scratch module in xtask's src",
                "the scratch file in xtask's tests",
                "the eval_result module in bp-tool's src"
            ]
        );
        for c in &lines {
            let t = crate::plants_text::text(c).unwrap();
            assert!(t.question.starts_with(&format!("How many lines does {} have?", c.subject)), "{}", t.question);
            assert!(!shares_four(&t.question, &t.title), "{} / {}", t.question, t.title);
            assert_eq!((c.settle.contains("-n"), c.kind), (false, "count"));
        }
        assert_eq!(
            crate::plants_text::text(&lines[0]).unwrap().true_title,
            "In xtask's src, the scratch module has 12 lines"
        );
    }

    #[test]
    fn a_question_that_a_grep_settles_leaks() {
        let corpus = Corpus::from_texts([
            "//! The walk takes `12` steps\n//! before it stops, and `bp-tool` rests.",
            "fn x() {}",
        ]);
        let ask = |q: &str| leaks(&format!("{q} {ROW_SHAPE}"), &["12".to_string()], &corpus);
        assert!(ask("Does \"the walk takes ___ steps\" hold?").is_none(), "the blank parts every run");
        assert!(ask("How far does the walk take steps before it stops?").is_some(), "a run across a line break");
        assert!(ask("Where it stops, and bp-tool rests?").is_some(), "backticks hide no run");
        assert!(ask("How many steps? Is it 12?").unwrap().contains("match word `12`"));
        assert!(ask("How many lines does the walk module have? Give the number and its source, at the repository's current commit.").is_none(), "the request sentence is no run");
        let line = "pub const CHUNK: usize = 20;";
        assert!(hop_leaks(line, &["chunk".into(), "walk".into()], "The doc names a file on chunk and walk.").is_some());
        assert!(hop_leaks(line, &["walk".into(), "pick".into()], "Which `usize` file?").is_some(), "a backticked word");
        assert!(hop_leaks(line, &["walk".into(), "pick".into()], &format!("Which file? {ROW_SHAPE}")).is_none());
    }

    #[test]
    fn weak_or_false_words_are_discarded() {
        let mut c = candidate("design");
        assert_eq!(worded(&c), None);
        (c.token, c.claim, c.at) = ("8".into(), "It stops after 8 tested candidates.".into(), 15);
        assert_eq!(worded(&c), Some("match word too short"));
        let mut t = candidate("design");
        t.line = "//! TODO(bp): It stops after 160 tested candidates.".into();
        assert_eq!(worded(&t), Some("the line holds TODO"));
        let mut one = candidate("design");
        (one.claim, one.token, one.swap, one.at) = ("It keeps 12 copies.".into(), "12".into(), "1".into(), 9);
        assert_eq!(worded(&one), Some("the flip gives 1 before a plural"));
        let mut neg = candidate("design");
        (neg.genre, neg.claim, neg.token, neg.at) = ("Negation", "The cache is never stale.".into(), "is".into(), 10);
        assert_eq!(worded(&neg), Some("the flip sits beside a negation"));
        (neg.claim, neg.token, neg.at) = ("It is not always fast.".into(), "always".into(), 10);
        assert_eq!(worded(&neg), Some("the flip sits beside a negation"));
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
        let unsaid = text.replace("after 320 tested candidates (", "after 330 tested candidates (");
        assert!(check(&unsaid).unwrap().contains("body does not state"), "a body that does not state the flip");
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
