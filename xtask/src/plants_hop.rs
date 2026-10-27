//! The two-hop candidates of `cargo xtask plants` (`plants_pool.rs`): file A's doc names file B in backticks, and B's
//! code holds one integer constant, so the figure has one source and the reader must take both hops.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;

use crate::lines::non_test;
use crate::plants_count::place;
use crate::plants_pool::{Candidate, constants, siblings, split_sentences};

/// The two-hop candidates of the Rust file `a`: each file B that a doc sentence of `a` names in backticks, when B's
/// code holds one integer constant. A name the clone does not hold gives a bare candidate (`not-in-clone`). The
/// figure is B's; the seed cites `a`; the question names `a`'s place and the sentence's words beside the name.
pub fn hops(clone: &Path, a: &str, text: &str, all: &BTreeSet<String>) -> Result<Vec<Candidate>> {
    let (mut out, mut para) = (Vec::new(), String::new());
    let me = Path::new(a).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let mut paras = Vec::new();
    for line in text.lines().take(non_test(text)) {
        let t = line.trim_start();
        match t.strip_prefix("//!").or_else(|| t.strip_prefix("///")).map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => write!(para, "{d} ").expect("a String takes a write"),
            None => paras.extend((!para.is_empty()).then(|| std::mem::take(&mut para))),
        }
    }
    for p in &paras {
        for (_, sentence) in split_sentences(p) {
            for name in siblings(sentence).into_iter().filter(|n| n != me) {
                let dir = Path::new(a).parent().map(|d| d.join(&name).display().to_string()).unwrap_or_default();
                let named: Vec<&String> = all.iter().filter(|f| f.ends_with(&format!("/{name}"))).collect();
                let alone = (named.len() == 1).then(|| named[0].clone());
                let b =
                    [Some(dir), Some(name.clone()), alone].into_iter().flatten().find(|b| all.contains(b) && b != a);
                let Some(b) = b else {
                    out.push(Candidate::bare(a, &name));
                    continue;
                };
                let inner = std::fs::read_to_string(clone.join(&b))?;
                let mut found = constants(&b, &inner);
                if found.len() != 1 {
                    continue;
                }
                let k = found.remove(0);
                let off: BTreeSet<String> = b
                    .split(|c: char| !c.is_alphanumeric())
                    .chain(k.line.split(|c: char| !c.is_alphanumeric()))
                    .map(str::to_lowercase)
                    .collect();
                let mut seen = BTreeSet::new();
                let near = sentence
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.len() >= 4 && w.chars().all(char::is_alphabetic))
                    .filter(|w| !off.contains(&w.to_lowercase()) && seen.insert(w.to_lowercase()))
                    .map(str::to_string)
                    .collect();
                let (at, stem, kind) = place(a);
                out.push(Candidate {
                    id: format!("two-hop:{a}>{}", k.id),
                    class: "two-hop",
                    cited: Some(a.to_string()),
                    subject: format!("the {stem} {kind} in {at}"),
                    form: "hop",
                    near,
                    ..k
                });
            }
        }
    }
    Ok(out)
}
