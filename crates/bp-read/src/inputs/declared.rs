//! The declared level of an issue (37, design change 1), read from the vault's history: a label may rise at any time,
//! and it lowers only before its milestone's first approval and before the issue's first claim. After that only the
//! human's signed lowering lowers a level, so a lower label written by hand reads as the higher one it replaced.

use bp_change::risk::Level;
use bp_vault::{Id, Index, Refusal, Vault};

use super::text;
use crate::git::git;

/// The issue's declared level: the `risk` key of its committed header, never a line of its body, `None` with no label,
/// which reads medium. Once the label is locked (at [`locked`]), it reads the highest label any commit set from the
/// lock on, or the one it held at the lock: a later commit that lowers it, which `bp set` refuses, does not count. `land`, the intents and `bp risk` read it. A
/// vault outside git reads the label as it stands; a git failure refuses `could_not_run`.
///
/// # Panics
///
/// Never: the index holds ids.
pub fn declared(vault: &Vault, index: &Index, issue: &str) -> Result<Option<Level>, Refusal> {
    let header = index.header(issue)?.unwrap_or_default();
    let now = text(&header, "risk").and_then(|l| l.parse::<Level>().ok());
    if bp_vault::git::Repo::find(&vault.root).is_none() {
        return Ok(now);
    }
    let path = format!("./{issue}.md");
    // Each patch holds the whole file, so the header of each version reads apart from its body (review of #100, W2).
    let args =
        ["log", "--follow", "--text", "-p", "-U1000000", "--format=commit: %H", "-G", "^risk:", "--", path.as_str()];
    let set = labels(&git(&vault.root, &args)?);
    // The label as the last commit left it: a line in the work tree that no commit holds is no evidence (review of
    // #100, V5), so `bp risk` and the intents agree with `land`, which refuses a vault that is not committed.
    let now = set.first().and_then(|(_, l)| *l);
    let level = |l: Option<Level>| l.unwrap_or(Level::Medium);
    // Only a label below one that git once held, or a low one, needs the lock: else the label reads as it stands.
    let fell = set.iter().any(|(_, l)| level(*l) > level(now));
    if !fell && (level(now) >= Level::Medium || set.is_empty()) {
        return Ok(now);
    }
    let id = Id::parse(issue).expect("the index holds ids");
    let Some(lock) = locked(vault, index, &id, text(&header, "under").as_deref())? else { return Ok(now) };
    let mut best = level(now);
    // Newest first: each label set after the lock, then the one in force at the lock. With none set by the lock, the
    // issue held no label there, or did not exist yet: either reads medium.
    let mut at_lock = Level::Medium;
    for (commit, label) in &set {
        if *commit != lock && git(&vault.root, &["merge-base", "--is-ancestor", &lock, commit]).is_ok() {
            best = best.max(level(*label));
        } else {
            at_lock = level(*label);
            break;
        }
    }
    best = best.max(at_lock);
    Ok(if best > level(now) { Some(best) } else { now })
}

/// Each commit of the log `-p` that changed the `risk` label, newest first, and the label it left: the level of the
/// `risk:` line in the header of the version it wrote, or `None` when that header holds none. Each patch holds the
/// whole file, so the version before and after each commit reads in full, and a `risk:` line of the body is never the
/// label. Each commit heads its patch as `commit: <sha>`, which no diff line starts with.
fn labels(log: &str) -> Vec<(String, Option<Level>)> {
    let mut out: Vec<(String, Option<Level>)> = Vec::new();
    let (mut commit, mut hunk, mut old, mut new) = (None::<&str>, false, String::new(), String::new());
    let mut close = |commit: Option<&str>, old: &str, new: &str| {
        if let Some(c) = commit.filter(|_| label(new) != label(old)) {
            out.push((c.to_string(), label(new)));
        }
    };
    for line in log.lines() {
        if let Some(c) = line.strip_prefix("commit: ") {
            close(commit, &old, &new);
            (commit, hunk) = (Some(c), false);
            old.clear();
            new.clear();
        } else if line.starts_with("@@") {
            hunk = true;
        } else if hunk {
            // The lines before the first `@@` name the files; in the hunk each line opens with its side.
            let (side, text) = match line.as_bytes().first() {
                Some(b) if b.is_ascii() => (*b, &line[1..]),
                _ => (b' ', line),
            };
            for (to, holds) in [(&mut old, side != b'+'), (&mut new, side != b'-')] {
                if holds && side != b'\\' {
                    to.push_str(text);
                    to.push('\n');
                }
            }
        }
    }
    close(commit, &old, &new);
    out
}

/// The level of the `risk` key of `text`'s header, between its opening and closing `---` lines; `None` with no header,
/// no key or a value that names no level.
fn label(text: &str) -> Option<Level> {
    let header = &text[..bp_vault::header_end(text)?];
    let value = header.lines().find_map(|l| l.strip_prefix("risk:"))?;
    value.split('#').next().unwrap_or_default().trim().trim_matches(['"', '\'']).parse().ok()
}

/// The commit from which a label no longer lowers the issue's level, or `None` while it still may (`bp set`'s rule,
/// 37, design change 1): the earlier of the first commit that held the milestone `under` at `approved` or `done`,
/// and the first claim record of the issue at its path or an alias. With no milestone a label never lowers, so the
/// lock is the issue's first commit.
fn locked(vault: &Vault, index: &Index, issue: &Id, under: Option<&str>) -> Result<Option<String>, Refusal> {
    let root = &vault.root;
    let oldest = |args: &[&str]| git(root, args).map(|out| out.lines().last().map(str::to_string));
    let Some(m) = under else {
        return oldest(&["log", "--follow", "--format=%H", "--diff-filter=A", "--", &format!("./{issue}.md")]);
    };
    let path = format!("./{m}.md");
    let approved =
        oldest(&["log", "--follow", "--text", "--format=%H", "-G", "^status: *[\"']?(approved|done)", "--", &path])?;
    let dirs: Vec<String> =
        std::iter::once(issue.clone()).chain(index.aliases_of(issue)?).map(|d| format!("./{d}/claims")).collect();
    let mut args = vec!["log", "--format=%H", "--diff-filter=A", "--"];
    args.extend(dirs.iter().map(String::as_str));
    let claimed = oldest(&args)?;
    Ok(match (approved, claimed) {
        (Some(a), Some(c)) if git(root, &["merge-base", "--is-ancestor", &c, &a]).is_ok() => Some(c),
        (Some(a), _) => Some(a),
        (None, c) => c,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_sets_the_level_of_its_header_line_or_none() {
        let log = "commit: c3\ndiff --git a/i.md b/i.md\n--- a/i.md\n+++ b/i.md\n@@ -1,3 +1,3 @@\n ---\n-risk: high\n\
                   +risk: \"low\" # by hand\n ---\ncommit: c2\n@@ -1,3 +1,2 @@\n ---\n-risk: medium\n ---\n\
                   commit: c1\n@@ -0,0 +1,4 @@\n+---\n+risk: medium\n+---\n+risk: the parser is slow\n\
                   commit: c0\n@@ -1,2 +1,3 @@\n ---\n ---\n+body text\n";
        let want = [("c3", Some(Level::Low)), ("c2", None), ("c1", Some(Level::Medium))];
        let set = labels(log);
        let got: Vec<(&str, Option<Level>)> = set.iter().map(|(c, l)| (c.as_str(), *l)).collect();
        assert_eq!(got, want);
    }

    /// A `risk:` line of the body, as a YAML example holds, is not the label (review of #100, W2).
    #[test]
    fn a_body_line_is_never_the_label() {
        let log = "commit: c2\n@@ -1,6 +1,9 @@\n ---\n kind: issue\n risk: high\n ---\n \n body\n+```yaml\n+risk: low\n\
                   +```\ncommit: c1\n@@ -0,0 +1,6 @@\n+---\n+kind: issue\n+risk: high\n+---\n+\n+body\n\
                   commit: c0\n@@ -0,0 +1,3 @@\n+body\n+\n+risk: low\n";
        let set = labels(log);
        let got: Vec<(&str, Option<Level>)> = set.iter().map(|(c, l)| (c.as_str(), *l)).collect();
        assert_eq!(got, [("c1", Some(Level::High))]);
    }
}
