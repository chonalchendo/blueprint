//! The reviews of an issue as their first commit wrote them, reduced to what the rounds read (37, design changes 2,
//! 6 and 7), and the inputs of an issue's review.

use std::collections::{BTreeMap, BTreeSet};

use bp_change::intents;
use bp_change::risk::RISK_ROW;
use bp_change::rounds::{self, Answer, Lens, Review, Row};
use bp_vault::{Id, Index, Kind, Record, Refusal, Vault};
use serde_json::{Map, Value};

use super::{runs, text};

/// A review record as its first commit wrote it (37, design change 2).
pub struct Written {
    /// The record's id.
    pub id: String,
    /// Its header.
    pub header: Map<String, Value>,
    /// Its body.
    pub body: String,
}

/// The reviews of `issue`, in time order, each as its first commit wrote it (37, design changes 2 and 6), so a later
/// edit of a record cannot drop a `risk` row, an error row, its round or its `late` list. The first add counts at the
/// issue's path or any of its aliases, so a `bp rename` after a hand edit does not launder it. One `git log` finds the
/// adds and one `git cat-file --batch` reads them. A vault outside git reads as it stands, as does a record git never
/// held; any git failure refuses `could_not_run`, never a fallback to the present text.
///
/// # Panics
///
/// Never: the index holds ids.
pub fn written(vault: &Vault, index: &Index, issue: &str) -> Result<Vec<Written>, Refusal> {
    let records = index.records(issue, Kind::Review)?;
    let present = |r: Record| Written { id: r.id, header: r.header, body: r.body };
    if records.is_empty() || bp_vault::git::Repo::find(&vault.root).is_none() {
        return Ok(records.into_iter().map(present).collect());
    }
    let id = Id::parse(issue).expect("the index holds ids");
    let dirs: Vec<String> =
        std::iter::once(id.clone()).chain(index.aliases_of(&id)?).map(|d| format!("{d}/reviews")).collect();
    let args = ["-c", "core.quotepath=off", "log", "--relative", "--reverse", "--no-renames", "--diff-filter=A"];
    let log = crate::git::command(&vault.root)
        .args(args)
        .args(["--name-only", "--format=@%H", "--"])
        .args(dirs.iter().map(|d| format!("./{d}")))
        .output()
        .map_err(|e| Refusal::failed("git log", format!("{e}; install git")))?;
    if !log.status.success() {
        return Err(failed("git log", &log.stderr, &vault.root));
    }
    // The oldest add of each file name, at any of the paths: the record as it was first written.
    let (mut first, mut sha) = (BTreeMap::<String, String>::new(), "");
    let text = String::from_utf8_lossy(&log.stdout);
    for line in text.lines() {
        if let Some(commit) = line.strip_prefix('@') {
            sha = commit;
        } else if let Some((dir, name)) = line.strip_suffix(".md").and_then(|p| p.rsplit_once('/'))
            && dirs.iter().any(|d| d == dir)
        {
            first.entry(name.to_string()).or_insert_with(|| format!("{sha}:{line}"));
        }
    }
    let name = |r: &Record| r.id.rsplit('/').next().unwrap_or_default().to_string();
    let wanted: Vec<&String> = records.iter().filter_map(|r| first.get(&name(r))).collect();
    let mut blobs = blobs(&vault.root, &wanted)?.into_iter();
    let mut out = Vec::new();
    for r in records {
        let Some(object) = first.get(&name(&r)) else {
            out.push(present(r));
            continue;
        };
        // The record parses at the path it was added at: before a rename, its `under` names the old issue id.
        let at = object.split_once(':').and_then(|(_, p)| p.strip_suffix(".md")).and_then(|p| Id::parse(p).ok());
        let id = at.expect("git named a path under an id's directory");
        let file = bp_vault::File::from_bytes(id.clone(), &blobs.next().expect("one blob per object asked"))?;
        out.push(Written {
            header: file.header.map(|h| h.to_json(&id)).unwrap_or_default(),
            body: file.body.text,
            id: r.id,
        });
    }
    Ok(out)
}

/// The contents of each `<commit>:<path>` in `objects`, in order, from one `git cat-file --batch`.
fn blobs(root: &std::path::Path, objects: &[&String]) -> Result<Vec<Vec<u8>>, Refusal> {
    use std::io::Write;
    if objects.is_empty() {
        return Ok(Vec::new());
    }
    let mut child = crate::git::command(root)
        .args(["cat-file", "--batch"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Refusal::failed("git cat-file", format!("{e}; install git")))?;
    let input = objects.iter().map(|o| o.as_str()).collect::<Vec<_>>().join("\n") + "\n";
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // A writer thread, so a long answer cannot fill the pipe while the question is still being written.
    let out = std::thread::scope(|s| {
        s.spawn(move || stdin.write_all(input.as_bytes()));
        child.wait_with_output()
    })
    .map_err(|e| Refusal::failed("git cat-file", e.to_string()))?;
    if !out.status.success() {
        return Err(failed("git cat-file", &out.stderr, root));
    }
    let (mut rest, mut found) = (out.stdout.as_slice(), Vec::new());
    for object in objects {
        let end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let line = String::from_utf8_lossy(&rest[..end]).into_owned();
        let size = match line.rsplit_once(' ') {
            Some((kind, size)) if kind.ends_with(" blob") => size.parse::<usize>().ok(),
            _ => None,
        };
        let Some(size) = size.filter(|n| end + 1 + n <= rest.len()) else {
            return Err(Refusal::failed(
                format!("git cat-file {object}"),
                format!("git answered `{line}`; run git fsck in {}", root.display()),
            ));
        };
        found.push(rest[end + 1..end + 1 + size].to_vec());
        // Each object ends with a newline after its bytes.
        rest = rest.get(end + 2 + size..).unwrap_or_default();
    }
    Ok(found)
}

fn failed(what: &str, stderr: &[u8], root: &std::path::Path) -> Refusal {
    Refusal::failed(
        what,
        format!("{}; fix it in {}, then rerun", String::from_utf8_lossy(stderr).trim(), root.display()),
    )
}

/// The reviews of `issue`, reduced from what [`written`] reads, with their dispositions. A converted review with no
/// round reads round 1, `conformance`.
pub fn reviews(vault: &Vault, index: &Index, issue: &str) -> Result<Vec<Review>, Refusal> {
    reduced(index, issue, &written(vault, index, issue)?)
}

/// The [`written`] reviews of `issue`, reduced, with their dispositions.
pub fn reduced(index: &Index, issue: &str, written: &[Written]) -> Result<Vec<Review>, Refusal> {
    let mut out = Vec::new();
    for w in written {
        let late: BTreeSet<u64> =
            w.header.get("late").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
        let answers = answers(index, issue, &w.id)?;
        let of = of_cells(&w.body);
        let rows = (1..)
            .zip(findings(&w.body))
            .map(|(n, cells)| {
                let cell = |i: usize| cells.get(i).map_or("", |c| c.trim_matches('`'));
                Row {
                    error: rounds::is_error(cell(0)),
                    risk: cell(0) == RISK_ROW,
                    late: late.contains(&n),
                    answer: answers.get(&n).copied(),
                    of: of.get(&n).cloned(),
                    at: cell(1).to_string(),
                }
            })
            .collect();
        out.push(Review {
            head: text(&w.header, "head").unwrap_or_default(),
            round: w.header.get("round").and_then(Value::as_u64).unwrap_or(1),
            lens: text(&w.header, "lens").and_then(|l| l.parse().ok()).unwrap_or(Lens::Conformance),
            id: w.id.clone(),
            rows,
        });
    }
    Ok(out)
}

/// The `of` cell of each finding of a review body, by row: the column whose header cell is `of`, holding
/// `<review id>#<row>` (`bp-review`). A cell that does not parse names nothing; `record review` refuses it.
#[must_use]
pub fn of_cells(body: &str) -> BTreeMap<u64, (String, u64)> {
    let header = body.lines().map(str::trim).find(|l| l.starts_with('|')).map(cells).unwrap_or_default();
    let Some(at) = header.iter().position(|c| c == "of") else { return BTreeMap::new() };
    let cell = |row: &Vec<String>| {
        let (id, n) = row.get(at)?.trim_matches('`').rsplit_once('#')?;
        Some((id.to_string(), n.parse().ok()?))
    };
    (1..).zip(findings(body)).filter_map(|(n, row)| Some((n, cell(&row)?))).collect()
}

/// The cells of one table line, trimmed. A `\|` is a literal pipe inside a cell (`bp-review`), so a reproduction can
/// quote a pipeline in any table shape. The closing bar leaves no empty last cell; a line with none keeps its last.
#[must_use]
pub fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let (mut out, mut cell, mut chars) = (Vec::new(), String::new(), line.strip_prefix('|').unwrap_or(line).chars());
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.as_str().starts_with('|') => {
                cell.push('|');
                chars.next();
            }
            '|' => out.push(std::mem::take(&mut cell)),
            c => cell.push(c),
        }
    }
    if !cell.is_empty() {
        out.push(cell);
    }
    out.into_iter().map(|c| c.trim().to_string()).collect()
}

/// The answer of each disposition of `review`, by finding.
fn answers(index: &Index, issue: &str, review: &str) -> Result<BTreeMap<u64, Answer>, Refusal> {
    let mut out = BTreeMap::new();
    for d in index.records(review, Kind::Disposition)? {
        let Some(n) = d.header.get("finding").and_then(Value::as_u64) else { continue };
        let answer = match text(&d.header, "as").as_deref() {
            Some("fixed") => Answer::Fixed,
            _ if follow_up(index, issue, &text(&d.header, "reason").unwrap_or_default())?.is_some() => Answer::FollowUp,
            _ => Answer::Declined,
        };
        out.insert(n, answer);
    }
    Ok(out)
}

/// The follow-up issue a decline's reason names (37, design change 7): its first word that is the id of an issue other
/// than `issue`, under `issue`'s milestone, and live (`open` or `in-progress`), so the milestone's stamp waits on it.
/// `None` when no word is; a follow-up later resolved or abandoned answers nothing, and the row holds the land again.
pub fn follow_up(index: &Index, issue: &str, reason: &str) -> Result<Option<String>, Refusal> {
    let Some(milestone) = index.header(issue)?.and_then(|h| text(&h, "under")) else { return Ok(None) };
    let cut = |c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '[' | ']' | '`' | '"' | '\'');
    for word in reason.split(cut).map(|w| w.trim_end_matches(['.', ':'])) {
        if word.is_empty() || word == issue || Id::parse(word).is_err() {
            continue;
        }
        let Some(h) = index.header(word)? else { continue };
        let live = text(&h, "status").is_some_and(|s| intents::is_live_issue(&s));
        if text(&h, "kind").as_deref() == Some("issue") && text(&h, "under").as_deref() == Some(&milestone) && live {
            return Ok(Some(word.to_string()));
        }
    }
    Ok(None)
}

/// What an issue's review reads (37, design changes 1, 6 and 8): its runs, its reviews as first committed, its `risk`
/// label and whether it caused a pending delta. The intents, `record review` and the review stop read the round and
/// the lenses from it, so they cannot disagree (21, rule 1).
///
/// # Panics
///
/// Never: the index holds ids.
pub fn reviewed(vault: &Vault, index: &Index, issue: &str) -> Result<intents::Issue, Refusal> {
    let header = index.header(issue)?.unwrap_or_default();
    let labels = super::labels(&header);
    Ok(intents::Issue {
        runs: runs(index, issue)?,
        reviews: reviews(vault, index, issue)?,
        declared: labels.declared,
        complex: labels.complex,
        unread: labels.unread,
        delta: !crate::carried_deltas(index, &Id::parse(issue).expect("the index holds ids"))?.is_empty(),
        ..intents::Issue::default()
    })
}

/// The [`cells`] of each finding of a review body, in row order: each line that starts with `|`, less the header and
/// separator.
fn findings(body: &str) -> Vec<Vec<String>> {
    body.lines().map(str::trim).filter(|l| l.starts_with('|')).skip(2).map(cells).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_reads_an_escaped_pipe_as_a_literal_one() {
        assert_eq!(cells("| error | f:3 | `a \\| b` |"), ["error", "f:3", "`a | b`"]);
        assert_eq!(cells("| error | f:3 | `a \\| b` | |"), ["error", "f:3", "`a | b`", ""], "an empty last cell");
        assert_eq!(cells("| error | f:3 | no closing bar"), ["error", "f:3", "no closing bar"]);
        let body =
            "head: h\n\n| severity | location | finding | of |\n|---|---|---|---|\n| error | f:3 | `x \\| y` | r#2 |\n";
        assert_eq!(of_cells(body), BTreeMap::from([(1, ("r".to_string(), 2))]));
        assert_eq!(findings(body)[0][2], "`x | y`");
    }
}
