//! The row checks of `record` (15, record; 29 design change 6). A review holds the head it read and one findings
//! table, and each row names a severity, a place in the diff of that head and what is wrong; a declined disposition
//! says why. A review the responder cannot count or place is refused when it is written, not when it is answered.

use std::collections::BTreeMap;
use std::path::Path;

use bp_change::DispositionAs;
use bp_read::inputs::{field, held};
use bp_vault::{Code, Id, Index, Kind, Refusal};

use crate::claim::git;

const RULE: &str = "a review holds a `head:` line and one findings table; each row names a severity, a location in \
                    the diff of its head or a node, and the finding (15, record)";

/// The lines of each file that the diff shows, by path.
type Hunks = BTreeMap<String, Vec<(u64, u64)>>;

/// Refuses a record whose rows fail their kind's check: a review's body, or a declined disposition with no reason.
pub(crate) fn check(
    index: &Index,
    kind: Kind,
    parent: Option<&Id>,
    given: &BTreeMap<&str, &str>,
    body: &str,
) -> Result<(), Refusal> {
    match kind {
        Kind::Review => review(index, parent.expect("a review sits under an issue"), given["head"], body),
        Kind::Disposition
            if given["as"] == DispositionAs::Declined.as_str()
                && given.get("reason").is_none_or(|r| r.trim().is_empty()) =>
        {
            let fix = "`reason` is required when as=declined: --field reason=\"<the issue, design or convention that \
                       rules the finding out>\"";
            Err(crate::record::schema("disposition reason".into(), fix.into()))
        }
        _ => Ok(()),
    }
}

fn review(index: &Index, issue: &Id, head: &str, body: &str) -> Result<(), Refusal> {
    let refuse =
        |what: &str, fix: String| Refusal::new(Code::FindingInvalid, format!("{issue} review {what}"), RULE, fix);
    // The reviewer's text may shorten the sha or quote it; it still names this head.
    let mut named = body.lines().filter_map(|l| l.trim().strip_prefix("head:")).map(|h| h.trim().trim_matches('`'));
    if !named.any(|h| h.len() >= 7 && head.starts_with(h)) {
        return Err(refuse("head", format!("write `head: {head}` as line 1 of the body")));
    }
    // The rows are what `bp_read::inputs::findings` counts: each line that starts with `|`, less the header and
    // separator.
    let table: Vec<&str> = body.lines().map(str::trim).filter(|l| l.starts_with('|')).collect();
    if table.len() < 2 || !table[1].chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
        let fix = "add the table `| severity | location | finding |` and its line `|---|---|---|`; a table with no \
                   row says no finding";
        return Err(refuse("table", fix.into()));
    }
    // A second table's header would read as a finding with a bad location, so its separator is refused first.
    if let Some(n) =
        table[2..].iter().position(|r| r.contains('-') && r.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')))
    {
        let fix = "the body holds a second table; keep one findings table";
        return Err(refuse(&format!("finding {}", n + 1), fix.into()));
    }
    let mut diff = None;
    for (n, row) in table[2..].iter().enumerate().map(|(i, r)| (i + 1, r)) {
        let inner = row[1..].strip_suffix('|').unwrap_or(&row[1..]);
        let cells: Vec<&str> = inner.split('|').map(str::trim).collect();
        if cells.len() < 3 || cells[..3].iter().any(|c| c.is_empty()) {
            let fix = format!(
                "fill row {n}: `| error or warn | <path>:<line> or a node id | what is wrong, and what would pass |`"
            );
            return Err(refuse(&format!("finding {n}"), fix));
        }
        let at = cells[1].trim_matches('`');
        if Id::parse(at).is_ok() && index.header(at)?.is_some() {
            continue;
        }
        if diff.is_none() {
            diff = Some(hunks(index, issue, head)?);
        }
        if !diff.as_ref().is_some_and(|h| inside(h, at)) {
            let fix = format!(
                "finding {n} cites `{at}`, which the diff of {head} against the claim's base does not show: cite a \
                 file and lines that `git diff <base> {head}` shows, or a node id, or drop the finding"
            );
            return Err(refuse(&format!("finding {n}"), fix));
        }
    }
    Ok(())
}

/// The lines that `git diff <base> <head>` shows in the claim's worktree, with git's three lines of context: the new
/// side of each file, the old side of a deleted one. Flags fix the format whatever the user's git config says.
fn hunks(index: &Index, issue: &Id, head: &str) -> Result<Hunks, Refusal> {
    let Some(claim) = held(index, issue)? else {
        let fix = format!(
            "{issue} holds no claim, so no diff places a row: cite a node id, or record the review on the issue after \
             its claim"
        );
        return Err(Refusal::new(Code::FindingInvalid, format!("{issue} review"), RULE, fix));
    };
    let (worktree, base) = (
        field(&claim, "worktree").expect("a claim holds its worktree and base"),
        field(&claim, "base").expect("a claim holds its worktree and base"),
    );
    let args = ["-c", "core.quotePath=false", "diff", "--no-ext-diff", "--no-color", "--no-prefix", "-U3", &base, head];
    let text = git(Path::new(&worktree), &args)?;
    let mut out = Hunks::new();
    // A pure rename, a binary file or a mode change shows no hunk, but the file is still citable by its bare path.
    let names = ["-c", "core.quotePath=false", "diff", "--no-ext-diff", "--name-only", &base, head];
    for name in git(Path::new(&worktree), &names)?.lines() {
        out.entry(name.to_string()).or_default();
    }
    let (mut old, mut new, mut header) = ("", "", false);
    for line in text.lines() {
        // A removed line `-- x` reads `--- x`, so the file names are read only between `diff --git` and the first hunk.
        // Git ends a name that holds a space with a tab.
        if line.starts_with("diff --git ") {
            header = true;
        } else if header && let Some(p) = line.strip_prefix("--- ") {
            old = p.strip_suffix('\t').unwrap_or(p);
        } else if header && let Some(p) = line.strip_prefix("+++ ") {
            new = p.strip_suffix('\t').unwrap_or(p);
        } else if let Some(h) = line.strip_prefix("@@ -") {
            header = false;
            let (o, n) = h.split_once(" +").expect("git writes a hunk as `@@ -a,b +c,d @@`");
            let (path, side) = if new == "/dev/null" { (old, o) } else { (new, n.split(' ').next().unwrap_or(n)) };
            let (start, count) = side.split_once(',').unwrap_or((side, "1"));
            let number = |t: &str| t.parse::<u64>().expect("git writes a hunk's lines as numbers");
            let (start, count) = (number(start), number(count));
            out.entry(path.to_string()).or_default().push((start, start + count.max(1) - 1));
        }
    }
    Ok(out)
}

/// Whether `at`, as `<path>` or `<path>:<lines>` with lines such as `20`, `18-21` or `15, 26`, names a file of the
/// diff, and each line or range touches a hunk of it.
fn inside(hunks: &Hunks, at: &str) -> bool {
    let lines = |spec: &str| -> Option<Vec<(u64, u64)>> {
        spec.split(',')
            .map(|s| {
                let (a, b) = s.split_once('-').unwrap_or((s, s));
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .collect()
    };
    let (path, want) = at.rsplit_once(':').and_then(|(p, s)| Some((p, lines(s)?))).unwrap_or((at, Vec::new()));
    hunks.get(path).is_some_and(|h| want.iter().all(|&(a, b)| h.iter().any(|&(s, e)| a <= e && b >= s)))
}
