//! The row checks of `record` (15, record; 29 design change 6). A review holds the head it read and one findings
//! table, and each row names a severity, a place in the diff of that head and what is wrong; a declined disposition
//! says why. A review the responder cannot count or place is refused when it is written, not when it is answered. A
//! `risk` row raises the issue's level (37, design change 2), so a misspelt one is refused too, as is any severity
//! outside `error`, `warn` and `risk`. The head is a commit the claim's clone holds. A review names its round, lens and
//! mode, which must be the head's (37, design change 6); a row may name the earlier row it re-reports in its `of`
//! cell; a `verify` review gains `late`, its new error rows at paths outside the fix, and a late row is declined only
//! with a live follow-up issue under the same milestone (37, design change 7).

use std::collections::BTreeMap;
use std::path::Path;

use bp_change::DispositionAs;
use bp_change::intents;
use bp_change::risk::RISK_ROW;
use bp_change::rounds::{self, Lens, Mode};
use bp_read::inputs::{cells, field, held};
use bp_vault::{Code, Id, Index, Kind, Refusal, Vault};
use serde_json::Value;

use crate::claim::git;

const RULE: &str = "a review holds a `head:` line and one findings table; each row names a severity, a location in \
                    the diff of its head or a node, and the finding (15, record)";

/// The lines of each file that the diff shows, by path.
type Hunks = BTreeMap<String, Vec<(u64, u64)>>;

/// Refuses a record whose rows fail their kind's check: a review's body or round, or a declined disposition with no
/// reason, or a late row's with no follow-up issue. Returns the fields the tool adds: a `verify` review's `late`.
pub(crate) fn check(
    vault: &Vault,
    index: &Index,
    kind: Kind,
    parent: Option<&Id>,
    given: &BTreeMap<&str, &str>,
    body: &str,
) -> Result<BTreeMap<String, Value>, Refusal> {
    match kind {
        Kind::Review => review(vault, index, parent.expect("a review sits under an issue"), given, body),
        Kind::Disposition if given["as"] == DispositionAs::Declined.as_str() => {
            let reason = given.get("reason").map_or("", |r| r.trim());
            if reason.is_empty() {
                let fix = "`reason` is required when as=declined: --field reason=\"<the issue, design or convention \
                           that rules the finding out>\"";
                return Err(crate::record::schema("disposition reason".into(), fix.into()));
            }
            late(vault, index, parent.expect("a disposition sits under a review"), given["finding"], reason)?;
            Ok(BTreeMap::new())
        }
        _ => Ok(BTreeMap::new()),
    }
}

/// A decline of a late row names its follow-up issue (37, design change 7): an issue under the reviewed issue's
/// milestone, `open` or `in-progress`, so the stamp waits on it. The row's `late` is read as the review's first commit
/// wrote it.
fn late(vault: &Vault, index: &Index, review: &Id, finding: &str, reason: &str) -> Result<(), Refusal> {
    let Some(header) = index.header(review.as_str())? else { return Ok(()) };
    let issue = header.get("under").and_then(Value::as_str).unwrap_or_default();
    let n = finding.parse::<u64>().unwrap_or_default();
    let reviews = bp_read::inputs::reviews(vault, index, issue)?;
    let row = reviews.iter().find(|r| r.id == review.as_str()).and_then(|r| (1..).zip(&r.rows).find(|(m, _)| *m == n));
    let row = row.map(|(_, row)| row);
    if !row.is_some_and(|r| r.late) || bp_read::inputs::follow_up(index, issue, reason)?.is_some() {
        return Ok(());
    }
    let milestone = index.header(issue)?.and_then(|h| h.get("under")?.as_str().map(str::to_string));
    let fix = format!(
        "finding {n} is a late error outside the fix: name its follow-up issue in the reason, as --field \
         reason=\"follow-up <issue id>\"; the session creates it under {}, and it must be open or in-progress",
        milestone.as_deref().unwrap_or("the issue's milestone (the issue sits under none)")
    );
    Err(crate::record::schema(format!("{review} disposition {n} reason"), fix))
}

/// The round, lens and mode a review of `head` must name: the head's round, `full` and a round-one lens at round
/// one; after it, `verify`, or `full` for a lens of the level that no review of the issue has read (37, design change
/// 6; review of #98, row 32). A `verify` review while round one is open names the lenses it lacks. `pr` is the open
/// pull request's head, which takes a review whatever its history (row 30).
fn round(
    issue: &intents::Issue,
    id: &Id,
    head: &str,
    given: &BTreeMap<&str, &str>,
    (worktree, pr): (&Path, Option<&str>),
) -> Result<Mode, Refusal> {
    let round = intents::round(issue, head);
    let verify = Lens::Verify.as_str();
    let missing = intents::missing(issue, head);
    // A review at a head whose round has closed would move the fix diff's base back, and two reviews of one round
    // would disagree on what is late (review of #98, row 27). It has closed when a later round is recorded, or a
    // reviewed head descends from it. Records of one second sort by id, so their order cannot say which came later.
    let descends = |h: &str| git(worktree, &["merge-base", "--is-ancestor", head, h]).is_ok();
    let later = issue.reviews.iter().find(|r| r.head != head && (r.round > round || descends(&r.head)));
    if let Some(later) = later.filter(|_| pr != Some(head)) {
        let open = later.round == round && !intents::missing(issue, &later.head).is_empty();
        let state = if open {
            format!("which is open at {}", later.head)
        } else {
            format!("which closed when {} was reviewed", later.head)
        };
        let fix = format!(
            "{head} is round {round} of {id}, {state}: review the head of the pull request, as `bp walk {id} --for \
             review` names it"
        );
        return Err(crate::record::schema(format!("{id} review round"), fix));
    }
    if round == 1 && (given["mode"] == Mode::Verify.as_str() || given["lens"] == verify) {
        let lenses: Vec<&str> = missing.iter().map(|l| l.as_str()).collect();
        let fix = if lenses.is_empty() {
            format!(
                "{head} is round 1 of {id}, and each of its lenses is recorded: a `verify` review reads the next head"
            )
        } else {
            format!(
                "round one of {id} is open at {head}: missing lenses: {}; record each with --field round=1 --field \
                 mode=full --field lens=<lens>",
                lenses.join(", ")
            )
        };
        return Err(crate::record::schema(format!("{id} review round"), fix));
    }
    let lens = missing.iter().any(|l| *l != Lens::Verify && l.as_str() == given["lens"]);
    let mode = if lens { Mode::Full } else { rounds::mode(round) };
    if given["round"].parse() != Ok(round) || given["mode"] != mode.as_str() {
        let fix = format!("{head} is round {round} of {id}: --field round={round} --field mode={mode}");
        return Err(crate::record::schema(format!("{id} review round"), fix));
    }
    match mode {
        Mode::Verify if given["lens"] != verify => {
            let lenses: Vec<&str> = missing.iter().filter(|l| **l != Lens::Verify).map(|l| l.as_str()).collect();
            let also = if lenses.is_empty() {
                String::new()
            } else {
                format!("; a lens review with --field mode=full takes one of: {}", lenses.join(", "))
            };
            let fix = format!("round {round} verifies the fix in one review: --field lens={verify}{also}");
            Err(crate::record::schema(format!("{id} review lens"), fix))
        }
        _ => Ok(mode),
    }
}

/// Refuses a head that no commit of the claim's clone is, after a fetch of the claim's branch, one that does not lie
/// past the claim's base, and one that is not the open pull request's head or behind it: a review at a sha that is no
/// head of the work, such as the base, a sibling issue's head or a newer commit of the base branch, would count as a
/// round (37, design change 6; review of #98, row 33). With no pull request in view, the claim branch's tip in the
/// clone or at origin stands in for its head. Returns the clone and the pull request's head.
fn commit(vault: &Vault, index: &Index, issue: &Id, head: &str) -> Result<(String, Option<String>), Refusal> {
    let refuse = |fix: String| Refusal::new(Code::FindingInvalid, format!("{issue} review head {head}"), RULE, fix);
    let Some(claim) = held(index, issue)? else {
        return Err(refuse(format!(
            "{issue} holds no claim, so no clone holds the head: record the review while it is held"
        )));
    };
    let worktree = field(&claim, "worktree").expect("a claim holds its worktree and base");
    let base = field(&claim, "base").expect("a claim holds its worktree and base");
    let dir = Path::new(&worktree);
    let branch = crate::claim::branch(issue);
    let known = || git(dir, &["cat-file", "-e", &format!("{head}^{{commit}}")]).is_ok();
    if !known() {
        // The pull request's head may be newer than the clone; a failed fetch leaves the refusal below to say so.
        let _ = git(dir, &["fetch", "-q", "origin", &format!("refs/heads/{branch}")]);
    }
    if !known() {
        return Err(refuse(format!(
            "no commit of {worktree} is {head}, after a fetch of the claim's branch: write the head of the pull \
             request, as `bp walk {issue} --for review` names it"
        )));
    }
    let ancestor = |a: &str, b: &str| git(dir, &["merge-base", "--is-ancestor", a, b]).is_ok();
    // A land of the layer below rebases a stacked claim off its base (`stack::move_onto`), so its head need only not
    // be the base or behind it.
    let past = if field(&claim, "base_issue").is_some() {
        !ancestor(head, &base)
    } else {
        ancestor(&base, head) && !ancestor(head, &base)
    };
    if !past {
        return Err(refuse(format!(
            "{head} is the claim's base {base} or does not descend from it: write the head of the pull request, as \
             `bp walk {issue} --for review` names it"
        )));
    }
    let pr = crate::done::open(&vault.root, index, issue, &mut None)?.map(|(p, _)| p.head_ref_oid);
    let tips = match &pr {
        Some(p) => vec![p.clone()],
        None => vec![format!("refs/heads/{branch}"), format!("refs/remotes/origin/{branch}")],
    };
    if !tips.iter().any(|tip| known_tip(dir, &branch, tip) && ancestor(head, tip)) {
        return Err(refuse(format!(
            "{head} is not {} or behind it on the claim's branch: write the head of the pull request, as `bp walk \
             {issue} --for review` names it",
            tips.join(" or ")
        )));
    }
    Ok((worktree, pr))
}

/// The clone holds `tip`: a pull request's head newer than the clone comes with a fetch of the claim's branch.
fn known_tip(dir: &Path, branch: &str, tip: &str) -> bool {
    let known = || git(dir, &["cat-file", "-e", &format!("{tip}^{{commit}}")]).is_ok();
    known() || (git(dir, &["fetch", "-q", "origin", &format!("refs/heads/{branch}")]).is_ok() && known())
}

fn review(
    vault: &Vault,
    index: &Index,
    issue: &Id,
    given: &BTreeMap<&str, &str>,
    body: &str,
) -> Result<BTreeMap<String, Value>, Refusal> {
    let head = given["head"];
    let refuse =
        |what: &str, fix: String| Refusal::new(Code::FindingInvalid, format!("{issue} review {what}"), RULE, fix);
    // The reviewer's text may shorten the sha or quote it; it still names this head.
    let mut named = body.lines().filter_map(|l| l.trim().strip_prefix("head:")).map(|h| h.trim().trim_matches('`'));
    if !named.any(|h| h.len() >= 7 && head.starts_with(h)) {
        return Err(refuse("head", format!("write `head: {head}` as line 1 of the body")));
    }
    let (worktree, pr) = commit(vault, index, issue, head)?;
    // The rows are what `bp_read::inputs::reduced` counts: each line that starts with `|`, less the header and
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
    let issued = bp_read::inputs::reviewed(vault, index, issue.as_str())?;
    let of = bp_read::inputs::of_cells(body);
    let at_of = cells(table[0]).iter().position(|c| c == "of");
    let mut diff = None;
    // Each error row's location, whether it names a node, which no diff shows, and whether it re-reports a row.
    let mut errors = Vec::new();
    for (n, row) in table[2..].iter().enumerate().map(|(i, r)| (i + 1, r)) {
        let cells = cells(row);
        if cells.len() < 3 || cells[..3].iter().any(String::is_empty) {
            let fix = format!(
                "fill row {n}: `| error, warn or risk | <path>:<line> or a node id | what is wrong, and what would pass |`"
            );
            return Err(refuse(&format!("finding {n}"), fix));
        }
        // Only the exact word raises the level, so a near miss (`Risk`, `risk/error`, a zero-width space inside)
        // would raise nothing and say nothing.
        let severity = cells[0].trim_matches('`');
        let letters: String = severity.chars().filter(|c| c.is_alphanumeric()).collect();
        if severity != RISK_ROW && letters.to_lowercase().contains(RISK_ROW) {
            let fix =
                format!("row {n}'s severity is `{severity}`: write `{RISK_ROW}`, in lower case, to raise the level");
            return Err(refuse(&format!("finding {n}"), fix));
        }
        // A word the tool does not know reads as an error, so a meant warn would block the land in silence.
        if !rounds::SEVERITIES.contains(&severity) {
            let fix = format!("row {n}'s severity is `{severity}`: write one of {}", rounds::SEVERITIES.join(", "));
            return Err(refuse(&format!("finding {n}"), fix));
        }
        let cell = at_of.and_then(|i| cells.get(i)).map(|c| c.trim_matches('`')).filter(|c| !c.is_empty());
        if let Some(cell) = cell {
            re_reports(&issued, issue, head, (n, severity, cell), of.get(&(n as u64)))?;
        }
        let at = cells[1].trim_matches('`').to_string();
        let node = Id::parse(&at).is_ok() && index.header(&at)?.is_some();
        if rounds::is_error(severity) {
            // An earlier error row at the same location is the same defect found again: a re-report, never late
            // (review of #98, row 24).
            let again = issued.reviews.iter().flat_map(|r| &r.rows).any(|r| r.error && r.at == at);
            errors.push((n as u64, at.clone(), node || cell.is_some() || again));
        }
        if node {
            continue;
        }
        if diff.is_none() {
            diff = Some(hunks(index, issue, None, head)?);
        }
        if !diff.as_ref().is_some_and(|h| inside(h, &at)) {
            let fix = format!(
                "finding {n} cites `{at}`, which the diff of {head} against the claim's base does not show: cite a \
                 file and lines that `git diff <base> {head}` shows, or a node id, or drop the finding"
            );
            return Err(refuse(&format!("finding {n}"), fix));
        }
    }
    let mut added = BTreeMap::new();
    if round(&issued, issue, head, given, (Path::new(&worktree), pr.as_deref()))? == Mode::Full {
        return Ok(added);
    }
    // The fix diff is the diff from the head the round before read. A new error row at a path it does not show is
    // late; a row that cites a node or re-reports an earlier row never is.
    let Some(prior) =
        rounds::prior_head(&issued.reviews, intents::round(&issued, head)).filter(|_| errors.iter().any(|e| !e.2))
    else {
        return Ok(added);
    };
    let fix = hunks(index, issue, Some(prior), head)?;
    let late: Vec<Value> =
        errors.iter().filter(|(_, at, kept)| !*kept && !inside(&fix, at)).map(|(n, ..)| (*n).into()).collect();
    if !late.is_empty() {
        added.insert("late".into(), Value::Array(late));
    }
    Ok(added)
}

/// Refuses an `of` cell that names no row of an earlier review of `issue`, one on a row that is no error, and one that
/// names a review of `head`. A re-report carries an error row to a later head; a `warn` row or a second lens at the
/// same head would clear it with no fix (37, design change 6; review of #98, row 20). `row` is the row's number,
/// severity and `of` cell; `named` is what the cell parses to.
fn re_reports(
    issued: &intents::Issue,
    issue: &Id,
    head: &str,
    (n, severity, cell): (usize, &str, &str),
    named: Option<&(String, u64)>,
) -> Result<(), Refusal> {
    let refuse = |fix: String| Refusal::new(Code::FindingInvalid, format!("{issue} review finding {n}"), RULE, fix);
    let earlier = named.and_then(|(id, m)| {
        let earlier = issued.reviews.iter().find(|r| r.id == *id)?;
        (1..=earlier.rows.len() as u64).contains(m).then_some(earlier)
    });
    let Some(earlier) = earlier else {
        return Err(refuse(format!(
            "row {n}'s `of` cell is `{cell}`: write `<review id>#<row>` of an earlier review of {issue}, as `bp list \
             review --under {issue}` names it, or leave the cell empty; write a pipe inside a finding as `\\|`"
        )));
    };
    if !rounds::is_error(severity) || earlier.head == head {
        return Err(refuse(format!(
            "row {n}'s `of` cell names {}: only an `error` row re-reports, and only a row of an earlier head; leave \
             the cell empty, and decline a duplicate at this head with the other row's id",
            earlier.id
        )));
    }
    Ok(())
}

/// The lines that `git diff <base> <head>` shows in the claim's worktree, `base` else the claim's, with git's three
/// lines of context: the new side of each file, the old side of a deleted one. Flags fix the format whatever the user's git config says.
fn hunks(index: &Index, issue: &Id, base: Option<&str>, head: &str) -> Result<Hunks, Refusal> {
    let Some(claim) = held(index, issue)? else {
        let fix = format!(
            "{issue} holds no claim, so no diff places a row: cite a node id, or record the review on the issue after \
             its claim"
        );
        return Err(Refusal::new(Code::FindingInvalid, format!("{issue} review"), RULE, fix));
    };
    let worktree = field(&claim, "worktree").expect("a claim holds its worktree and base");
    let base = base.map_or_else(|| field(&claim, "base").expect("a claim holds its worktree and base"), str::to_string);
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
