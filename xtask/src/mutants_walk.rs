//! The order and the walk of `cargo xtask mutants` (`mutants.rs`): the added lines a candidate must start on, which
//! candidates it tests, in what groups, and which survivors it keeps. Nothing here runs a command, so the tests drive
//! it with fake outcomes.
//!
//! A chunk is one `cargo mutants` call over at most `CHUNK` candidates of one package. One call per candidate pays
//! the call's start-up each time (#40: 72 to 113 s per `bp-tool` mutant alone, about 45 s each inside one call),
//! and one call for every candidate tests far past `--count` (#42: about six hours for six mutants). A chunk of 8
//! keeps the start-up small and the overshoot at most 7 candidates. A chunk fills only when the order holds a package's
//! candidates together, so the order groups the seeded order by package (#40's seeded order alternates `bp-change` and
//! `bp-tool`: 29 chunks held 36 candidates). `CAP` bounds a pull request whose mutants CI catches: 20 chunks, after
//! which it stops with what it kept.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde_json::Value;

/// Candidates per `cargo mutants` call.
pub(crate) const CHUNK: usize = 8;
/// Candidates tested per pull request before the walk stops short of `--count`.
pub(crate) const CAP: usize = 20 * CHUNK;

/// The end of one pull request's walk: the kept mutants in seed order, the number tested, and whether `CAP` stopped
/// it before `--count`.
pub(crate) struct Walk<'a> {
    pub kept: Vec<&'a Value>,
    pub tested: usize,
    pub capped: bool,
}

/// The candidates in the order a run walks them, seed order grouped by package: sorted by name, shuffled by `seed`,
/// then stably sorted by the position of each package's first candidate in that shuffle.
pub(crate) fn order(mut candidates: Vec<&Value>, seed: u64) -> Vec<&Value> {
    let mut state = seed;
    candidates.sort_by_key(|m| m["name"].as_str().unwrap_or(""));
    for i in (1..candidates.len()).rev() {
        candidates.swap(i, usize::try_from(splitmix(&mut state) % (i as u64 + 1)).expect("at most i"));
    }
    let mut first = BTreeMap::new();
    for (i, m) in candidates.iter().enumerate() {
        first.entry(m["package"].as_str()).or_insert(i);
    }
    candidates.sort_by_key(|m| first[&m["package"].as_str()]);
    candidates
}

/// Walks `order` in chunks and keeps the first `count` survivors, at most one from each function: two mutants of one
/// function are not two independent pieces of evidence. A chunk takes the next candidates that pass `clean` (CI's
/// fmt and clippy), skips those whose function it already keeps, and ends early where the order changes package.
/// `test` gives each chunk member's survival. The walk stops at `count` kept, at `CAP` tested, or at the end of the
/// order. So the kept set depends only on the order and the outcomes, not on where the chunks fall.
pub(crate) fn walk<'a>(
    order: Vec<&'a Value>,
    count: usize,
    mut clean: impl FnMut(&Value) -> bool,
    mut test: impl FnMut(&[&Value]) -> Result<Vec<bool>>,
) -> Result<Walk<'a>> {
    let function = |m: &'a Value| (m["file"].as_str(), m["function"]["function_name"].as_str());
    let mut functions = BTreeSet::new();
    let (mut kept, mut tested) = (Vec::new(), 0);
    let mut rest = order.into_iter().peekable();
    while kept.len() < count {
        if tested >= CAP && rest.peek().is_some() {
            return Ok(Walk { kept, tested, capped: true });
        }
        let mut chunk: Vec<&Value> = Vec::new();
        while chunk.len() < CHUNK.min(CAP - tested) {
            let Some(&m) = rest.peek() else { break };
            if chunk.first().is_some_and(|c| c["package"] != m["package"]) {
                break;
            }
            rest.next();
            if !functions.contains(&function(m)) && clean(m) {
                chunk.push(m);
            }
        }
        if chunk.is_empty() {
            break;
        }
        tested += chunk.len();
        for (m, survived) in chunk.iter().zip(test(&chunk)?) {
            if survived && kept.len() < count && functions.insert(function(m)) {
                kept.push(m);
            }
        }
    }
    Ok(Walk { kept, tested, capped: false })
}

/// `SplitMix64`: a seeded sequence with no dependency, enough for a reproducible shuffle.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The touched crates whose tests fail at the head, when `passes` fails for all of them together: each that fails
/// alone, or all of them when none does. Empty when they pass.
pub(crate) fn failing(crates: &BTreeSet<String>, mut passes: impl FnMut(&[&str]) -> bool) -> Vec<String> {
    let all: Vec<&str> = crates.iter().map(String::as_str).collect();
    if passes(&all) {
        return Vec::new();
    }
    let alone: Vec<String> = all.iter().filter(|c| !passes(&[c])).map(|c| (*c).to_string()).collect();
    if alone.is_empty() { all.iter().map(|c| (*c).to_string()).collect() } else { alone }
}

/// The added (`+`) lines of a unified diff, as (path, line in the new file). A line in a hunk that starts with `+++`
/// is an added line, not a header: the headers come before a file's first hunk.
pub(crate) fn added_lines(diff: &str) -> BTreeSet<(String, usize)> {
    let (mut out, mut file, mut at) = (BTreeSet::new(), None::<String>, None::<usize>);
    for l in diff.lines() {
        if l.starts_with("diff --git ") {
            (file, at) = (None, None);
        } else if let (None, Some(path)) = (at, l.strip_prefix("+++ ")) {
            file = path.strip_prefix("b/").map(str::to_string);
        } else if let Some(hunk) = l.strip_prefix("@@ ") {
            let new = hunk.split_whitespace().find_map(|w| w.strip_prefix('+')).unwrap_or("1");
            at = new.split(',').next().and_then(|n| n.parse().ok());
        } else if let Some(n) = at.as_mut() {
            if l.starts_with('+') {
                out.extend(file.clone().map(|f| (f, *n)));
                *n += 1;
            } else if !l.starts_with('-') && !l.starts_with('\\') {
                *n += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Candidates `m0`, `m1`, … in one package, with the functions given.
    fn candidates(functions: &[&str]) -> Vec<Value> {
        functions
            .iter()
            .enumerate()
            .map(|(i, f)| serde_json::json!({"name": format!("m{i}"), "file": "a.rs", "package": "p", "function": {"function_name": f}}))
            .collect()
    }

    fn distinct(n: usize) -> Vec<Value> {
        candidates(&(0..n).map(|i| format!("f{i}")).collect::<Vec<_>>().iter().map(String::as_str).collect::<Vec<_>>())
    }

    /// Walks `all` with every candidate clean; `survives` names the survivors. It returns the kept names and the
    /// names of each chunk tested.
    fn run<'a>(all: &'a [Value], count: usize, survives: &[&str]) -> (Walk<'a>, Vec<Vec<String>>) {
        let mut chunks = Vec::new();
        let w = walk(
            all.iter().collect(),
            count,
            |_| true,
            |chunk| {
                chunks.push(chunk.iter().map(|m| m["name"].as_str().unwrap().to_string()).collect());
                Ok(chunk.iter().map(|m| survives.contains(&m["name"].as_str().unwrap())).collect())
            },
        )
        .unwrap();
        (w, chunks)
    }

    fn names(w: &Walk) -> Vec<String> {
        w.kept.iter().map(|m| m["name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_pick_is_seeded_and_takes_one_mutant_per_function() {
        let all: Vec<Value> = (0..9)
            .map(|i| serde_json::json!({"name": format!("m{i}"), "file": "a.rs", "function": {"function_name": format!("f{}", i % 3)}}))
            .collect();
        let pick = |count, seed| {
            walk(order(all.iter().collect(), seed), count, |_| true, |c| Ok(vec![true; c.len()])).unwrap()
        };
        let names = |seed| pick(2, seed).kept.iter().map(|m| m["name"].to_string()).collect::<Vec<_>>();
        assert_eq!(names(7), names(7), "one seed, one pick");
        assert_ne!((0..20).map(names).collect::<BTreeSet<_>>().len(), 1, "the seed moves the pick");
        let three = pick(9, 7).kept;
        let functions: BTreeSet<_> = three.iter().map(|m| m["function"]["function_name"].as_str()).collect();
        assert_eq!((three.len(), functions.len()), (3, 3), "one per function, however many survive");
    }

    #[test]
    fn the_walk_stops_at_the_count() {
        let all = distinct(20);
        let (w, chunks) = run(&all, 2, &["m2", "m4"]);
        assert_eq!(names(&w), ["m2", "m4"]);
        assert_eq!(
            (w.tested, chunks.len(), w.capped),
            (8, 1, false),
            "the third and fifth survive: the first chunk only"
        );
        let (w, chunks) = run(&all, 3, &["m2"]);
        assert_eq!(
            (names(&w), w.tested, chunks.len(), w.capped),
            (vec!["m2".into()], 20, 3, false),
            "the order runs out"
        );
    }

    #[test]
    fn the_count_is_reached_in_the_second_chunk() {
        let all = distinct(20);
        let (w, chunks) = run(&all, 2, &["m3", "m10", "m12"]);
        assert_eq!(names(&w), ["m3", "m10"], "m12 survives too, past the count");
        assert_eq!((w.tested, chunks.len()), (16, 2));
        assert_eq!(chunks[1].first().map(String::as_str), Some("m8"));
        let mut split = distinct(6);
        for m in &mut split[3..] {
            m["package"] = "q".into();
        }
        let (w, chunks) = run(&split, 2, &["m1", "m4"]);
        assert_eq!(names(&w), ["m1", "m4"]);
        assert_eq!(chunks, [vec!["m0", "m1", "m2"], vec!["m3", "m4", "m5"]], "a chunk ends where the package changes");
    }

    #[test]
    fn the_order_groups_the_seeded_order_by_package() {
        // One package: the order is the seed's shuffle alone, which does not read the package.
        let one = distinct(16);
        let seeded: Vec<String> =
            order(one.iter().collect(), 5).iter().map(|m| m["name"].as_str().unwrap().to_string()).collect();
        let mut all = one.clone();
        for (i, m) in all.iter_mut().enumerate() {
            m["package"] = if i % 3 == 0 { "q" } else { "p" }.into();
        }
        let package = |n: &String| all.iter().find(|m| m["name"] == n.as_str()).unwrap()["package"].to_string();
        let first = package(&seeded[0]);
        let in_seed = |same: bool| seeded.iter().filter(|n| (package(n) == first) == same).cloned().collect::<Vec<_>>();
        let grouped: Vec<String> =
            order(all.iter().collect(), 5).iter().map(|m| m["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(grouped, [in_seed(true), in_seed(false)].concat(), "each package in seed order, first-seen first");
        let mut chunks = Vec::new();
        walk(
            order(all.iter().collect(), 5),
            99,
            |_| true,
            |c| {
                chunks.push(c.len());
                Ok(vec![false; c.len()])
            },
        )
        .unwrap();
        let sizes = if first == "\"p\"" { [8, 2, 6] } else { [6, 8, 2] };
        assert_eq!(chunks, sizes, "10 in p and 6 in q: the chunks fill, and only a package's end cuts one short");
    }

    #[test]
    fn a_kept_function_is_not_tested_twice() {
        let mut fs = vec!["f", "f"];
        fs.extend(["g0", "g1", "g2", "g3", "g4", "g5", "f", "h"]);
        let all = candidates(&fs);
        let (w, chunks) = run(&all, 3, &["m0", "m1", "m8", "m9"]);
        assert_eq!(names(&w), ["m0", "m9"], "m1 survives in m0's function: one per function");
        assert_eq!(chunks[1], ["m9"], "m8 is in f, which the first chunk kept: it is not tested");
    }

    #[test]
    fn the_cap_stops_a_pull_request_whose_mutants_are_all_caught() {
        let all = distinct(200);
        let (w, chunks) = run(&all, 6, &[]);
        assert_eq!((w.kept.len(), w.tested, chunks.len(), w.capped), (0, CAP, 20, true));
        let (w, _) = run(&all, 1, &["m159"]);
        assert_eq!((names(&w), w.tested, w.capped), (vec!["m159".into()], CAP, false), "kept at the last test");
        let exact = distinct(CAP);
        let (w, _) = run(&exact, 1, &[]);
        assert!(!w.capped, "the order ran out at the cap: not capped");
    }

    #[test]
    fn a_failing_baseline_names_its_crates() {
        let crates = BTreeSet::from(["a".to_string(), "b".to_string(), "c".to_string()]);
        // A fake `cargo test -p …`: the crate sets it was run on, and the crates `failing` names.
        let fake = |fails: &dyn Fn(&[&str]) -> bool| {
            let mut calls = Vec::new();
            let named = failing(&crates, |cs| {
                calls.push(cs.join(" "));
                !fails(cs)
            });
            (named, calls)
        };
        let (named, calls) = fake(&|_| false);
        assert!(named.is_empty() && calls == ["a b c"], "they pass: one run over every touched crate");
        let (named, calls) = fake(&|cs| cs.contains(&"b"));
        assert_eq!(
            (named, calls),
            (vec!["b".to_string()], vec!["a b c", "a", "b", "c"].into_iter().map(String::from).collect()),
            "b fails alone"
        );
        assert_eq!(fake(&|cs| cs.len() > 1).0, ["a", "b", "c"], "they fail only together: all are named");
    }

    #[test]
    fn a_candidate_counts_only_on_an_added_line() {
        let diff = "diff --git a/a.rs b/a.rs\nindex 1..2 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -3,4 +3,4 @@ fn f() {\n x\n-y\n+z\n+++w\n w\n\\ No newline at end of file\ndiff --git a/gone.rs b/gone.rs\n--- a/gone.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-q\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -10,0 +11,2 @@\n+m\n+n\n";
        let got: Vec<(String, usize)> = added_lines(diff).into_iter().collect();
        let want = [("a.rs", 4), ("a.rs", 5), ("b.rs", 11), ("b.rs", 12)].map(|(f, n)| (f.to_string(), n));
        assert_eq!(got, want, "context line 3 and 6 and the deleted file add nothing; `+++w` in a hunk is added");
    }
}
