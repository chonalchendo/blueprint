//! `cargo xtask mutants`, the planted-defect fixtures of the review eval (30, design change 1).
//!
//! ```text
//! cargo xtask mutants --pr <n> [--pr <n>]... --count <k> --seed <s> [--out <dir>]
//! cargo xtask mutants --discard <id> --reason <text> [--out <dir>]
//! ```
//!
//! For each landed pull request it finds the squash on master whose subject ends `(#<n>)`; the base is its parent.
//! In a throwaway clone at that head it lists `cargo-mutants`' candidates on the lines the diff changed. A mutant that
//! CI catches is a CI finding, not a review finding, so a survivor passes all of CI at the planted head: first
//! `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`, the cheap gates, then the
//! tests of every crate the diff touches, one mutant at a time. A guard mutant (`MatchArmGuard`) is dropped before
//! that: the tool writes a literal `true` or `false`, which any reader sees. It shuffles the survivors with a seed of
//! `s` and the pull request, takes at most one per function, and keeps `k`. It commits each on the head as `bp-eval`
//! and records the diff. It writes `mutant-<n>-<i>.md` for each and `control-<n>.md` with no mutant
//! (`eval_fixture.rs` has the shape), into `--out` (default `xtask/evals/bp-review-mutants`).
//!
//! The issue is the landed pull request's own, from its body (`gh pr view <n>`, read once, here): its `## Problem`,
//! and its `## What changed` table as the Shape, or `## How it works` when that has no table. The fixture stores the
//! text, so no trial needs the network. The Check runs the touched crates' tests, the gate the mutants survived: a
//! milestone's own blocks read the live designs repository, so at a past head they fail, and the sandbox's `pr` would
//! refuse.
//!
//! `--discard` is the planter's judgement that a fixture shows nothing a reviewer can prove, such as a mutant with no
//! observable effect: it deletes the fixture and appends `- <id>: <reason>` to the directory's `discarded.txt`.
//! `cargo-mutants` is a dev tool, not a dependency; without it, or at another version, the verb refuses with the
//! install line.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::eval::usage;
use crate::eval_fixture::INDENT;
use crate::eval_sandbox::capture;

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
    for &pr in &a.prs {
        one(&a, (&root, &code, &work), &cargo, &log, pr)?;
    }
    // The clone and its target are gigabytes; on an error they stay for the logs, and the next run prunes them.
    std::fs::remove_dir_all(&work)?;
    Ok(Vec::new())
}

/// The fixtures of one pull request: its candidates, the survivors of CI, the pick and the files.
fn one(a: &Args, (root, code, work): (&Path, &Path, &Path), cargo: &std::ffi::OsStr, log: &str, pr: u32) -> Result<()> {
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
    let issue = issue_of(&body).with_context(|| format!("the body of #{pr}"))?;
    git(&["checkout", "-q", "--detach", &head])?;
    let diff = work.join(format!("{pr}.diff"));
    std::fs::write(&diff, format!("{}\n", git(&["diff", &base, &head])?))?;
    let started = Instant::now();

    // `cargo mutants` exits 2 when it missed a mutant and 3 on a timeout: both are results here.
    let mutants = |args: &[&str]| -> Result<String> {
        let shown = format!("cargo mutants {}", args.join(" "));
        let started = Instant::now();
        let out = Command::new(cargo).arg("mutants").args(args).current_dir(code).stdin(Stdio::null()).output()?;
        // A run names every clean candidate by `--re`; the log keeps the start.
        let shown: String =
            if shown.len() > 300 { shown.chars().take(300).chain("...".chars()).collect() } else { shown };
        eprintln!("mutants: #{pr}: {shown} ({} s)", started.elapsed().as_secs());
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !matches!(out.status.code(), Some(0 | 2 | 3)) {
            bail!("{shown} exited {}: {}{stdout}", out.status, String::from_utf8_lossy(&out.stderr));
        }
        Ok(stdout)
    };
    let in_diff = diff.display().to_string();
    // A diff that touches no mutable code (tests only) lists nothing at all, not `[]`.
    let listed = mutants(&["--list", "--json", "--in-diff", &in_diff])?;
    let listed: Vec<Value> =
        serde_json::from_str(if listed.trim().is_empty() { "[]" } else { &listed }).context("parse --list --json")?;
    let crates: BTreeSet<String> = listed.iter().filter_map(|m| m["package"].as_str().map(str::to_string)).collect();
    let clean: Vec<&Value> =
        listed.iter().filter(|m| not_a_guard(m) && ci_clean(code, cargo, &head, m).unwrap_or(false)).collect();
    eprintln!(
        "mutants: #{pr}: {} of {} candidates pass fmt and clippy ({} s)",
        clean.len(),
        listed.len(),
        started.elapsed().as_secs()
    );
    let tested: Vec<String> = crates.iter().flat_map(|c| ["--test-package".to_string(), c.clone()]).collect();
    let mut missed = BTreeSet::new();
    for krate in &crates {
        let names: Vec<&str> =
            clean.iter().filter(|m| m["package"] == krate.as_str()).filter_map(|m| m["name"].as_str()).collect();
        if names.is_empty() {
            continue;
        }
        // CI's `cargo test --workspace` builds every binary first, and some tests drive one (`xtask`'s eval tests
        // need `bp`): without them those tests pass as skipped, and mutants they would kill look missed. It is
        // rebuilt per crate, because the last mutant of the crate before leaves its binary built with the defect.
        capture(Command::new(cargo).args(["build", "-q", "--workspace", "--bins"]).current_dir(code))?;
        let out = work.join(format!("out-{pr}-{krate}"));
        let out_arg = out.display().to_string();
        let mut args = vec!["--in-diff", &in_diff, "-p", krate, "--in-place", "--output", &out_arg];
        args.extend(tested.iter().map(String::as_str));
        let res: Vec<String> = names.iter().map(|n| format!("^{}$", escape(n))).collect();
        args.extend(res.iter().flat_map(|r| ["--re", r.as_str()]));
        // `--in-place` runs one mutant at a time in the throwaway clone, and builds in its own target.
        mutants(&args)?;
        let outcomes: Value = serde_json::from_str(&std::fs::read_to_string(out.join("mutants.out/outcomes.json"))?)?;
        for o in outcomes["outcomes"].as_array().into_iter().flatten() {
            if o["summary"] == "MissedMutant" {
                missed.extend(o["scenario"]["Mutant"]["name"].as_str().map(str::to_string));
            }
        }
    }
    let survivors: Vec<&Value> =
        clean.into_iter().filter(|m| missed.contains(m["name"].as_str().unwrap_or(""))).collect();
    let chosen = pick(survivors.clone(), a.count, a.seed.wrapping_add(pr.into()));
    eprintln!(
        "mutants: #{pr}: {} candidates in {crates:?}, {} survive CI, {} kept, one per function ({} s)",
        listed.len(),
        survivors.len(),
        chosen.len(),
        started.elapsed().as_secs()
    );
    if chosen.len() < a.count {
        eprintln!("mutants: #{pr}: fewer than --count {}", a.count);
    }
    // A control pairs with the mutants of its pull request; alone it adds a run and measures nothing new.
    if chosen.is_empty() {
        return Ok(());
    }

    let tests: Vec<String> = crates.iter().map(|c| format!("-p {c}")).collect();
    let issue = format!(
        "{issue}\n\n## Check\n\n~~~check shell {{name=tests expect=0}}\ncargo test -q {}\n~~~\n",
        tests.join(" ")
    );
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
    Ok(())
}

/// Whether the head with the mutant in it passes `cargo fmt --all --check` and the workspace's clippy with warnings as
/// errors. A body replaced by `"xyzzy".into()` leaves unused code, and a deleted arm leaves a blank line: CI rejects
/// both, and a reader sees them in the diff.
fn ci_clean(code: &Path, cargo: &std::ffi::OsStr, head: &str, m: &Value) -> Result<bool> {
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

/// `count` survivors in an order shuffled by `seed`, at most one from each function: two mutants of one function are
/// not two independent pieces of evidence.
fn pick(mut survivors: Vec<&Value>, count: usize, seed: u64) -> Vec<&Value> {
    let mut state = seed;
    survivors.sort_by_key(|m| m["name"].as_str().unwrap_or(""));
    for i in (1..survivors.len()).rev() {
        survivors.swap(i, usize::try_from(splitmix(&mut state) % (i as u64 + 1)).expect("at most i"));
    }
    let mut functions = BTreeSet::new();
    let mut chosen = Vec::new();
    for m in survivors {
        let function = (m["file"].as_str(), m["function"]["function_name"].as_str());
        if chosen.len() < count && functions.insert(function) {
            chosen.push(m);
        }
    }
    chosen
}

/// `SplitMix64`: a seeded sequence with no dependency, enough for a reproducible shuffle.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Writes the mutant into the clone at `head`: the span's text becomes the replacement as `cargo-mutants` writes it,
/// without the tool's marker comment, which would point at the defect. With `commit`, it commits it as `bp-eval`,
/// returns the diff and resets the clone to `head`.
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
    let diff = git(&["diff", head, "HEAD"])?;
    git(&["reset", "-q", "--hard", head])?;
    Ok(format!("{diff}\n"))
}

/// The byte offset of a 1-based line and character column, as `cargo-mutants` gives a span.
fn offset(text: &str, line: usize, column: usize) -> usize {
    let start: usize = text.split_inclusive('\n').take(line - 1).map(str::len).sum();
    let rest = &text[start..];
    start + rest.char_indices().nth(column - 1).map_or(rest.len(), |c| c.0)
}

/// The Problem and Shape of a landed pull request's body: `## Problem`, and `## What changed` when it holds a table,
/// else `## How it works`. The template (`.github/pull_request_template.md`) names these sections.
fn issue_of(body: &str) -> Result<String> {
    let section = |heading: &str| {
        let lines: Vec<&str> =
            body.lines().skip_while(|l| l.trim() != heading).skip(1).take_while(|l| !l.starts_with("## ")).collect();
        Some(lines.join("\n").trim().to_string()).filter(|s| !s.is_empty())
    };
    let problem = section("## Problem").context("no ## Problem")?;
    let shape = section("## What changed")
        .filter(|s| s.lines().any(|l| l.trim_start().starts_with('|')))
        .or_else(|| section("## How it works"))
        .context("no ## What changed table and no ## How it works")?;
    Ok(format!("## Problem\n\n{problem}\n\n## Shape\n\n{shape}"))
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
    fn the_pick_is_seeded_and_takes_one_mutant_per_function() {
        let all: Vec<Value> = (0..9)
            .map(|i| serde_json::json!({"name": format!("m{i}"), "file": "a.rs", "function": {"function_name": format!("f{}", i % 3)}}))
            .collect();
        let names =
            |seed| pick(all.iter().collect(), 2, seed).iter().map(|m| m["name"].to_string()).collect::<Vec<_>>();
        assert_eq!(names(7), names(7), "one seed, one pick");
        assert_ne!((0..20).map(names).collect::<BTreeSet<_>>().len(), 1, "the seed moves the pick");
        let three = pick(all.iter().collect(), 9, 7);
        let functions: BTreeSet<_> = three.iter().map(|m| m["function"]["function_name"].as_str()).collect();
        assert_eq!((three.len(), functions.len()), (3, 3), "one per function, however many survive");
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
    fn the_issue_comes_from_the_landed_pull_requests_body() {
        let body = "## Problem\n\nThe reader counts calls.\n\n## Why now\n\nx\n\n## What changed\n\n| Before | After |\n|---|---|\n| a | b |\n\n## How it works\n\nIt splits.\n";
        assert_eq!(
            issue_of(body).unwrap(),
            "## Problem\n\nThe reader counts calls.\n\n## Shape\n\n| Before | After |\n|---|---|\n| a | b |"
        );
        let no_table = body.replace("| Before | After |\n|---|---|\n| a | b |", "None.");
        assert!(issue_of(&no_table).unwrap().ends_with("## Shape\n\nIt splits."), "no table: How it works");
        assert!(issue_of("## Summary\n\nx\n").is_err(), "a body with no Problem refuses");
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
