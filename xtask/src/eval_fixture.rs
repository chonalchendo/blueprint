//! A fixture and the outcome it is graded against (29, design change 2). A fixture is one file in
//! `xtask/evals/<skill>/`: the clone's recorded base, the issue the sandbox holds, and a ground truth. For `bp-review`
//! and `bp-respond` the head is a pull request and the truth is the findings of a past review that the worker fixed
//! (`analysis/review-pr-*.md`, "Disposition"). For `bp-complete` the head is the landed squash, and the truth is the
//! issue's crates: the first two path parts of the files that squash changed, as `replay-evidence.sh` computes them.
//!
//! ```text
//! base: <sha>
//! head: <sha>
//! title: <issue title>
//! stack: <sha> | <title>                   (complete only: the dependency's pull request head)
//! scope: <path part> <path part> ...       (complete only)
//! truth:
//! - [<severity>] <path>:<line>[-<line>][, <line>] | <what the finding said>
//! ---
//! <the issue body, verbatim>
//! ```
//!
//! `holdout.txt` in the same directory lists the held-out ids, one per line: no run reads those.

use std::path::Path;

use anyhow::{Context, Result, bail};

pub struct Fixture {
    pub id: String,
    pub base: String,
    pub head: String,
    pub title: String,
    pub truth: Vec<Truth>,
    pub issue: String,
    /// The head and title of the dependency's pull request: the sandbox opens it, and the issue depends on it.
    pub stack: Option<(String, String)>,
    pub scope: Vec<String>,
}

/// One finding of the ground truth: a file and the lines it named. No line means the whole file.
pub struct Truth {
    path: String,
    lines: Vec<(u32, u32)>,
    pub what: String,
    /// The first column of the review table: `bp-respond`'s sandbox records these rows.
    severity: String,
}

pub struct Finding {
    pub location: String,
    pub text: String,
}

/// A review as the role returned it: the head it names and the rows of its findings table.
pub struct Review {
    pub head: String,
    pub findings: Vec<Finding>,
    pub tables: usize,
    /// The `head:` line to the last table row: what `record review` takes as the body.
    pub body: String,
}

/// A finding at most this many lines from a ground-truth range still matches it: reviewers cite the line of the
/// symptom or of its cause.
pub const NEAR: u32 = 3;

impl Fixture {
    pub fn parse(id: &str, text: &str) -> Result<Self> {
        let (head, issue) =
            text.split_once("\n---\n").with_context(|| format!("{id}: no `---` line before the issue"))?;
        let mut f = Self {
            id: id.into(),
            base: String::new(),
            head: String::new(),
            title: String::new(),
            truth: Vec::new(),
            issue: issue.trim().to_string(),
            stack: None,
            scope: Vec::new(),
        };
        for line in head.lines().filter(|l| !l.trim().is_empty()) {
            if let Some(item) = line.strip_prefix("- ") {
                let (at, what) = item.split_once('|').with_context(|| format!("{id}: truth `{item}` has no `|`"))?;
                f.truth.push(Truth::parse(at.trim(), what.trim()).with_context(|| format!("{id}: truth `{item}`"))?);
            } else if let Some((key, value)) = line.split_once(':') {
                let value = value.trim().to_string();
                match key {
                    "base" => f.base = value,
                    "head" => f.head = value,
                    "title" => f.title = value,
                    "scope" => f.scope = value.split_whitespace().map(str::to_string).collect(),
                    "stack" => {
                        let (sha, title) =
                            value.split_once('|').with_context(|| format!("{id}: stack is `<sha> | <title>`"))?;
                        f.stack = Some((sha.trim().into(), title.trim().into()));
                    }
                    "truth" => {}
                    _ => bail!("{id}: unknown key `{key}`"),
                }
            }
        }
        if [&f.base, &f.head, &f.title].iter().any(|v| v.is_empty()) || (f.truth.is_empty() && f.scope.is_empty()) {
            bail!("{id}: base, head, title and a truth item or a scope are required");
        }
        Ok(f)
    }

    /// The fixtures of `dir` that are not held out, by id; `only` narrows to one, and a held-out id refuses.
    pub fn load(dir: &Path, only: Option<&str>) -> Result<Vec<Self>> {
        let held = holdout(dir);
        if let Some(id) = only.filter(|id| held.iter().any(|h| h == id)) {
            bail!("{id} is held out: no run reads it (29, decision 9)");
        }
        let mut all = Vec::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
            let path = entry?.path();
            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            if path.extension().is_some_and(|e| e == "md") && !held.contains(&id) && only.is_none_or(|o| o == id) {
                all.push(Self::parse(&id, &std::fs::read_to_string(&path)?)?);
            }
        }
        all.sort_by(|a, b| a.id.cmp(&b.id));
        if all.is_empty() {
            bail!("no fixture in {}{}", dir.display(), only.map(|o| format!(" named {o}")).unwrap_or_default());
        }
        Ok(all)
    }

    /// The ground-truth item each finding matches, if any. A finding matches at most one item: the nearest by lines,
    /// the narrower range on a tie, then the first.
    pub fn assign(&self, findings: &[Finding]) -> Vec<Option<usize>> {
        let nearest = |f: &Finding| {
            let near = self.truth.iter().enumerate().filter_map(|(i, t)| Some((t.gap(&f.location)?, i)));
            near.min().map(|(_, i)| i)
        };
        findings.iter().map(nearest).collect()
    }

    /// How many ground-truth items some finding matches.
    pub fn hits(&self, assigned: &[Option<usize>]) -> usize {
        (0..self.truth.len()).filter(|i| assigned.contains(&Some(*i))).count()
    }

    /// The review the session records for `bp-respond`: the `head:` line and one table row per ground-truth item.
    pub fn review_body(&self) -> String {
        let rows = self.truth.iter().map(|t| format!("| {} | `{}` | {} |", t.severity, t.location(), t.what));
        let table = ["| severity | location | finding |".into(), "|---|---|---|".into()];
        format!("head: {}\n{}", self.head, table.into_iter().chain(rows).collect::<Vec<String>>().join("\n"))
    }

    /// How many ground-truth items a text names, in any form: its file and a line within `NEAR` of the item's.
    /// A reply with no findings table still counts, so recall does not hang on the shape only `bp-review` defines.
    pub fn named_in(&self, text: &str) -> usize {
        self.truth.iter().filter(|t| t.named_in(text)).count()
    }
}

/// The ids in `holdout.txt`; a missing file holds none.
pub fn holdout(dir: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join("holdout.txt")).unwrap_or_default();
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect()
}

impl Truth {
    fn parse(at: &str, what: &str) -> Result<Self> {
        let (severity, at) = match at.strip_prefix('[').and_then(|a| a.split_once(']')) {
            Some((s, rest)) => (s, rest.trim()),
            None => ("warn", at),
        };
        let (path, lines) = locate(at).context("a location is `<path>[:<line>[-<line>]]`")?;
        Ok(Self { path, lines, what: what.to_string(), severity: severity.to_string() })
    }

    pub fn location(&self) -> String {
        let lines: Vec<String> =
            self.lines.iter().map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") }).collect();
        if lines.is_empty() { self.path.clone() } else { format!("{}:{}", self.path, lines.join(", ")) }
    }

    fn in_file(&self, path: &str) -> bool {
        self.path == path || self.path.ends_with(&format!("/{path}")) || path.ends_with(&format!("/{}", self.path))
    }

    /// How far a finding's location is from this item, or `None` when it names another file or lines more than
    /// `NEAR` away. The pair orders by lines apart, then by the item's width, so a narrow item beats a wide one that
    /// holds the same line. A location with no lines, or an item with none, is as far as can be.
    fn gap(&self, location: &str) -> Option<(u32, u32)> {
        let (path, lines) = locate(location)?;
        if !self.in_file(&path) {
            return None;
        }
        let width = self.lines.iter().map(|&(a, b)| b - a).min().unwrap_or(u32::MAX);
        if self.lines.is_empty() || lines.is_empty() {
            return Some((u32::MAX, width));
        }
        let apart = |(a, b): (u32, u32), (c, d): (u32, u32)| c.saturating_sub(b).max(a.saturating_sub(d));
        let gap = self.lines.iter().flat_map(|&t| lines.iter().map(move |&l| apart(t, l))).min()?;
        (gap <= NEAR).then_some((gap, width))
    }

    /// Whether the text writes this item's file name followed, within 40 characters, by a number inside the item's
    /// lines widened by `NEAR`. With no lines on the item, the file name is enough.
    fn named_in(&self, text: &str) -> bool {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        text.match_indices(name).any(|(at, _)| {
            let after: String = text[at + name.len()..].chars().take(40).collect();
            let numbers = after.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u32>().ok());
            self.lines.is_empty()
                || numbers.into_iter().any(|n| self.lines.iter().any(|&(a, b)| n + NEAR >= a && n <= b + NEAR))
        })
    }
}

/// A path and the line ranges after its colon. A word after the location, as in `a.rs:22-33 (note)`, is ignored.
fn locate(at: &str) -> Option<(String, Vec<(u32, u32)>)> {
    let at = at.trim().trim_start_matches('`');
    let (path, tail) = at.split_once(':').unwrap_or((at, ""));
    let path = path.split_whitespace().next()?.trim_end_matches('`').trim_start_matches("./");
    let numbers: String = tail.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | ',' | ' ')).collect();
    let lines = numbers
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            let (a, b) = part.split_once('-').unwrap_or((part, part));
            Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
        })
        .collect();
    Some((path.to_string(), lines))
}

impl Review {
    /// `None` when the text has no `head:` line, or the line is followed by no table: nothing can be recorded.
    pub fn parse(text: &str) -> Option<Self> {
        let lines: Vec<&str> = text.lines().collect();
        let at = lines.iter().position(|l| l.trim().trim_matches('`').starts_with("head:"))?;
        let head = lines[at].trim().trim_matches('`').trim_start_matches("head:").trim().trim_matches('`').to_string();
        let (mut findings, mut tables, mut last, mut inside) = (Vec::new(), 0, None, false);
        for (i, line) in lines.iter().enumerate().skip(at + 1) {
            let line = line.trim();
            if !line.starts_with('|') {
                inside = false;
                continue;
            }
            tables += usize::from(!inside);
            (inside, last) = (true, Some(i));
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            let rule = cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')));
            if cells.len() >= 3 && !rule && !cells[0].eq_ignore_ascii_case("severity") {
                findings.push(Finding { location: cells[1].to_string(), text: cells[2..].join("|") });
            }
        }
        let body = lines[at..=last?].join("\n");
        Some(Self { head, findings, tables, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "base: b1\nhead: h1\ntitle: T\ntruth:\n- a/x.rs:20 | one\n- a/x.rs:11-27 | two\n- justfile:18-21 | three\n---\n## Problem\n\nbody\n";

    #[test]
    fn parses_a_fixture_and_matches_by_file_and_nearby_lines() {
        let f = Fixture::parse("pr-1", FIXTURE).unwrap();
        assert_eq!((f.base.as_str(), f.head.as_str(), f.issue.as_str()), ("b1", "h1", "## Problem\n\nbody"));
        assert_eq!(f.truth[1].location(), "a/x.rs:11-27");
        let at = |l: &str| Finding { location: l.into(), text: String::new() };
        let hits = |l: &[&str]| f.hits(&f.assign(&l.iter().map(|x| at(x)).collect::<Vec<_>>()));
        assert_eq!(hits(&["x.rs:22", "justfile:21", "`a/x.rs`"]), 3, "a basename, a near line, a bare file");
        assert_eq!(hits(&["a/x.rs:60", "other.rs:20", "projects/bp/designs/x"]), 0);
        assert_eq!(f.assign(&[at("a/x.rs:20")]), vec![Some(0)], "the nearest item takes the finding");
        assert_eq!(f.assign(&[at("a/x.rs:60")]), vec![None]);
    }

    #[test]
    fn a_fixture_without_its_parts_refuses() {
        assert!(Fixture::parse("x", "base: b\n---\nbody").is_err(), "no head, title or truth");
        assert!(Fixture::parse("x", "base: b\nhead: h\ntitle: T\ntruth:\n- a.rs | w\n").is_err(), "no `---`");
        assert!(Fixture::parse("x", "color: red\n---\nbody").is_err());
    }

    #[test]
    fn a_held_out_fixture_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pr-1.md"), FIXTURE).unwrap();
        std::fs::write(dir.path().join("pr-2.md"), FIXTURE).unwrap();
        std::fs::write(dir.path().join("holdout.txt"), "# held\npr-2\n").unwrap();
        let ids: Vec<_> = Fixture::load(dir.path(), None).unwrap().into_iter().map(|f| f.id).collect();
        assert_eq!(ids, ["pr-1"]);
        assert!(Fixture::load(dir.path(), Some("pr-2")).is_err());
        assert_eq!(holdout(dir.path()), ["pr-2"]);
    }

    #[test]
    fn a_fixture_holds_a_stack_a_scope_and_the_rows_of_its_review() {
        let text = "base: b1\nhead: h1\ntitle: T\nstack: d1 | The dependency\nscope: a/x justfile\ntruth:\n- [major] a/x.rs:20 | One.\n- b.rs | Two.\n---\nbody\n";
        let f = Fixture::parse("m-1", text).unwrap();
        assert_eq!(f.stack, Some(("d1".into(), "The dependency".into())));
        assert_eq!(f.scope, ["a/x", "justfile"]);
        let review = Review::parse(&f.review_body()).unwrap();
        assert_eq!((review.head.as_str(), review.tables, review.findings.len()), ("h1", 1, 2));
        let rows: Vec<String> = f.review_body().lines().skip(3).map(str::to_string).collect();
        assert_eq!(rows, ["| major | `a/x.rs:20` | One. |", "| warn | `b.rs` | Two. |"]);
        assert!(Fixture::parse("x", "base: b\nhead: h\ntitle: T\n---\nbody").is_err(), "no truth and no scope");
        assert!(Fixture::parse("x", "base: b\nhead: h\ntitle: T\nscope: a\nstack: only-a-sha\n---\nb").is_err());
        assert!(Fixture::parse("x", "base: b\nhead: h\ntitle: T\nscope: a\n---\nbody").is_ok(), "a scope is a truth");
    }

    /// The shipped fixtures: each parses, names commits of this repository when the history is present, and a
    /// complete fixture's scope is what its landed commit changed (the first two path parts, as 29 computes them).
    #[test]
    fn the_shipped_fixtures_match_the_history_they_name() {
        let evals = Path::new(env!("CARGO_MANIFEST_DIR")).join("evals");
        let root = evals.parent().unwrap().parent().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git").args(args).current_dir(root).output().unwrap();
            out.status.success().then(|| String::from_utf8(out.stdout).unwrap())
        };
        for skill in ["bp-review", "bp-complete", "bp-respond"] {
            let fixtures = Fixture::load(&evals.join(skill), None).unwrap();
            assert!(!fixtures.is_empty() && !holdout(&evals.join(skill)).is_empty(), "{skill}");
            for f in fixtures {
                let known = |sha: &str| git(&["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_some();
                if !known(&f.base) {
                    eprintln!("skipped {skill}/{}: history is shallow", f.id);
                    continue;
                }
                let stack = f.stack.iter().map(|s| s.0.as_str());
                assert!(stack.chain([f.head.as_str()]).all(known), "{skill}/{}", f.id);
                if skill == "bp-complete" {
                    let names = git(&["diff", "--name-only", &format!("{}^", f.head), &f.head]).unwrap();
                    let mut crates: Vec<String> = names.lines().map(crate::eval_end::crate_of).collect();
                    crates.sort();
                    crates.dedup();
                    assert_eq!(f.scope, crates, "{}", f.id);
                    assert!(f.issue.contains("check shell {name="), "{}: a worker needs a named block", f.id);
                }
            }
        }
    }

    #[test]
    fn parses_a_review() {
        let text = "Here it is.\nhead: `abc123`\n| severity | location | finding |\n|---|---|---|\n| error | `a/x.rs:20` | One. |\n| warn | justfile:19 | Two | extra |\n\nbp record review x\n";
        let r = Review::parse(text).unwrap();
        assert_eq!((r.head.as_str(), r.tables, r.findings.len()), ("abc123", 1, 2));
        assert_eq!(r.findings[1].text, "Two|extra");
        assert!(r.body.starts_with("head:") && r.body.ends_with("extra |") && !r.body.contains("bp record"));
        assert!(Review::parse("no head line\n| a | b | c |").is_none());
        assert!(Review::parse("head: abc\nno table").is_none());
        let two = Review::parse("head: a\n| severity | location | finding |\n|-|-|-|\ntext\n| x | y | z |\n").unwrap();
        assert_eq!(two.tables, 2, "a second table breaks the shape");
    }
}
