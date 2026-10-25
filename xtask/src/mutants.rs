//! `cargo xtask mutants`, the planted-defect fixtures of the review eval (30, design change 1).
//!
//! ```text
//! cargo xtask mutants --pr <n> [--pr <n>]... --count <k> --seed <s> [--out <dir>]
//! ```
//!
//! For each landed pull request it finds the squash on master whose subject ends `(#<n>)`; the base is its parent.
//! In a throwaway clone at that head it lists `cargo-mutants`' candidates on the lines the diff changed, then runs
//! each touched crate's tests against them, one mutant at a time. A mutant the tests kill is a CI finding, not a review
//! finding, so only the missed ones are kept. It picks `k` of them with a shuffle seeded by `s` and the pull request,
//! one genre at a time so the genres spread, commits each on the head as `bp-eval` and records the diff. It writes
//! `mutant-<n>-<i>.md` for each and `control-<n>.md` with no mutant (`eval_fixture.rs` has the shape), into `--out`
//! (default `xtask/evals/bp-review-mutants`).
//!
//! The issue is the one the pull request closed, read from its milestone's design under `$BP_REDESIGN`: the subject's
//! `(m<m>-<i>)` names it. The Problem is the issue's note, the Shape the design changes its row names. The Check runs
//! the touched crates' tests, the gate the mutants survived: a milestone's own blocks read the live designs
//! repository, so at a past head they fail, and the sandbox's `pr` would refuse. `cargo-mutants` is a dev tool, not a
//! dependency; without it the verb refuses with the install line.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::eval::usage;
use crate::eval_fixture::INDENT;
use crate::eval_sandbox::capture;

const INSTALL: &str = "cargo install --locked cargo-mutants@27.1.0";

struct Args {
    prs: Vec<u32>,
    count: usize,
    seed: u64,
    out: PathBuf,
}

impl Args {
    fn parse(args: &[String]) -> Result<Self> {
        let mut a = Self { prs: Vec::new(), count: 0, seed: 0, out: "xtask/evals/bp-review-mutants".into() };
        let mut seed = None;
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = || it.next().ok_or_else(|| usage(format!("{arg} takes a value")));
            match arg.as_str() {
                "--pr" => a.prs.push(value()?.trim_start_matches('#').parse().map_err(|_| usage("--pr is a number"))?),
                "--count" => a.count = value()?.parse().map_err(|_| usage("--count is a count"))?,
                "--seed" => seed = Some(value()?.parse().map_err(|_| usage("--seed is a number"))?),
                "--out" => a.out = value()?.into(),
                s => return Err(usage(format!("unknown argument {s}"))),
            }
        }
        match seed {
            Some(s) if !a.prs.is_empty() && a.count > 0 => a.seed = s,
            _ => {
                return Err(usage(
                    "usage: cargo xtask mutants --pr <n> [--pr <n>]... --count <k> --seed <s> [--out <dir>]",
                ));
            }
        }
        Ok(a)
    }
}

pub fn run(args: &[String]) -> Result<Vec<String>> {
    let a = Args::parse(args)?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let version = Command::new(&cargo).args(["mutants", "--version"]).stdin(Stdio::null()).output();
    if !version.is_ok_and(|o| o.status.success()) {
        bail!("cargo-mutants is not installed: {INSTALL}");
    }
    let redesign = std::env::var_os("BP_REDESIGN").context("set BP_REDESIGN to the designs repository")?;
    let root = Path::new(".").canonicalize()?;
    let work = crate::scratch::fresh(&std::env::temp_dir().join("bp-mutants"), 2)?;
    let code = work.join("code");
    capture(Command::new("git").args(["clone", "-q", "--no-hardlinks"]).arg(&root).arg(&code))?;
    std::fs::create_dir_all(&a.out)?;
    let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(&code));
    // Read once, before the first checkout detaches the clone at an older head.
    let log = git(&["log", "--format=%H %s", "--first-parent", "HEAD"])?;
    for &pr in &a.prs {
        let (head, subject) = log
            .lines()
            .filter_map(|l| l.split_once(' '))
            .find(|(_, s)| s.ends_with(&format!("(#{pr})")))
            .with_context(|| format!("no squash on master ends (#{pr})"))?;
        let (head, subject) = (head.to_string(), subject.to_string());
        let base = git(&["rev-parse", &format!("{head}^")])?;
        let (title, issue) = issue_of(Path::new(&redesign), &subject)?;
        git(&["checkout", "-q", "--detach", &head])?;
        let diff = work.join(format!("{pr}.diff"));
        std::fs::write(&diff, format!("{}\n", git(&["diff", &base, &head])?))?;

        // `cargo mutants` exits 2 when it missed a mutant and 3 on a timeout: both are results here.
        let mutants = |args: &[&str]| -> Result<String> {
            let shown = format!("cargo mutants {}", args.join(" "));
            let started = Instant::now();
            let out =
                Command::new(&cargo).arg("mutants").args(args).current_dir(&code).stdin(Stdio::null()).output()?;
            eprintln!("mutants: #{pr}: {shown} ({} s)", started.elapsed().as_secs());
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            if !matches!(out.status.code(), Some(0 | 2 | 3)) {
                bail!("{shown} exited {}: {}{stdout}", out.status, String::from_utf8_lossy(&out.stderr));
            }
            Ok(stdout)
        };
        let in_diff = diff.display().to_string();
        let listed: Vec<Value> = serde_json::from_str(&mutants(&["--list", "--json", "--in-diff", &in_diff])?)
            .context("parse --list --json")?;
        let crates: BTreeSet<String> =
            listed.iter().filter_map(|m| m["package"].as_str().map(str::to_string)).collect();
        let mut missed = BTreeSet::new();
        for krate in &crates {
            // CI's `cargo test --workspace` builds every binary first, and some tests drive one (`xtask`'s eval tests
            // need `bp`): without them those tests pass as skipped, and mutants they would kill look missed. It is
            // rebuilt per crate, because the last mutant of the crate before leaves its binary built with the defect.
            capture(Command::new(&cargo).args(["build", "-q", "--workspace", "--bins"]).current_dir(&code))?;
            let out = work.join(format!("out-{pr}-{krate}"));
            let out_arg = out.display().to_string();
            // `--in-place` runs one mutant at a time in the throwaway clone, and builds in its own target.
            mutants(&["--in-diff", &in_diff, "-p", krate, "--in-place", "--output", &out_arg])?;
            let outcomes: Value =
                serde_json::from_str(&std::fs::read_to_string(out.join("mutants.out/outcomes.json"))?)?;
            for o in outcomes["outcomes"].as_array().into_iter().flatten() {
                if o["summary"] == "MissedMutant" {
                    missed.extend(o["scenario"]["Mutant"]["name"].as_str().map(str::to_string));
                }
            }
        }
        let survivors: Vec<&Value> =
            listed.iter().filter(|m| missed.contains(m["name"].as_str().unwrap_or(""))).collect();
        eprintln!("mutants: #{pr}: {} candidates in {crates:?}, {} survive the tests", listed.len(), survivors.len());
        if survivors.len() < a.count {
            eprintln!("mutants: #{pr}: fewer survivors than --count {}", a.count);
        }

        let tests: Vec<String> = crates.iter().map(|c| format!("-p {c}")).collect();
        let issue = format!(
            "{issue}\n\n## Check\n\n~~~check shell {{name=tests expect=0}}\ncargo test -q {}\n~~~\n",
            tests.join(" ")
        );
        let shared = format!("base: {base}\nhead: {head}\ntitle: {title}\n");
        std::fs::write(a.out.join(format!("control-{pr}.md")), format!("kind: control\n{shared}truth:\n---\n{issue}"))?;
        for (i, m) in pick(survivors, a.count, a.seed.wrapping_add(pr.into())).into_iter().enumerate() {
            let planted = plant(&code, &head, m)?;
            std::fs::write(
                a.out.join(format!("mutant-{pr}-{}.md", i + 1)),
                mutant_text(&shared, m, &planted, a.seed, &issue),
            )?;
        }
    }
    // The clone and its target are gigabytes; on an error they stay for the logs, and the next run prunes them.
    std::fs::remove_dir_all(&work)?;
    Ok(Vec::new())
}

/// The text of `mutant-<n>-<i>.md`: the pull request's lines, the mutant's keys, its diff indented under
/// `mutant_diff: |`, and one truth item at its span named in the tool's words (`replace > with >= in run`).
fn mutant_text(shared: &str, m: &Value, diff: &str, seed: u64, issue: &str) -> String {
    let (file, from, to) = (m["file"].as_str().unwrap_or(""), &m["span"]["start"]["line"], &m["span"]["end"]["line"]);
    let span = format!("{file}:{from}-{to}");
    let name = m["name"].as_str().unwrap_or("");
    // The name is `<file>:<line>:<column>: <what>`.
    let what = name.splitn(4, ':').nth(3).unwrap_or(name).trim();
    let indented: String = diff.lines().flat_map(|l| [INDENT, l, "\n"]).collect();
    format!(
        "kind: mutant\n{shared}genre: {}\nspan: {span}\nseed: {seed}\nmutant_diff: |\n{indented}truth:\n- {span} | {what}\n---\n{issue}",
        m["genre"].as_str().unwrap_or("")
    )
}

/// `count` survivors, shuffled by `seed` within each genre and then taken one genre at a time, so a genre with many
/// mutants cannot fill the set while another has some.
fn pick(survivors: Vec<&Value>, count: usize, seed: u64) -> Vec<&Value> {
    let mut state = seed;
    let mut genres: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for m in survivors {
        genres.entry(m["genre"].as_str().unwrap_or("")).or_default().push(m);
    }
    for list in genres.values_mut() {
        list.sort_by_key(|m| m["name"].as_str().unwrap_or(""));
        for i in (1..list.len()).rev() {
            list.swap(i, usize::try_from(splitmix(&mut state) % (i as u64 + 1)).expect("at most i"));
        }
    }
    let mut chosen = Vec::new();
    while chosen.len() < count && genres.values().any(|l| !l.is_empty()) {
        for list in genres.values_mut() {
            if chosen.len() < count {
                chosen.extend(list.pop());
            }
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

/// Commits the mutant on `head` in the clone, as `bp-eval`, and returns its diff. The span's text becomes the
/// replacement as `cargo-mutants` writes it, without the tool's marker comment, which would point at the defect.
fn plant(code: &Path, head: &str, m: &Value) -> Result<String> {
    let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(code));
    git(&["reset", "-q", "--hard", head])?;
    let path = code.join(m["file"].as_str().context("a mutant names its file")?);
    let text = std::fs::read_to_string(&path)?;
    let n = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(1);
    let at = |p: &Value| offset(&text, n(&p["line"]), n(&p["column"]));
    let (start, end) = (at(&m["span"]["start"]), at(&m["span"]["end"]));
    let replacement = m["replacement"].as_str().unwrap_or("");
    std::fs::write(&path, format!("{}{replacement}{}", &text[..start], &text[end..]))?;
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

/// The title and the Problem and Shape of the issue a squash subject names (`feat(m6-9): the title (#62)`), from the
/// milestone's design: the note `**<i>.**` under "## Issues", and the items under "## Design change" that the issue's
/// row lists.
fn issue_of(redesign: &Path, subject: &str) -> Result<(String, String)> {
    let (tag, rest) = subject.split_once("): ").with_context(|| format!("`{subject}` is no `<type>(m<m>-<i>): `"))?;
    let tag = tag.rsplit('(').next().unwrap_or(tag);
    let (m, i) = tag
        .strip_prefix('m')
        .and_then(|t| t.split_once('-'))
        .filter(|(m, i)| [m, i].iter().all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
        .with_context(|| format!("`{subject}` names no single issue m<m>-<i>"))?;
    let title = rest.rsplit_once(" (#").map_or(rest, |r| r.0);
    let title = title[..1].to_uppercase() + &title[1..];
    let design = std::fs::read_dir(redesign)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&format!("-milestone-{m}.md"))))
        .with_context(|| format!("no *-milestone-{m}.md in {}", redesign.display()))?;
    let text = std::fs::read_to_string(&design)?;
    let section = |heading: &str| {
        let from = text.lines().position(|l| l == heading).unwrap_or(usize::MAX);
        text.lines().skip(from).skip(1).take_while(|l| !l.starts_with("## ")).collect::<Vec<_>>()
    };
    let issues = section("## Issues");
    let row = issues
        .iter()
        .find(|l| l.starts_with(&format!("| {i} |")))
        .with_context(|| format!("{}: no row for issue {i}", design.display()))?;
    let cells: Vec<&str> = row.split('|').map(str::trim).collect();
    // A note runs to the next note's `**<j>.**`, which may share its line.
    let note = issues.iter().find_map(|l| {
        let after = l.split_once(&format!("**{i}.** "))?.1;
        let next = after.match_indices(" **").map(|m| m.0).find(|&at| {
            let rest = after[at + 3..].trim_start_matches(|c: char| c.is_ascii_digit());
            rest.len() < after.len() - at - 3 && rest.starts_with(".**")
        });
        Some(after[..next.unwrap_or(after.len())].to_string())
    });
    let problem = note.unwrap_or_else(|| format!("Issue {i} of milestone {m}: {}.", cells[2]));
    let named: BTreeSet<&str> = cells[3].split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).collect();
    let changes = section("## Design change");
    let mut shape = Vec::new();
    let mut taking = false;
    for line in changes {
        if let Some((n, _)) =
            line.split_once(". ").filter(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            taking = named.contains(n);
        }
        if taking {
            shape.push(line);
        }
    }
    let shape = shape.join("\n").trim().to_string();
    Ok((title, format!("## Problem\n\n{problem}\n\n## Shape\n\n{shape}")))
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

        let control =
            Fixture::parse("control-9", &format!("kind: control\nbase: b\nhead: h\ntitle: T\ntruth:\n---\n{issue}"));
        assert!(control.is_ok_and(|c| c.truth.is_empty() && c.mutant.is_none()), "a control has no truth");
        for bad in [
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
    fn the_pick_is_seeded_and_spreads_the_genres() {
        let all: Vec<Value> = (0..9)
            .map(|i| serde_json::json!({"name": format!("m{i}"), "genre": if i < 7 { "BinaryOperator" } else { "FnValue" }}))
            .collect();
        let names =
            |seed| pick(all.iter().collect(), 4, seed).iter().map(|m| m["name"].to_string()).collect::<Vec<_>>();
        assert_eq!(names(7), names(7), "one seed, one pick");
        assert_ne!(names(7), names(8));
        let fn_value = pick(all.iter().collect(), 4, 7).iter().filter(|m| m["genre"] == "FnValue").count();
        assert_eq!(fn_value, 2, "both of the rare genre, before a third of the common one");
        assert_eq!(pick(all.iter().collect(), 20, 7).len(), 9, "no more than survive");
    }

    #[test]
    fn a_span_is_replaced_by_character_column() {
        let text = "fn a() {\n    let s = \"é\"; s > t\n}\n";
        let (start, end) = (offset(text, 2, 20), offset(text, 2, 21));
        assert_eq!(&text[start..end], ">");
        assert_eq!(offset(text, 3, 2), text.len() - 1);
    }

    #[test]
    fn the_issue_comes_from_the_milestone_design() {
        let dir = tempfile::tempdir().unwrap();
        let design = "# 29\n\n## Design change\n\n1. **One** first.\n   - a sub-point\n2. **Two** second.\n\n## Finish line\n\nx\n\n## Issues\n\n| # | slug | changes | after | blocks |\n|---|---|---|---|---|\n| 1 | `one` | 1 | — | `a` |\n| 2 | `two` | 2, 1 | — | `b` |\n\n- **1.** The note of one, with **bold** in it. **2.** The note of two.\n";
        std::fs::write(dir.path().join("29-milestone-6.md"), design).unwrap();
        let (title, body) = issue_of(dir.path(), "feat(m6-2): the second issue (#58)").unwrap();
        assert_eq!(title, "The second issue");
        assert_eq!(
            body,
            "## Problem\n\nThe note of two.\n\n## Shape\n\n1. **One** first.\n   - a sub-point\n2. **Two** second."
        );
        let (_, one) = issue_of(dir.path(), "fix(m6-1): one (#57)").unwrap();
        assert!(one.contains("The note of one, with **bold** in it.\n") && !one.contains("Two"), "{one}");
        assert!(issue_of(dir.path(), "feat(m5-6..8): three (#46)").is_err(), "a range names no single issue");
        assert!(issue_of(dir.path(), "docs: a template (#18)").is_err());
    }
}
