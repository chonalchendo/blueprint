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
    let set = labels(vault, &format!("./{issue}.md"), None)?;
    // The label as the last commit left it: a line in the work tree that no commit holds is no evidence (review of
    // #100, V5), so `bp risk` and the intents agree with `land`, which refuses a vault that is not committed.
    let now = set.first().and_then(|(_, l)| *l);
    let level = |l: Option<Level>| l.unwrap_or(Level::Medium);
    // A hand `git mv` that rewrites the file past git's rename threshold starts a new history, so the labels of each
    // issue file that the move's commit deleted only raise this issue's level: with or without a milestone, and never
    // as the label in force at the lock (review of #100, L1; review of #102, S5 and F7). Read only below high.
    let floor = if now == Some(Level::High) { None } else { moved(vault, issue)? };
    let own = own(vault, index, issue, &header, &set, now)?;
    Ok(match floor {
        Some(f) if f > level(own) => Some(f),
        _ => own,
    })
}

/// The level the issue's own history declares: the label as it stands, or past the lock the highest label any
/// commit set from the lock on, or the one it held at the lock.
fn own(
    vault: &Vault,
    index: &Index,
    issue: &str,
    header: &serde_json::Map<String, serde_json::Value>,
    set: &[(String, Option<Level>)],
    now: Option<Level>,
) -> Result<Option<Level>, Refusal> {
    let level = |l: Option<Level>| l.unwrap_or(Level::Medium);
    // Only a label below one that git once held, or a low one, needs the lock: else the label reads as it stands.
    let fell = set.iter().any(|(_, l)| level(*l) > level(now));
    if !fell && (level(now) >= Level::Medium || set.is_empty()) {
        return Ok(now);
    }
    let id = Id::parse(issue).expect("the index holds ids");
    let Some(lock) = locked(vault, index, &id, text(header, "under").as_deref())? else { return Ok(now) };
    let mut best = level(now);
    // Newest first: each label set after the lock, then the one in force at the lock. With none set by the lock, the
    // issue held no label there, or did not exist yet: either reads medium.
    let mut at_lock = Level::Medium;
    for (commit, label) in set {
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

/// The label changes of the file at `path`, newest first, through git's renames, in the history of `until` (`HEAD`
/// when `None`). Each patch holds the whole file, so the header of each version reads apart from its body (review of
/// #100, W2).
fn labels(vault: &Vault, path: &str, until: Option<&str>) -> Result<Vec<(String, Option<Level>)>, Refusal> {
    // `--follow` follows a copy too, and `-G` skips a copy that changed no `risk` line: the newest copy is found
    // first, its commit reads as the file's first, and the copied file's history is not read (review of #102, S6).
    let mut listing = vec!["log", "--follow", "--name-status", "--format=commit: %H"];
    listing.extend(until);
    listing.extend(["--", path]);
    let (mut commit, mut copy) = (None, None);
    for line in git(&vault.root, &listing)?.lines() {
        if let Some(c) = line.strip_prefix("commit: ") {
            commit = Some(c.to_string());
        } else if let Some(rest) = line.strip_prefix('C').filter(|r| r.starts_with(|c: char| c.is_ascii_digit())) {
            copy = commit.clone().zip(rest.split('\t').nth(2).map(str::to_string));
            break;
        }
    }
    let range = copy.as_ref().map(|(c, _)| format!("{c}..{}", until.unwrap_or("HEAD")));
    let mut args = vec!["log", "--follow", "--text", "-p", "-U1000000", "--format=commit: %H", "-G", "risk"];
    args.extend(range.as_deref().or(until));
    args.extend(["--", path]);
    let mut out = changes(&git(&vault.root, &args)?, label);
    if let Some((c, at)) = copy {
        let text = git(&vault.root, &["show", &format!("{c}:{at}")])?;
        out.extend(label(&text).map(|l| (c, Some(l))));
    }
    Ok(out)
}

/// The highest label of each issue file of the project that the commit adding `issue`'s path deleted: a move git did
/// not see as one. Each such file's own move is followed too, up to ten moves; past that it reads high, so no chain of
/// moves lowers a label.
fn moved(vault: &Vault, issue: &str) -> Result<Option<Level>, Refusal> {
    const MOVES: usize = 10;
    let Some(project) = issue.split('/').nth(1) else { return Ok(None) };
    let issues = format!("./projects/{project}/issues");
    let (mut best, mut path) = (None, format!("./{issue}.md"));
    for _ in 0..MOVES {
        let added = git(&vault.root, &["log", "--format=%H", "--diff-filter=A", "--", &path])?;
        let Some(commit) = added.lines().last() else { return Ok(best) };
        let args = ["show", "--format=", "--name-only", "--no-renames", "--diff-filter=D", "--relative", commit];
        let gone = git(&vault.root, &[&args[..], &["--", &issues]].concat())?;
        let gone: Vec<String> =
            gone.lines().filter(|p| p.matches('/').count() == 3).map(|p| format!("./{p}")).collect();
        let parent = format!("{commit}^");
        for g in &gone {
            let high = labels(vault, g, Some(&parent))?.into_iter().map(|(_, l)| l.unwrap_or(Level::Medium)).max();
            best = best.max(high);
        }
        match gone.as_slice() {
            [one] => path.clone_from(one),
            _ => return Ok(best),
        }
    }
    Ok(Some(Level::High))
}

/// Each commit of the log `-p` that changed what `read` reads from the file, newest first, and the value it left.
/// Each patch holds the whole file, so the version before and after each commit reads in full. Each commit heads its
/// patch as `commit: <sha>`, which no diff line starts with.
///
/// `git log --follow` follows a copy as well as a rename, and no option turns that off. A copy made the file new: its
/// commit reads as the file's first, and the copied file's older commits are not read (review of #102, S6).
fn changes<T: PartialEq>(log: &str, read: impl Fn(&str) -> T) -> Vec<(String, T)> {
    let mut out: Vec<(String, T)> = Vec::new();
    let (mut commit, mut hunk, mut copy) = (None::<&str>, false, false);
    let (mut old, mut new) = (String::new(), String::new());
    let mut close = |commit: Option<&str>, old: &str, new: &str| {
        let value = read(new);
        if let Some(c) = commit.filter(|_| value != read(old)) {
            out.push((c.to_string(), value));
        }
    };
    for line in log.lines() {
        if let Some(c) = line.strip_prefix("commit: ") {
            close(commit, if copy { "" } else { &old }, &new);
            if copy {
                return out;
            }
            (commit, hunk) = (Some(c), false);
            old.clear();
            new.clear();
        } else if line.starts_with("@@") {
            hunk = true;
        } else if !hunk && line.starts_with("copy from ") {
            copy = true;
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
    close(commit, if copy { "" } else { &old }, &new);
    out
}

/// The header of `text` as the vault's parser reads it, so a quoted or spaced key reads as `bp show` shows it (review
/// of #100, X1); `None` with no closed header or YAML the parser refuses.
fn header(text: &str) -> Option<bp_vault::Header> {
    let text = bp_vault::normalise(text);
    let head = &text[..bp_vault::header_end(&text)?];
    let yaml = head.strip_prefix("---")?.trim_end().strip_suffix("---")?;
    bp_vault::Header::parse(yaml).ok()
}

/// The level of the `risk` key of `text`'s header; `None` with no header, no key or a value that names no level.
fn label(text: &str) -> Option<Level> {
    header(text)?.fields.get("risk")?.as_str()?.parse().ok()
}

/// The `status` of `text`'s header.
fn status(text: &str) -> Option<String> {
    header(text)?.status
}

/// The first commit that held the milestone `m` at `approved` or `done`, through renames. The parser reads each
/// version's status, so a quoted or spaced key counts (review of #100, L2). `--text`: a `-diff` attribute makes git skip
/// a binary file under `-G` and exit 0, which would read as "never approved" (review of #98, row 35).
pub fn approval(root: &std::path::Path, m: &str) -> Result<Option<String>, Refusal> {
    let path = format!("./{m}.md");
    let log = git(
        root,
        &["log", "--follow", "--text", "-p", "-U1000000", "--format=commit: %H", "-G", "status", "--", &path],
    )?;
    Ok(changes(&log, status)
        .into_iter()
        .filter(|(_, s)| matches!(s.as_deref(), Some("approved" | "done")))
        .map(|(c, _)| c)
        .next_back())
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
    let approved = approval(root, m)?;
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

    fn read(log: &str) -> Vec<(String, Option<Level>)> {
        changes(log, label)
    }

    fn set<const N: usize>(rows: [(&str, Option<Level>); N]) -> Vec<(String, Option<Level>)> {
        rows.into_iter().map(|(c, l)| (c.to_owned(), l)).collect()
    }

    #[test]
    fn a_commit_sets_the_level_of_its_header_or_none() {
        let log = "commit: c3\ndiff --git a/i.md b/i.md\n--- a/i.md\n+++ b/i.md\n@@ -1,4 +1,4 @@\n ---\n kind: issue\n title: t\n\
                   -risk: high\n+risk: \"low\" # by hand\n ---\ncommit: c2\n@@ -1,4 +1,3 @@\n ---\n kind: issue\n title: t\n-risk: medium\n\
                   \x20---\ncommit: c1\n@@ -0,0 +1,5 @@\n+---\n+kind: issue\n+title: t\n+risk: medium\n+---\n+risk: the parser is slow\n\
                   commit: c0\n@@ -1,3 +1,4 @@\n ---\n kind: issue\n title: t\n ---\n+body text\n";
        assert_eq!(read(log), set([("c3", Some(Level::Low)), ("c2", None), ("c1", Some(Level::Medium))]));
    }

    /// A `risk:` line of the body, as a YAML example holds, is not the label (review of #100, W2).
    #[test]
    fn a_body_line_is_never_the_label() {
        let log = "commit: c2\n@@ -1,6 +1,9 @@\n ---\n kind: issue\n title: t\n risk: high\n ---\n \n body\n+```yaml\n+risk: low\n\
                   +```\ncommit: c1\n@@ -0,0 +1,6 @@\n+---\n+kind: issue\n+title: t\n+risk: high\n+---\n+\n+body\n\
                   commit: c0\n@@ -0,0 +1,3 @@\n+body\n+\n+risk: low\n";
        assert_eq!(read(log), set([("c1", Some(Level::High))]));
    }

    /// A copy `git log --follow` followed is the file's first commit: the copied file's label and history are not
    /// this file's (review of #102, S6).
    #[test]
    fn a_copy_reads_as_the_file_s_first_commit() {
        let log = "commit: c2\ndiff --git a/a.md b/c.md\nsimilarity index 80%\ncopy from a.md\ncopy to c.md\n--- a/a.md\n\
                   +++ b/c.md\n@@ -1,4 +1,4 @@\n ---\n kind: issue\n-title: a\n+title: c\n risk: low\n ---\ncommit: c1\n\
                   @@ -0,0 +1,4 @@\n+---\n+kind: issue\n+title: a\n+risk: high\n+---\n";
        assert_eq!(read(log), set([("c2", Some(Level::Low))]));
    }

    /// A quoted or spaced key is the key the parser reads, as `bp show` shows it (review of #100, X1).
    #[test]
    fn a_quoted_or_spaced_key_is_the_label() {
        for key in ["\"risk\"", "'risk'", "risk "] {
            let log = format!("commit: c1\n@@ -0,0 +1,4 @@\n+---\n+kind: issue\n+title: t\n+{key}: high\n+---\n");
            let set = changes(&log, label);
            assert_eq!(set, [("c1".to_string(), Some(Level::High))], "{key}");
        }
    }

    /// A quoted or spaced `status` key is the milestone's status (review of #100, L2).
    #[test]
    fn a_quoted_status_is_the_status() {
        for key in ["\"status\"", "status "] {
            let log = format!(
                "commit: c2\n@@ -1,4 +1,4 @@\n ---\n kind: milestone\n title: t\n-status: planned\n+{key}: approved\n ---\n\
                 commit: c1\n@@ -0,0 +1,4 @@\n+---\n+kind: milestone\n+title: t\n+status: planned\n+---\n"
            );
            let set = changes(&log, status);
            let want =
                [("c2".to_string(), Some("approved".to_string())), ("c1".to_string(), Some("planned".to_string()))];
            assert_eq!(set, want, "{key}");
        }
    }
}
