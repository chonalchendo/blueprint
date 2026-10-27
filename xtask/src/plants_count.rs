//! The measurement candidates of `cargo xtask plants` (`plants_pool.rs`): a count the clone gives only by a command,
//! stated with no path, so a researcher must run the command to settle it. A file's subject says where the file is
//! ([`place`]), so no two files share one. The title puts the same facts in another order, so the question shares no
//! run of four words with it (`eval_plant::RUN`).

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
            let (at, stem, kind) = place(f);
            let label = format!("In {at}, the {stem} {kind} has {{}} lines");
            out.push(count(clone, id, f, &cmd, (&format!("the {stem} {kind} in {at}"), &label), "lines"));
        }
    }
    for c in &crates {
        let name = c.rsplit('/').next().unwrap_or(c);
        let cmd = format!("git ls-files '{c}/src/*.rs' | wc -l");
        let what = format!("the {name} crate");
        let label = format!("The {name} crate has {{}} Rust source files");
        out.push(count(clone, format!("measurement:files:{c}"), c, &cmd, (&what, &label), "Rust source files"));
        let cmd = format!("git grep -c '#\\[test\\]' -- '{c}/*.rs' | awk -F: '{{n += $2}} END {{print n}}'");
        let label = format!("The {name} crate has {{}} test functions");
        out.push(count(clone, format!("measurement:tests:{c}"), c, &cmd, (&what, &label), "test functions"));
    }
    for d in &bundles {
        let cmd = format!("git ls-files '{d}/*.md' | wc -l");
        let name = d.rsplit('/').next().unwrap_or(d);
        let (what, label) = (
            format!("the bundle's {name} directory"),
            format!("The {name} directory of the bundle holds {{}} markdown files"),
        );
        out.push(count(clone, format!("measurement:md:{d}"), d, &cmd, (&what, &label), "markdown files"));
    }
    out
}

/// A count candidate: the command run at the base gives the figure, and the claim states it with no path. `what`
/// is the subject and the title, `{}` in the title's place for the figure.
fn count(clone: &Path, id: String, source: &str, cmd: &str, what: (&str, &str), things: &str) -> Candidate {
    let out = capture(Command::new("bash").args(["-c", cmd]).current_dir(clone)).unwrap_or_default();
    let n: Option<u64> = out.trim().parse().ok().filter(|n| *n > 0);
    let mut state = n.unwrap_or(0) ^ id.len() as u64;
    let (claim, flipped, token, flip) = match n {
        Some(n) => {
            let f = flip_number(n, splitmix(&mut state));
            let what = format!("{} has", upper_first(what.0));
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
        kind: "count",
        at: claim.find(&format!(" {token} ")).map_or(0, |i| i + 1),
        claim,
        flipped,
        token: token.clone(),
        swap: flip.clone(),
        form: "count",
        subject: what.0.to_string(),
        unit: things.to_string(),
        label: what.1.to_string(),
        line: String::new(),
        near: Vec::new(),
        matches: vec![token],
        unless: vec![flip],
        settle: if n.is_some() { cmd.to_string() } else { String::new() },
    }
}

/// Where a file sits, its stem and what it is: (`xtask's src`, `scratch`, `module`) for `xtask/src/scratch.rs`,
/// (`bp-tool's tests`, `check`, `file`) for `crates/bp-tool/tests/check.rs`. The stem keeps its `_`, so a stem of two
/// words is one word of a run.
pub fn place(path: &str) -> (String, String, &'static str) {
    let parts: Vec<&str> = path.split('/').collect();
    let (krate, rest) = if parts[0] == "crates" { (parts[1], &parts[2..]) } else { (parts[0], &parts[1..]) };
    let dir = rest[..rest.len().saturating_sub(1)].join("/");
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let kind = if rest.first() == Some(&"src") { "module" } else { "file" };
    (format!("{krate}'s {dir}"), stem.to_string(), kind)
}

/// A Rust file's subject in a constant's template: `the <stem> module of the <crate> crate`.
pub fn module(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let krate = if parts[0] == "crates" { parts[1] } else { parts[0] };
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    format!("the {stem} module of the {krate} crate")
}

pub fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
