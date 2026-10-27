//! The measurement candidates of `cargo xtask plants` (`plants_pool.rs`): a count the clone gives only by a command,
//! stated with no path, so a researcher must run the command to settle it.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::eval_sandbox::capture;
use crate::mutants_walk::splitmix;
use crate::plants_flip::flip_number;
use crate::plants_pool::{Candidate, rust};

/// The counts: each crate's Rust files and `#[test]` functions, each module's lines, each bundle directory's
/// markdown files. The settle command is the count; the planter runs it at the base for the figure.
pub fn measurements(clone: &Path, files: &[String]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut crates = BTreeSet::new();
    let mut bundles = BTreeSet::new();
    for f in files {
        let parts: Vec<&str> = f.split('/').collect();
        match parts.as_slice() {
            ["crates", c, ..] => {
                crates.insert(format!("crates/{c}"));
            }
            ["xtask", ..] => {
                crates.insert("xtask".to_string());
            }
            ["bundle", d, _, ..] => {
                bundles.insert(format!("bundle/{d}"));
            }
            _ => {}
        }
        let stem = Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        if rust(f) && !["lib", "main", "mod"].contains(&stem) {
            let (id, cmd) = (format!("measurement:lines:{f}"), format!("wc -l < {f}"));
            out.push(count(clone, id, f, "lines", &cmd, (&module(f), "has"), "lines"));
        }
    }
    for c in &crates {
        let name = c.rsplit('/').next().unwrap_or(c);
        let cmd = format!("git ls-files '{c}/src/*.rs' | wc -l");
        let what = format!("the {name} crate");
        let what = (what.as_str(), "has");
        out.push(count(clone, format!("measurement:files:{c}"), c, "files", &cmd, what, "Rust source files"));
        let cmd = format!("git grep -c '#\\[test\\]' -- '{c}/*.rs' | awk -F: '{{n += $2}} END {{print n}}'");
        out.push(count(clone, format!("measurement:tests:{c}"), c, "tests", &cmd, what, "test functions"));
    }
    for d in &bundles {
        let cmd = format!("git ls-files '{d}/*.md' | wc -l");
        let what = format!("the bundle's {} directory", d.rsplit('/').next().unwrap_or(d));
        let what = (what.as_str(), "holds");
        out.push(count(clone, format!("measurement:md:{d}"), d, "bundle-files", &cmd, what, "markdown files"));
    }
    out
}

/// A count candidate: the command run at the base gives the figure, and the claim states it with no path.
fn count(
    clone: &Path,
    id: String,
    source: &str,
    kind: &'static str,
    cmd: &str,
    what: (&str, &str),
    things: &str,
) -> Candidate {
    let out = capture(Command::new("bash").args(["-c", cmd]).current_dir(clone)).unwrap_or_default();
    let n: Option<u64> = out.trim().parse().ok().filter(|n| *n > 0);
    let mut state = n.unwrap_or(0) ^ id.len() as u64;
    let (claim, flipped, token, flip) = match n {
        Some(n) => {
            let f = flip_number(n, splitmix(&mut state));
            let what = format!("{} {}", upper_first(what.0), what.1);
            (format!("{what} {n} {things}."), format!("{what} {f} {things}."), n.to_string(), f.to_string())
        }
        None => (String::new(), String::new(), String::new(), String::new()),
    };
    Candidate {
        id,
        class: "measurement",
        genre: "Number",
        source: source.into(),
        cited: None,
        kind,
        at: claim.find(&format!(" {token} ")).map_or(0, |i| i + 1),
        claim,
        flipped,
        token: token.clone(),
        swap: flip.clone(),
        form: "count",
        subject: what.0.to_string(),
        unit: things.to_string(),
        matches: vec![token],
        unless: vec![flip],
        settle: if n.is_some() { cmd.to_string() } else { String::new() },
    }
}

/// A Rust file's subject in words: `the <stem> module of the <crate> crate`.
pub fn module(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let krate = if parts[0] == "crates" { parts[1] } else { parts[0] };
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    format!("the {} module of the {krate} crate", stem.replace('_', " "))
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
