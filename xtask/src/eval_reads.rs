//! What an agent read (31, design change 4): the list [`crate::eval_trace::Trace::reads`] builds from a transcript,
//! and the read test the research grader asks of it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// The files and lines a search's output holds, one entry per file: `<path>:N:text`, and `N:text` when the search ran
/// on the one file `single`. A path holds no space, so a compiler's `--> src/a.rs:3:5` reads nothing.
pub(crate) fn hits(out: &str, single: Option<&str>, rel: &dyn Fn(&str) -> String) -> BTreeMap<String, Vec<(u32, u32)>> {
    let mut hits = BTreeMap::<String, Vec<(u32, u32)>>::new();
    for line in out.lines() {
        let mut parts = line.splitn(3, ':');
        let (file, n) = match (parts.next(), parts.next(), parts.next(), single) {
            (Some(n), Some(_), _, Some(path)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => {
                (rel(path), n)
            }
            (Some(file), Some(n), Some(_), _) if !file.is_empty() && !file.contains(char::is_whitespace) => {
                (rel(file), n)
            }
            _ => continue,
        };
        if let Ok(n) = n.parse() {
            hits.entry(file).or_default().push((n, n));
        }
    }
    hits
}

/// What an agent read ([`crate::eval_trace::Trace::reads`]).
#[derive(Debug, Default, PartialEq)]
pub struct Reads {
    /// A file and the line ranges read in it: one entry per `Read` call, and per file of a `Grep` call's output.
    pub files: Vec<(String, Vec<(u32, u32)>)>,
    /// Each Bash command once, clone-relative.
    pub commands: Vec<String>,
    /// The ids `bp show` named.
    pub nodes: BTreeSet<String>,
    /// The URLs fetched; the eval agent has no web tool (31, Out of scope), so this stays empty there.
    pub urls: BTreeSet<String>,
}

impl Reads {
    /// Whether `path` (clone-relative, or its last parts, as `ledger.rs`) was read at one of `lines`, or at all when
    /// `lines` is empty: a `Read` range or a `Grep` line on it, or a Bash command whose text names it ([`names`]).
    pub fn read(&self, path: &str, lines: &[(u32, u32)]) -> bool {
        let same = |p: &str| p == path || p.ends_with(&format!("/{path}"));
        let at = |have: &[(u32, u32)]| {
            lines.is_empty() || lines.iter().any(|&(a, b)| have.iter().any(|&(c, d)| a <= d && c <= b))
        };
        self.files.iter().any(|(p, have)| same(p) && at(have)) || self.commands.iter().any(|c| names(c, path))
    }
}

/// Whether `command` holds `path` whole: no path character before it but a `/`, and none after it, so `walk.rs` is
/// in `src/walk.rs` and not in `walk.rsx`, and the directory `xtask` is not in `xtask/src`.
fn names(command: &str, path: &str) -> bool {
    let part = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    !path.is_empty()
        && command.match_indices(path).any(|(at, _)| {
            let after = &command[at + path.len()..];
            !command[..at].ends_with(part)
                && !after.strip_prefix('/').unwrap_or(after).starts_with(|c: char| part(c) || c == '/')
        })
}

/// `text` with the clone root `root` taken out: a leading `cd <root> && `, then `<root>/` before every path, so a
/// row that cites the absolute path is read as the relative one (`m8-pilot/result.md`, finding 7). A temporary
/// directory on macOS has two forms, with and without `/private`; both go.
pub(crate) fn relative(text: &str, root: &str) -> String {
    let mut out = text.to_string();
    if root.is_empty() {
        return out;
    }
    let short = root.strip_prefix("/private").unwrap_or(root);
    for r in [format!("/private{short}"), short.to_string()] {
        out =
            out.strip_prefix(&format!("cd {r} && ")).map_or(out.clone(), str::to_string).replace(&format!("{r}/"), "");
    }
    out
}

/// A tool result's text: the string, or its text blocks joined.
pub(crate) fn text_of(content: &Value) -> String {
    match content {
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        v => v.as_str().unwrap_or_default().to_string(),
    }
}
