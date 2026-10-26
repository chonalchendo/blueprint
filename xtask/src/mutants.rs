//! `cargo xtask mutants`, the planted-defect fixtures of the review eval (30, design change 1).
//!
//! ```text
//! cargo xtask mutants --pr <n> [--pr <n>]... --count <k> --seed <s> [--out <dir>]
//! cargo xtask mutants --discard <id> --reason <text> [--out <dir>]
//! ```
//!
//! For each landed pull request it finds the squash on master whose subject ends `(#<n>)`; the base is its parent.
//! In a throwaway clone at that head it lists `cargo-mutants`' candidates on the lines the diff changed. A candidate
//! counts only when its span starts on an added (`+`) line of `base..head`, read from the diff's hunks: `--in-diff`
//! also lists a candidate on an unchanged line beside a change, and the planted diff then shows the original line
//! removed and the mutated one added, which a reader finds by reading. It drops the
//! guard mutants (`MatchArmGuard`): the tool writes a literal `true` or `false`, which any reader sees. It sorts the
//! rest by name, shuffles them with a seed of `s` and the pull request, and groups them by package: each package's
//! candidates stay in seed order, and the packages follow in the order of their first candidate. It walks them in that
//! order, seed order grouped by package, so a package's binaries build once. A mutant that CI
//! catches is a CI finding, not a review finding, so a kept mutant passes all of CI at the planted head: first
//! `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`, the cheap gates, then the
//! tests of every crate the diff touches. The tests run in chunks of 8 candidates of one package, one `cargo mutants`
//! call each (`mutants_walk.rs` says why). Each test has a timeout of 300 s (`--timeout 300`); a mutant that times
//! out is tested and not kept, since CI fails on a hang. It keeps at most one mutant per function and skips a
//! candidate whose function it already keeps. It stops when it keeps `k`, or after 160 tested candidates. So a run
//! tests few candidates past the `k`th survivor: the kept set is the first `k` survivors in seed order grouped by
//! package, one per function, so the same seed and candidate list keep the same mutants when the test outcomes are
//! the same. It commits each kept mutant on the head as `bp-eval` and records the diff as git writes it, untrimmed:
//! a hunk's last context line may be blank, and without it the diff does not apply. Before CI it checks that the
//! recorded diff applies at the head (`git apply --check`); a candidate whose diff does not is not clean, with a log
//! line, since the sandbox plants it with `git apply`. It writes `mutant-<n>-<i>.md`
//! for each and `control-<n>.md` with no mutant (`eval_fixture.rs` has the shape), into `--out` (default
//! `xtask/evals/bp-review-mutants`).
//!
//! Before the walk it runs the tests of every touched crate once, at the head. When they fail, every mutant would read
//! as caught, so it skips the pull request, names the failing crates, and counts the skip in its last line. The last
//! line counts a pull request that the cap stopped, too.
//!
//! The issue is the landed pull request's own, from its body (`gh pr view <n>`, read once, here), in the product's
//! sections: the body's `## Problem` is the Problem, and its whole `## How it works` is the Goal, the claim a reviewer
//! attacks. The fixture stores the text, so no trial needs the network. The Check runs the touched crates' tests, the
//! gate the mutants survived: a milestone's own blocks read the live designs repository, so at a past head they fail,
//! and the sandbox's `pr` would refuse.
//!
//! `--discard` is the planter's judgement that a fixture shows nothing a reviewer can prove, such as a mutant with no
//! observable effect: it deletes the fixture and appends `- <id>: <reason>` to the directory's `discarded.txt`.
//! `cargo-mutants` is a dev tool, not a dependency; without it, or at another version, the verb refuses with the
//! install line.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::eval::usage;
use crate::eval_fixture::INDENT;
use crate::eval_sandbox::capture;
use crate::mutants_walk::{CAP, added_lines, failing, order, walk};

const VERSION: &str = "27.1.0";
const INSTALL: &str = "cargo install --locked cargo-mutants@27.1.0";
const USAGE: &str = "usage: cargo xtask mutants --pr <n> [--pr <n>]... --count <k> --seed <s> [--out <dir>] | cargo xtask mutants --discard <id> --reason <text> [--out <dir>]";

struct Args {
    prs: Vec<u32>,
    count: usize,
    seed: u64,
    out: PathBuf,
    discard: Option<(String, String)>,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self> {
        let mut a =
            Self { prs: Vec::new(), count: 0, seed: 0, out: "xtask/evals/bp-review-mutants".into(), discard: None };
        let (mut seed, mut discard, mut reason) = (None, None, None);
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| usage(format!("{arg} takes a value")));
            match arg.as_str() {
                "--pr" => a.prs.push(value()?.trim_start_matches('#').parse().map_err(|_| usage("--pr is a number"))?),
                "--count" => a.count = value()?.parse().map_err(|_| usage("--count is a count"))?,
                "--seed" => seed = Some(value()?.parse().map_err(|_| usage("--seed is a number"))?),
                "--out" => a.out = value()?.into(),
                "--discard" => discard = Some(value()?.clone()),
                "--reason" => reason = Some(value()?.clone()),
                s => return Err(usage(format!("unknown argument {s}"))),
            }
        }
        match (seed, discard, reason) {
            (Some(s), None, None) if !a.prs.is_empty() && a.count > 0 => a.seed = s,
            (None, Some(id), Some(why)) if a.prs.is_empty() && a.count == 0 && !why.trim().is_empty() => {
                a.discard = Some((id, why));
            }
            _ => return Err(usage(USAGE)),
        }
        Ok(a)
    }
}

pub fn run(args: &[String]) -> Result<Vec<String>> {
    let a = Args::parse(args)?;
    if let Some((id, why)) = &a.discard {
        let file = a.out.join(format!("{id}.md"));
        std::fs::remove_file(&file).with_context(|| format!("no fixture {}", file.display()))?;
        let list = a.out.join("discarded.txt");
        let had = std::fs::read_to_string(&list).unwrap_or_default();
        std::fs::write(&list, format!("{had}- {id}: {}\n", why.trim()))?;
        return Ok(Vec::new());
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let version = Command::new(&cargo).args(["mutants", "--version"]).stdin(Stdio::null()).output();
    // The JSON fields read here, and the procedure the holdout seals, are this version's.
    if !version.is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains(VERSION)) {
        bail!("cargo-mutants {VERSION} is not installed: {INSTALL}");
    }
    let root = Path::new(".").canonicalize()?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-mutants"), 2)?;
    let code = work.join("code");
    capture(Command::new("git").args(["clone", "-q", "--no-hardlinks"]).arg(&root).arg(&code))?;
    std::fs::create_dir_all(&a.out)?;
    // Read once, before the first checkout detaches the clone at an older head.
    let log =
        capture(Command::new("git").args(["log", "--format=%H %s", "--first-parent", "HEAD"]).current_dir(&code))?;
    let (mut kept, mut skipped, mut capped) = (0, Vec::new(), Vec::new());
    for &pr in &a.prs {
        match one(&a, (&root, &code, &work), &cargo, &log, pr)? {
            End::Kept(k) => kept += k,
            End::Failing => skipped.push(format!("#{pr}")),
            End::Capped(k) => (kept, capped) = (kept + k, [capped, vec![format!("#{pr}")]].concat()),
        }
    }
    let listed = |prs: &[String]| if prs.is_empty() { String::new() } else { format!(" ({})", prs.join(", ")) };
    eprintln!(
        "mutants: {kept} kept from {} pull requests; {} skipped for failing tests{}; {} stopped at the cap{}",
        a.prs.len(),
        skipped.len(),
        listed(&skipped),
        capped.len(),
        listed(&capped)
    );
    // The clone and its target are gigabytes; on an error they stay for the logs, and the next run prunes them.
    std::fs::remove_dir_all(&work)?;
    Ok(Vec::new())
}

/// How one pull request ended.
enum End {
    /// The walk kept this many: `--count`, or fewer when the candidates ran out.
    Kept(usize),
    /// The touched crates' tests fail at the head.
    Failing,
    /// `CAP` candidates were tested before `--count` survived; this many were kept.
    Capped(usize),
}

/// The fixtures of one pull request: its candidates in seed order grouped by package, the walk to `--count` survivors
/// of CI, the files.
fn one(a: &Args, (root, code, work): (&Path, &Path, &Path), cargo: &OsStr, log: &str, pr: u32) -> Result<End> {
    let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(code));
    let (head, subject) = log
        .lines()
        .filter_map(|l| l.split_once(' '))
        .find(|(_, s)| s.ends_with(&format!("(#{pr})")))
        .with_context(|| format!("no squash on master ends (#{pr})"))?;
    let (head, subject) = (head.to_string(), subject.to_string());
    let base = git(&["rev-parse", &format!("{head}^")])?;
    let body = capture(
        Command::new("gh").args(["pr", "view", &pr.to_string(), "--json", "body", "-q", ".body"]).current_dir(root),
    )?;
    git(&["checkout", "-q", "--detach", &head])?;
    let diff = work.join(format!("{pr}.diff"));
    let changed = format!("{}\n", git(&["diff", &base, &head])?);
    std::fs::write(&diff, &changed)?;
    let added = added_lines(&changed);
    let started = Instant::now();
    let in_diff = diff.display().to_string();
    // A diff that touches no mutable code (tests only) lists nothing at all, not `[]`.
    let listed = mutants(cargo, code, &["--list", "--json", "--in-diff", &in_diff])?;
    let listed: Vec<Value> =
        serde_json::from_str(if listed.trim().is_empty() { "[]" } else { &listed }).context("parse --list --json")?;
    let crates: BTreeSet<String> = listed.iter().filter_map(|m| m["package"].as_str().map(str::to_string)).collect();
    let issue = issue_of(&body, &crates).with_context(|| format!("the body of #{pr}"))?;
    let on_added = |m: &&Value| {
        let line = m["span"]["start"]["line"].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
        added.contains(&(m["file"].as_str().unwrap_or("").to_string(), line))
    };
    let candidates =
        order(listed.iter().filter(|m| not_a_guard(m) && on_added(m)).collect(), a.seed.wrapping_add(pr.into()));
    eprintln!(
        "mutants: #{pr}: {} candidates in {crates:?}, {} after the guards and on added lines",
        listed.len(),
        candidates.len()
    );
    if !candidates.is_empty() {
        bins(cargo, code)?;
        let failing = failing(&crates, |cs| {
            let mut args = vec!["test", "-q"];
            args.extend(cs.iter().flat_map(|c| ["-p", c]));
            Command::new(cargo)
                .args(args)
                .current_dir(code)
                .stdin(Stdio::null())
                .output()
                .is_ok_and(|o| o.status.success())
        });
        if !failing.is_empty() {
            eprintln!("mutants: #{pr}: the tests of {} fail at {head}; #{pr} is skipped", failing.join(", "));
            return Ok(End::Failing);
        }
    }
    let mut chunks = Chunks {
        code,
        cargo,
        work,
        in_diff,
        tested: crates.iter().flat_map(|c| ["--test-package".to_string(), c.clone()]).collect(),
        pr,
        last: None,
        runs: 0,
    };
    let walked =
        walk(candidates, a.count, |m| ci_clean(code, cargo, &head, m).unwrap_or(false), |chunk| chunks.test(chunk))?;
    let kept = walked.kept.len();
    let chosen = walked.kept;
    eprintln!(
        "mutants: #{pr}: {} tested in {} chunks, {kept} kept, one per function ({} s)",
        walked.tested,
        chunks.runs,
        started.elapsed().as_secs()
    );
    let end = if walked.capped {
        eprintln!("mutants: #{pr}: cap of {CAP} tested candidates reached, {kept} kept");
        End::Capped(kept)
    } else {
        if kept < a.count {
            eprintln!("mutants: #{pr}: fewer than --count {}", a.count);
        }
        End::Kept(kept)
    };
    // A control pairs with the mutants of its pull request; alone it adds a run and measures nothing new.
    if chosen.is_empty() {
        return Ok(end);
    }

    let title = subject.split_once("): ").map_or(subject.as_str(), |s| s.1);
    let title = title.rsplit_once(" (#").map_or(title, |t| t.0);
    let shared = format!("base: {base}\nhead: {head}\ntitle: {}{}\n", title[..1].to_uppercase(), &title[1..]);
    std::fs::write(a.out.join(format!("control-{pr}.md")), format!("kind: control\n{shared}truth:\n---\n{issue}"))?;
    for (i, m) in chosen.into_iter().enumerate() {
        let planted = plant(code, &head, m, true)?;
        std::fs::write(
            a.out.join(format!("mutant-{pr}-{}.md", i + 1)),
            mutant_text(&shared, m, &planted, a.seed, &issue),
        )?;
    }
    Ok(end)
}

/// CI's `cargo test --workspace` builds every binary first, and some tests drive one (`xtask`'s eval tests need `bp`):
/// without them those tests pass as skipped, and mutants they would kill look missed.
fn bins(cargo: &OsStr, code: &Path) -> Result<String> {
    capture(Command::new(cargo).args(["build", "-q", "--workspace", "--bins"]).current_dir(code))
}

/// `cargo mutants` with `args` in the clone. It exits 2 when it missed a mutant and 3 on a timeout: both are results.
fn mutants(cargo: &OsStr, code: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new(cargo).arg("mutants").args(args).current_dir(code).stdin(Stdio::null()).output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !matches!(out.status.code(), Some(0 | 2 | 3)) {
        bail!(
            "cargo mutants {} exited {}: {}{stdout}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(stdout)
}

/// The tests of one pull request's chunks, at a head whose tests pass: one `cargo mutants` call per chunk.
struct Chunks<'a> {
    code: &'a Path,
    cargo: &'a OsStr,
    work: &'a Path,
    in_diff: String,
    /// `--test-package` for every crate the diff touches.
    tested: Vec<String>,
    pr: u32,
    /// The package of the last tested chunk, whose binaries hold a defect; `None` while they are clean.
    last: Option<String>,
    runs: usize,
}

impl Chunks<'_> {
    /// Whether each mutant of the chunk, all of one package, passes CI at its planted head: `MissedMutant`. It logs one
    /// line per mutant, with its build and test seconds.
    fn test(&mut self, chunk: &[&Value]) -> Result<Vec<bool>> {
        let pr = self.pr;
        let package = chunk[0]["package"].as_str().unwrap_or("").to_string();
        // A tested mutant leaves its package's binaries built with the defect: another package's tests would run them.
        if self.last.as_ref().is_some_and(|last| *last != package) {
            bins(self.cargo, self.code)?;
        }
        self.runs += 1;
        let names: Vec<&str> = chunk.iter().map(|m| m["name"].as_str().unwrap_or("")).collect();
        let out = self.work.join(format!("out-{pr}-{}", self.runs));
        let out_arg = out.display().to_string();
        let res: Vec<String> = names.iter().map(|n| format!("^{}$", escape(n))).collect();
        let mut args = vec!["--in-diff", &self.in_diff, "-p", &package, "--in-place", "--output", &out_arg];
        args.extend(self.tested.iter().map(String::as_str));
        args.extend(res.iter().flat_map(|r| ["--re", r.as_str()]));
        args.extend(["--baseline", "skip", "--timeout", "300"]);
        let at = Instant::now();
        // `--in-place` tests one mutant at a time in the throwaway clone, and builds in its own target.
        mutants(self.cargo, self.code, &args)?;
        self.last = Some(package.clone());
        let outcomes: Value = serde_json::from_str(&std::fs::read_to_string(out.join("mutants.out/outcomes.json"))?)?;
        let outcomes = outcomes["outcomes"].as_array().cloned().unwrap_or_default();
        let survived = names
            .iter()
            .map(|name| {
                let o = outcomes.iter().find(|o| o["scenario"]["Mutant"]["name"] == *name);
                let summary = o.and_then(|o| o["summary"].as_str()).unwrap_or("no outcome");
                let seconds: f64 = o
                    .and_then(|o| o["phase_results"].as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p["duration"].as_f64())
                    .sum();
                eprintln!("mutants: #{pr}: {name}: {summary} ({seconds:.0} s)");
                summary == "MissedMutant"
            })
            .collect();
        eprintln!("mutants: #{pr}: chunk {} of {} in {package} ({} s)", self.runs, chunk.len(), at.elapsed().as_secs());
        Ok(survived)
    }
}

/// Whether the head with the mutant in it passes `cargo fmt --all --check` and the workspace's clippy with warnings as
/// errors. A body replaced by `"xyzzy".into()` leaves unused code, and a deleted arm leaves a blank line: CI rejects
/// both, and a reader sees them in the diff.
fn ci_clean(code: &Path, cargo: &OsStr, head: &str, m: &Value) -> Result<bool> {
    if let Err(e) = plant(code, head, m, true) {
        eprintln!("mutants: {}: {e:#}", m["name"].as_str().unwrap_or(""));
        return Ok(false);
    }
    plant(code, head, m, false)?;
    let ok = |args: &[&str]| {
        Command::new(cargo).args(args).current_dir(code).stdin(Stdio::null()).output().is_ok_and(|o| o.status.success())
    };
    let clean = ok(&["fmt", "--all", "--check"])
        && ok(&["clippy", "-q", "--workspace", "--all-targets", "--", "-D", "warnings"]);
    capture(Command::new("git").args(["reset", "-q", "--hard", head]).current_dir(code))?;
    Ok(clean)
}

/// A name as a regex that matches only itself: `cargo mutants --re` takes regexes, and names hold `(`, `|` and `.`.
fn escape(name: &str) -> String {
    name.chars().fold(String::new(), |mut out, c| {
        if r"\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// The text of `mutant-<n>-<i>.md`: the pull request's lines, the mutant's keys, its diff indented under
/// `mutant_diff: |`, and one truth item at its span named in the tool's words (`replace > with >= in run`). The span is
/// in the planted head's lines, which the reviewer reads.
fn mutant_text(shared: &str, m: &Value, diff: &str, seed: u64, issue: &str) -> String {
    let (from, to) = planted_lines(diff);
    let span = format!("{}:{from}-{to}", m["file"].as_str().unwrap_or(""));
    let name = m["name"].as_str().unwrap_or("");
    // The name is `<file>:<line>:<column>: <what>`.
    let what = name.splitn(4, ':').nth(3).unwrap_or(name).trim();
    let indented: String = diff.lines().flat_map(|l| [INDENT, l, "\n"]).collect();
    format!(
        "kind: mutant\n{shared}genre: {}\nspan: {span}\nseed: {seed}\nmutant_diff: |\n{indented}truth:\n- {span} | {what}\n---\n{issue}",
        m["genre"].as_str().unwrap_or("")
    )
}

/// The first and last line of the planted head that a one-file diff changes: its `+` lines, or for a pure deletion the
/// line that follows the cut.
fn planted_lines(diff: &str) -> (usize, usize) {
    let (mut at, mut lines) = (0, Vec::new());
    let mut cut = None;
    for l in diff.lines() {
        if let Some(hunk) = l.strip_prefix("@@ ") {
            let new = hunk.split_whitespace().find_map(|w| w.strip_prefix('+')).unwrap_or("1");
            at = new.split(',').next().and_then(|n| n.parse().ok()).unwrap_or(1);
        } else if at == 0 || l.starts_with('\\') {
            // The file header, before the first hunk, or `\ No newline at end of file`.
        } else if l.starts_with('+') {
            lines.push(at);
            at += 1;
        } else if l.starts_with('-') {
            cut.get_or_insert(at);
        } else {
            at += 1;
        }
    }
    match (lines.first(), lines.last()) {
        (Some(&a), Some(&b)) => (a, b),
        _ => (cut.unwrap_or(1), cut.unwrap_or(1)),
    }
}

/// Whether the mutant is not a guard mutant (`MatchArmGuard`). A guard mutant writes a literal `true` or `false` into
/// the guard, which any reader sees (30, design change 1).
fn not_a_guard(m: &Value) -> bool {
    m["genre"] != "MatchArmGuard"
}

/// Writes the mutant into the clone at `head`: the span's text becomes the replacement as `cargo-mutants` writes it,
/// without the tool's marker comment, which would point at the defect. With `commit`, it commits it as `bp-eval`,
/// resets the clone to `head`, and returns the diff as git wrote it once `git apply --check` takes it there.
fn plant(code: &Path, head: &str, m: &Value, commit: bool) -> Result<String> {
    let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(code));
    git(&["reset", "-q", "--hard", head])?;
    let path = code.join(m["file"].as_str().context("a mutant names its file")?);
    let text = std::fs::read_to_string(&path)?;
    let n = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(1);
    let at = |p: &Value| offset(&text, n(&p["line"]), n(&p["column"]));
    let (start, end) = (at(&m["span"]["start"]), at(&m["span"]["end"]));
    let replacement = m["replacement"].as_str().unwrap_or("");
    std::fs::write(&path, format!("{}{replacement}{}", &text[..start], &text[end..]))?;
    if !commit {
        return Ok(String::new());
    }
    let me = ["-c", "user.name=bp-eval", "-c", "user.email=bp-eval@bp.invalid"];
    git(&[&me[..], &["commit", "-q", "--no-verify", "-am", "mutant"]].concat())?;
    // `capture` trims, which drops a blank last context line: the diff is read as git wrote it.
    let out = Command::new("git").args(["diff", head, "HEAD"]).current_dir(code).output()?;
    if !out.status.success() {
        bail!("git diff {head} HEAD exited {}", out.status);
    }
    let diff = String::from_utf8(out.stdout).context("the diff is UTF-8")?;
    git(&["reset", "-q", "--hard", head])?;
    let mut check = Command::new("git")
        .args(["apply", "--check", "-"])
        .current_dir(code)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    std::io::Write::write_all(&mut check.stdin.take().context("git apply takes stdin")?, diff.as_bytes())?;
    let checked = check.wait_with_output()?;
    if !checked.status.success() {
        bail!("the planted diff does not apply at {head}: {}", String::from_utf8_lossy(&checked.stderr).trim());
    }
    Ok(diff)
}

/// The byte offset of a 1-based line and character column, as `cargo-mutants` gives a span.
fn offset(text: &str, line: usize, column: usize) -> usize {
    let start: usize = text.split_inclusive('\n').take(line - 1).map(str::len).sum();
    let rest = &text[start..];
    start + rest.char_indices().nth(column - 1).map_or(rest.len(), |c| c.0)
}

/// The seeded issue in the product's sections. Problem and Goal come from a landed pull request's body: its
/// `## Problem`, and its whole `## How it works`, so the goal-tester has the full claim to attack. The template
/// (`.github/pull_request_template.md`) names these sections. The Check runs the touched crates' tests.
fn issue_of(body: &str, crates: &BTreeSet<String>) -> Result<String> {
    let section = |heading: &str| {
        let lines: Vec<&str> =
            body.lines().skip_while(|l| l.trim() != heading).skip(1).take_while(|l| !l.starts_with("## ")).collect();
        Some(lines.join("\n").trim().to_string()).filter(|s| !s.is_empty())
    };
    let problem = section("## Problem").context("no ## Problem")?;
    let goal = section("## How it works").context("no ## How it works")?;
    let tests: Vec<String> = crates.iter().map(|c| format!("-p {c}")).collect();
    Ok(format!(
        "## Problem\n\n{problem}\n\n## Goal\n\n{}\n\n## Check\n\n~~~check shell {{name=tests expect=0}}\ncargo test -q {}\n~~~\n",
        goal,
        tests.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval_fixture::Fixture;

    #[test]
    fn a_mutant_fixture_round_trips_through_the_parser() {
        let m: Value = serde_json::from_str(
            r#"{"name": "xtask/src/a.rs:22:18: replace > with >= in run", "file": "xtask/src/a.rs",
                "genre": "BinaryOperator", "span": {"start": {"line": 22, "column": 18}, "end": {"line": 22, "column": 19}}}"#,
        )
        .unwrap();
        let diff = "diff --git a/xtask/src/a.rs b/xtask/src/a.rs\n--- a/xtask/src/a.rs\n+++ b/xtask/src/a.rs\n@@ -21,3 +21,3 @@\n \n-    if lines > CAP {\n+    if lines >= CAP {\n---\n- x\n";
        let issue =
            "## Problem\n\nP\n\n## Check\n\n~~~check shell {name=tests expect=0}\ncargo test -q -p xtask\n~~~\n";
        let text = mutant_text("base: b1\nhead: h1\ntitle: T\n", &m, diff, 7, issue);
        let f = Fixture::parse("mutant-9-1", &text).unwrap();
        assert_eq!((f.kind.as_deref(), f.base.as_str(), f.head.as_str()), (Some("mutant"), "b1", "h1"));
        let got = f.mutant.unwrap();
        assert_eq!((got.genre.as_str(), got.span.as_str(), got.seed), ("BinaryOperator", "xtask/src/a.rs:22-22", 7));
        assert_eq!(got.diff, diff, "the diff's `---` and `- ` lines stay in the diff");
        assert_eq!(f.truth.len(), 1);
        assert_eq!(
            (f.truth[0].location(), f.truth[0].what.as_str()),
            ("xtask/src/a.rs:22".into(), "replace > with >= in run")
        );
        assert_eq!(f.issue, issue.trim());

        let known = "known: xtask/src/a.rs:96 | A quoted ; still splits.\n";
        let control = Fixture::parse(
            "control-9",
            &format!("kind: control\nbase: b\nhead: h\ntitle: T\n{known}truth:\n---\n{issue}"),
        )
        .unwrap();
        assert!(control.truth.is_empty() && control.mutant.is_none(), "a control has no truth");
        assert_eq!(
            control.known.iter().map(|k| (k.location(), k.what.as_str())).collect::<Vec<_>>(),
            [("xtask/src/a.rs:96".into(), "A quoted ; still splits.")]
        );
        for bad in [
            text.replace("truth:\n", &format!("{known}truth:\n")),
            text.replace("seed: 7\n", ""),
            text.replace("kind: mutant", "kind: other"),
            text.replace("kind: mutant\n", ""),
        ] {
            assert!(Fixture::parse("x", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_fixture_set_loads_from_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let m: Value = serde_json::from_str(
            r#"{"name": "a.rs:1:1: delete ! in f", "file": "a.rs", "genre": "UnaryOperator", "span": {"start": {"line": 1, "column": 1}, "end": {"line": 1, "column": 2}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("mutant-1-1.md"),
            mutant_text("base: b\nhead: h\ntitle: T\n", &m, "+x\n", 3, "body"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("control-1.md"),
            "kind: control\nbase: b\nhead: h\ntitle: T\ntruth:\n---\nbody\n",
        )
        .unwrap();
        let all = Fixture::load(dir.path(), None).unwrap();
        assert_eq!(all.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["control-1", "mutant-1-1"]);
        assert_eq!(all[1].mutant.as_ref().map(|m| (m.genre.as_str(), m.seed)), Some(("UnaryOperator", 3)));
    }

    #[test]
    fn a_guard_mutant_is_dropped() {
        let listed: Vec<Value> =
            serde_json::from_str(r#"[{"name":"a","genre":"MatchArmGuard"},{"name":"b","genre":"BinaryOperator"}]"#)
                .unwrap();
        let kept: Vec<&str> = listed.iter().filter(|m| not_a_guard(m)).filter_map(|m| m["name"].as_str()).collect();
        assert_eq!(kept, ["b"]);
    }

    #[test]
    fn a_span_is_replaced_by_character_column() {
        let text = "fn a() {\n    let s = \"é\"; s > t\n}\n";
        let (start, end) = (offset(text, 2, 20), offset(text, 2, 21));
        assert_eq!(&text[start..end], ">");
        assert_eq!(offset(text, 3, 2), text.len() - 1);
    }

    #[test]
    fn the_span_is_in_the_planted_heads_lines() {
        let body = "diff --git a/s.rs b/s.rs\n--- a/s.rs\n+++ b/s.rs\n@@ -26,20 +26,3 @@ x\n fn run() {\n-    a();\n-    b();\n+    Ok(String::new())\n }\n";
        assert_eq!(planted_lines(body), (27, 27), "a body replaced over many lines is one line in the planted head");
        let cut =
            "--- a/m.rs\n+++ b/m.rs\n@@ -54,4 +54,3 @@\n     match x {\n-        Some(1) => {}\n         _ => {}\n";
        assert_eq!(planted_lines(cut), (55, 55), "a deleted arm: the line after the cut");
        let dashes = "--- a/m.rs\n+++ b/m.rs\n@@ -3,2 +3,2 @@\n a\n---- b\n+--- c\n";
        assert_eq!(planted_lines(dashes), (4, 4), "a removed line that starts with `---` is no header");
    }

    #[test]
    fn the_planted_diff_keeps_a_blank_last_context_line_and_applies() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(dir.path())).unwrap();
        git(&["init", "-q"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {\n    x == y\n}\n\n").unwrap();
        git(&["add", "a.rs"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t.invalid", "commit", "-q", "-m", "a"]);
        let head = git(&["rev-parse", "HEAD"]);
        let m: Value = serde_json::from_str(
            r#"{"file": "a.rs", "replacement": "!=", "span": {"start": {"line": 2, "column": 7}, "end": {"line": 2, "column": 9}}}"#,
        )
        .unwrap();
        let diff = plant(dir.path(), &head, &m, true).unwrap();
        assert!(diff.ends_with("+    x != y\n }\n \n"), "the hunk keeps its blank context line: {diff:?}");
        assert_eq!(git(&["rev-parse", "HEAD"]), head, "the clone is back at the head");
    }

    #[test]
    fn the_issue_comes_from_the_landed_pull_requests_body() {
        let body = "## Problem\n\nThe reader counts calls.\n\n## Why now\n\nx\n\n## What changed\n\n| Before | After |\n|---|---|\n| a | b |\n\n## How it works\n\nIt splits.\n\nThen it counts.\n";
        let crates = BTreeSet::from(["bp-core".to_string(), "xtask".to_string()]);
        let check = "## Check\n\n~~~check shell {name=tests expect=0}\ncargo test -q -p bp-core -p xtask\n~~~\n";
        assert_eq!(
            issue_of(body, &crates).unwrap(),
            format!("## Problem\n\nThe reader counts calls.\n\n## Goal\n\nIt splits.\n\nThen it counts.\n\n{check}"),
            "the Goal is the whole How it works, not the What changed table"
        );
        assert!(issue_of("## Summary\n\nx\n", &crates).is_err(), "a body with no Problem refuses");
        assert!(issue_of("## Problem\n\nx\n", &crates).is_err(), "a body with no How it works refuses");
    }

    #[test]
    fn the_seeded_issue_has_problem_goal_and_check() {
        let m: Value = serde_json::from_str(
            r#"{"name": "a.rs:1:1: delete ! in f", "file": "a.rs", "genre": "UnaryOperator", "span": {"start": {"line": 1, "column": 1}, "end": {"line": 1, "column": 2}}}"#,
        )
        .unwrap();
        let issue =
            issue_of("## Problem\n\nP.\n\n## How it works\n\nG.\n", &BTreeSet::from(["a".to_string()])).unwrap();
        let f =
            Fixture::parse("mutant-1-1", &mutant_text("base: b\nhead: h\ntitle: T\n", &m, "+x\n", 3, &issue)).unwrap();
        let headings: Vec<&str> = f.issue.lines().filter(|l| l.starts_with("## ")).collect();
        assert_eq!(headings, ["## Problem", "## Goal", "## Check"]);
    }

    #[test]
    fn a_name_matches_itself_as_a_regex() {
        assert_eq!(escape("a.rs:1:2: replace || with && in f"), r"a\.rs:1:2: replace \|\| with \&\& in f");
        assert_eq!(
            escape("replace f -> Vec<(String, u32)> with vec![]"),
            r"replace f \-> Vec<\(String, u32\)> with vec!\[\]"
        );
    }

    #[test]
    fn discard_removes_a_fixture_and_records_why() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mutant-1-1.md"), "x").unwrap();
        let out = dir.path().display().to_string();
        let args = |id: &str| ["--discard", id, "--reason", "no observable effect", "--out", &out].map(String::from);
        run(&args("mutant-1-1")).unwrap();
        assert!(!dir.path().join("mutant-1-1.md").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("discarded.txt")).unwrap(),
            "- mutant-1-1: no observable effect\n"
        );
        assert!(run(&args("mutant-1-1")).is_err(), "a fixture that is not there");
        let bad = ["--discard", "x", "--pr", "1", "--reason", "r"].map(String::from);
        assert!(Args::parse(&bad).is_err(), "discard takes no --pr");
    }
}
