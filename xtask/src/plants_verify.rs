//! `cargo xtask plants --verify --fixtures <dir>` (31, design change 2): no block needs a model to check a plant's
//! truth. It clones every distinct `base:` once and runs each plant's `settled_by:` at the clone's root. It refuses a
//! plant whose output holds no `match:` word or holds an `unless:` word, matched by [`holds`], and a seed whose URL
//! the clone holds. It refuses a plant whose settle target another plant of any `xtask/evals/bp-research-*` set holds.
//! Then it seeds every fixture into one scratch copy of `$BP_VAULT` (`eval_sandbox::seed`) and refuses a plant that
//! `bp search <noun> --json` does not return in its first five rows, for each noun alone: `bp search` needs every term
//! and has no stemming (`m8-pilot/result.md`, finding 3). It refuses a question or a title that breaks the rules
//! the planter writes them by (`plants_text::breaks`). It prints one line per fixture.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::eval_fixture::Fixture;
use crate::eval_plant::Seed;
use crate::eval_sandbox::capture;

/// Rows of `bp search` a plant must sit in, for each noun.
const TOP: usize = 5;

/// Whether `text` holds `word` as a whole word, in any case, with digit separators normalised: `1,500`, `1_500` and
/// `1500` are one word, and `1500` is not `15000` (`m8-pilot/result.md`, finding 6). A unit after a figure (`300s`)
/// still ends it.
pub fn holds(text: &str, word: &str) -> bool {
    let (text, word) = (normalise(&text.to_lowercase()), normalise(&word.to_lowercase()));
    if word.is_empty() {
        return false;
    }
    let figure = word.bytes().all(|b| b.is_ascii_digit());
    text.match_indices(word.as_str()).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = &text[at + word.len()..];
        let unit = ["ms", "s", "px"].iter().find(|u| figure && after.starts_with(*u)).map_or(0, |u| u.len());
        let next = after[unit..].chars().next();
        let part = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        // A figure inside a decimal or a version (`1.500`, `v2.15`) is another number.
        let dotted = figure
            && (text[..at].ends_with('.')
                || after.starts_with(['.']) && after[1..].starts_with(|c: char| c.is_ascii_digit()));
        !part(before) && !part(next) && !dotted
    })
}

/// `text` with every `,` or `_` that sits between a digit and three digits removed.
fn normalise(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        let sep = matches!(c, ',' | '_')
            && i > 0
            && b[i - 1].is_ascii_digit()
            && b.len() >= i + 4
            && b[i + 1..i + 4].iter().all(u8::is_ascii_digit)
            && !b.get(i + 4).is_some_and(u8::is_ascii_digit);
        if !sep {
            out.push(c);
        }
    }
    out
}

/// Why the output of a plant's settle command does not settle it, or `None` when it does.
pub fn settles(output: &str, seed: &Seed) -> Option<String> {
    if !seed.matches.iter().any(|m| holds(output, m)) {
        return Some(format!("the settle output holds no match word of {:?}", seed.matches));
    }
    seed.unless.iter().find(|u| holds(output, u)).map(|u| format!("the settle output holds the unless word `{u}`"))
}

/// Runs a plant's settle command at the root of `clone` and says why it does not settle the plant, if it does not.
pub fn settle(clone: &Path, seed: &Seed) -> Option<String> {
    let out = Command::new("bash").args(["-c", &seed.settled_by]).current_dir(clone).stdin(Stdio::null()).output();
    match out {
        Ok(o) if o.status.success() => settles(&String::from_utf8_lossy(&o.stdout), seed),
        Ok(o) => Some(format!("`{}` exited {}", seed.settled_by, o.status)),
        Err(e) => Some(format!("`{}` did not run: {e}", seed.settled_by)),
    }
}

/// The settle targets of every plant in the `bp-research-*` sets under `evals`, with the sets that hold each, and
/// the targets of `dir` when it is not one of them.
pub fn targets(evals: &Path, dir: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(evals).map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
    dirs.retain(|d| d.is_dir() && d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("bp-research-")));
    let same = |d: &PathBuf| d.canonicalize().ok() == dir.canonicalize().ok();
    if !dirs.iter().any(same) {
        dirs.push(dir.to_path_buf());
    }
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for d in dirs {
        let md = std::fs::read_dir(&d)?.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "md")));
        for f in if md { Fixture::load(&d, None)? } else { Vec::new() } {
            for p in f.research.iter().filter(|r| !r.control).flat_map(|r| &r.plants) {
                let at = format!("{}/{}", d.file_name().unwrap_or_default().to_string_lossy(), f.id);
                out.entry(p.target().unwrap_or_default()).or_default().push(at);
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    Ok(out)
}

pub fn run(dir: &Path) -> Result<Vec<String>> {
    let started = Instant::now();
    let fixtures = Fixture::load(dir, None)?;
    if let Some(f) = fixtures.iter().find(|f| f.research.is_none()) {
        bail!("{} is no research fixture", f.id);
    }
    let release = std::env::current_dir()?.join("target/release/bp");
    if !release.exists() {
        bail!("{} is missing; run cargo build --release -p bp-tool first", release.display());
    }
    let root = Path::new(".").canonicalize()?;
    let mut clones = BTreeMap::new();
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-plants"), 4)?;
    for base in fixtures.iter().map(|f| f.base.clone()).collect::<BTreeSet<_>>() {
        let code = work.join(&base);
        capture(Command::new("git").args(["clone", "-q", "--no-hardlinks"]).arg(&root).arg(&code))?;
        capture(Command::new("git").args(["checkout", "-q", "--detach", &base]).current_dir(&code))?;
        clones.insert(base, code);
    }
    let held = targets(&root.join("xtask/evals"), dir)?;
    let mut why: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for f in &fixtures {
        let (r, clone) = (f.research.as_ref().expect("checked above"), &clones[&f.base]);
        let mut add = |reason: String| why.entry(f.id.as_str()).or_default().push(reason);
        for p in &r.plants {
            for url in p.text.split_whitespace().filter(|w| w.starts_with("https://") || w.starts_with("http://")) {
                let url = url.trim_end_matches(['.', ',', ')']);
                let found = Command::new("git").args(["grep", "-qF", url]).current_dir(clone).status()?.success();
                if found {
                    add(format!("{}: the clone holds its URL {url}", p.node));
                }
            }
            if !p.text.contains("://") {
                add(format!("{}: the seed names no URL", p.node));
            }
            if r.control {
                continue;
            }
            if let Some(w) = settle(clone, p) {
                add(format!("{}: {w}", p.node));
            }
            if let Some(w) = crate::plants_text::breaks(&r.question, p) {
                add(format!("{}: {w}", p.node));
            }
            match p.target() {
                None => add(format!("{}: settled_by names no .rs or .md path", p.node)),
                Some(t) if held.get(&t).is_some_and(|at| at.len() > 1) => {
                    add(format!("{}: target {t} is also planted in {}", p.node, held[&t].join(", ")));
                }
                Some(_) => {}
            }
        }
    }
    let vault = crate::scratch::run()?;
    crate::eval_sandbox::seed(&vault, &release, &fixtures.iter().collect::<Vec<_>>())?;
    for f in &fixtures {
        for p in f.research.iter().filter(|r| !r.control).flat_map(|r| &r.plants) {
            for noun in &p.nouns {
                let rows = search(&release, &vault, noun)?;
                if !rows.iter().take(TOP).any(|id| *id == p.node) {
                    why.entry(f.id.as_str()).or_default().push(format!("{}: `bp search {noun}` misses it", p.node));
                }
            }
        }
    }
    let mut refused = Vec::new();
    for f in &fixtures {
        match why.get(f.id.as_str()) {
            None => println!("{} ok", f.id),
            Some(w) => {
                println!("{} refused: {}", f.id, w.join("; "));
                refused.push(format!("{}: {}", f.id, w.join("; ")));
            }
        }
    }
    eprintln!("plants: {} fixtures, {} refused, {} s", fixtures.len(), refused.len(), started.elapsed().as_secs());
    std::fs::remove_dir_all(&work)?;
    Ok(refused)
}

/// The ids `bp search <noun> --json` returns, best first.
fn search(bp: &Path, vault: &Path, noun: &str) -> Result<Vec<String>> {
    let out = capture(
        Command::new(bp)
            .args(["search", noun, "--json"])
            .env("BP_VAULT", vault)
            .env_remove("BP_ROLE")
            .env_remove("BP_SESSION")
            .current_dir(vault)
            .stdin(Stdio::null()),
    )?;
    let v: Value = serde_json::from_str(&out).with_context(|| format!("bp search {noun} printed no JSON"))?;
    let rows = v["rows"].as_array().or_else(|| v.as_array()).cloned().unwrap_or_default();
    Ok(rows.iter().filter_map(|r| r["id"].as_str().map(str::to_string)).collect())
}
