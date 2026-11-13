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
            .zip(severities(&w.body))
            .map(|(n, severity)| Row {
                error: rounds::is_error(&severity),
                risk: severity == RISK_ROW,
                late: late.contains(&n),
                answer: answers.get(&n).copied(),
                of: of.get(&n).cloned(),
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
    let table: Vec<Vec<&str>> = body
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('|'))
        .map(|l| l[1..].strip_suffix('|').unwrap_or(&l[1..]).split('|').map(|c| c.trim().trim_matches('`')).collect())
        .collect();
    let Some(at) = table.first().and_then(|h| h.iter().position(|c| *c == "of")) else { return BTreeMap::new() };
    let cell = |row: &Vec<&str>| {
        let (id, n) = row.get(at)?.rsplit_once('#')?;
        Some((id.to_string(), n.parse().ok()?))
    };
    (1..).zip(table.iter().skip(2)).filter_map(|(n, row)| Some((n, cell(row)?))).collect()
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
    Ok(intents::Issue {
        runs: runs(index, issue)?,
        reviews: reviews(vault, index, issue)?,
        declared: text(&header, "risk").and_then(|l| l.parse().ok()),
        delta: !crate::carried_deltas(index, &Id::parse(issue).expect("the index holds ids"))?.is_empty(),
        ..intents::Issue::default()
    })
}

/// The severity cell of each finding of a review body, in row order: the first cell of each of the [`rows`].
fn severities(body: &str) -> Vec<String> {
    let table = body.lines().map(str::trim_start).filter(|l| l.starts_with('|'));
    let first = |l: &str| l[1..].split('|').next().unwrap_or_default().trim().trim_matches('`').to_string();
    table.skip(2).map(first).collect()
}
