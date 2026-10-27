//! `cargo xtask plants --verify --fixtures <dir>` (31, design change 2): no block needs a model to check a plant's
//! truth. It builds `bp`, clones every distinct `base:` once and runs each plant's and control's `settled_by:` at the
//! clone's root. It refuses a seed whose output holds no `match:` word or holds an `unless:` word, matched by
//! [`holds`], and a seed whose URL the clone holds. It refuses a plant whose settle line ([`key`]) another plant of
//! any `xtask/evals/bp-research-*` set holds, a question that leaks (`plants_leak`), and a question or a title that
//! breaks the rules the planter writes them by (`plants_text::breaks`). Then it seeds every fixture into one scratch
//! copy of `$BP_VAULT` (`eval_sandbox::seed`) and refuses a seed that `bp search <noun> --json` does not return in its
//! first five rows, for each noun alone: `bp search` needs every term and has no stemming (`m8-pilot/result.md`,
//! finding 3). It prints one line per fixture.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::eval_fixture::Fixture;
use crate::eval_plant::Seed;
use crate::eval_sandbox::capture;
use crate::plants_leak::{Corpus, hop_leaks, leaks};

/// Rows of `bp search` a plant must sit in, for each noun.
pub const TOP: usize = 5;

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

/// The output of a settle command run at the root of `clone`, or why it did not run.
pub fn output(clone: &Path, cmd: &str) -> Result<String, String> {
    match Command::new("bash").args(["-c", cmd]).current_dir(clone).stdin(Stdio::null()).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(format!("`{cmd}` exited {}", o.status)),
        Err(e) => Err(format!("`{cmd}` did not run: {e}")),
    }
}

/// Runs a plant's settle command at the root of `clone` and says why it does not settle the plant, if it does not.
pub fn settle(clone: &Path, seed: &Seed) -> Option<String> {
    output(clone, &seed.settled_by).map_or_else(Some, |o| settles(&o, seed))
}

/// The settle line of a seed: its target, and `:<n>` when the settle output's first line is line `n` of the target,
/// so two plants on one file are two targets and two plants on one line are one.
pub fn key(clone: &Path, seed: &Seed) -> Option<String> {
    let t = seed.target()?;
    let out = output(clone, &seed.settled_by).unwrap_or_default();
    let text = std::fs::read_to_string(clone.join(&t)).unwrap_or_default();
    let line = out.lines().next().and_then(|first| text.lines().position(|l| l == first));
    Some(line.map_or_else(|| t.clone(), |n| format!("{t}:{}", n + 1)))
}

/// The repository cloned at each base, on first use, under one work directory.
pub struct Clones {
    root: PathBuf,
    work: PathBuf,
    at: BTreeMap<String, PathBuf>,
}

impl Clones {
    pub fn new(root: &Path, work: &Path) -> Self {
        Self { root: root.to_path_buf(), work: work.to_path_buf(), at: BTreeMap::new() }
    }

    /// Takes `code` as the clone at `base`, made elsewhere.
    #[cfg(test)]
    pub fn insert(&mut self, base: &str, code: &Path) {
        self.at.insert(base.to_string(), code.to_path_buf());
    }

    pub fn at(&mut self, base: &str) -> Result<PathBuf> {
        if let Some(c) = self.at.get(base) {
            return Ok(c.clone());
        }
        let code = self.work.join(base);
        capture(Command::new("git").args(["clone", "-q", "--no-hardlinks"]).arg(&self.root).arg(&code))?;
        capture(Command::new("git").args(["checkout", "-q", "--detach", base]).current_dir(&code))?;
        self.at.insert(base.to_string(), code.clone());
        Ok(code)
    }
}

/// The settle lines ([`key`]) of every plant in the `bp-research-*` sets under `evals`, each read at its own base,
/// with the fixtures that hold each. `dir` is one more set when `with` holds, and is left out when it does not: the
/// planter's `--out` set is the one it makes again.
pub fn targets(evals: &Path, dir: &Path, with: bool, clones: &mut Clones) -> Result<BTreeMap<String, Vec<String>>> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(evals).map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
    dirs.retain(|d| d.is_dir() && d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("bp-research-")));
    let same = |d: &PathBuf| d.canonicalize().ok() == dir.canonicalize().ok();
    dirs.retain(|d| !same(d));
    if with {
        dirs.push(dir.to_path_buf());
    }
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for d in dirs {
        let md = std::fs::read_dir(&d)?.any(|e| e.is_ok_and(|e| e.path().extension().is_some_and(|x| x == "md")));
        for f in if md { Fixture::load(&d, None)? } else { Vec::new() } {
            let clone = clones.at(&f.base)?;
            for p in f.research.iter().filter(|r| !r.control).flat_map(|r| &r.plants) {
                let at = format!("{}/{}", d.file_name().unwrap_or_default().to_string_lossy(), f.id);
                out.entry(key(&clone, p).unwrap_or_default()).or_default().push(at);
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    Ok(out)
}

/// Builds `bp` as a block would, and returns the binary: a check never runs a stale one.
pub fn build_bp(root: &Path) -> Result<PathBuf> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "-q", "--release", "-p", "bp-tool"])
        .current_dir(root)
        .stdin(Stdio::null())
        .status()
        .context("run cargo build")?;
    if !status.success() {
        bail!("cargo build -p bp-tool failed");
    }
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
    Ok(target.join("release/bp"))
}

pub fn run(dir: &Path) -> Result<Vec<String>> {
    let started = Instant::now();
    let fixtures = Fixture::load(dir, None)?;
    if let Some(f) = fixtures.iter().find(|f| f.research.is_none()) {
        bail!("{} is no research fixture", f.id);
    }
    let root = Path::new(".").canonicalize()?;
    let release = build_bp(&root)?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-plants"), 4)?;
    let mut clones = Clones::new(&root, &work);
    let held = targets(&root.join("xtask/evals"), dir, true, &mut clones)?;
    let mut corpora = BTreeMap::new();
    let mut why: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for f in &fixtures {
        let (r, clone) = (f.research.as_ref().expect("checked above"), clones.at(&f.base)?);
        if !corpora.contains_key(&f.base) {
            corpora.insert(f.base.clone(), Corpus::of(&clone)?);
        }
        let mut add = |reason: String| why.entry(f.id.as_str()).or_default().push(reason);
        let matches: Vec<String> = r.plants.iter().flat_map(|p| p.matches.clone()).collect();
        if let Some(w) = leaks(&r.question, &matches, &corpora[&f.base]) {
            add(w);
        }
        for p in &r.plants {
            for url in p.text.split_whitespace().filter(|w| w.starts_with("https://") || w.starts_with("http://")) {
                let url = url.trim_end_matches(['.', ',', ')']);
                let found = Command::new("git").args(["grep", "-qF", url]).current_dir(&clone).status()?.success();
                if found {
                    add(format!("{}: the clone holds its URL {url}", p.node));
                }
            }
            if !p.text.contains("://") {
                add(format!("{}: the seed names no URL", p.node));
            }
            if let Some(w) = settle(&clone, p) {
                add(format!("{}: {w}", p.node));
            }
            if let Some(w) = crate::plants_text::breaks(&r.question, p) {
                add(format!("{}: {w}", p.node));
            }
            if p.class == "two-hop" {
                let line = output(&clone, &p.settled_by).unwrap_or_default();
                if let Some(w) = hop_leaks(&line, &p.nouns, &r.question) {
                    add(format!("{}: {w}", p.node));
                }
            }
            if r.control {
                continue;
            }
            match key(&clone, p) {
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
        for p in f.research.iter().flat_map(|r| &r.plants) {
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
pub fn search(bp: &Path, vault: &Path, noun: &str) -> Result<Vec<String>> {
    Ok(ranked(bp, vault, noun)?.into_iter().map(|r| r.0).collect())
}

/// The ids and ranks `bp search <noun> --json` returns, best first.
pub fn ranked(bp: &Path, vault: &Path, noun: &str) -> Result<Vec<(String, f64)>> {
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
    Ok(rows.iter().filter_map(|r| Some((r["id"].as_str()?.to_string(), r["rank"].as_f64().unwrap_or(0.0)))).collect())
}
