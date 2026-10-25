//! Grades the stored trials of a skill (29, design change 2). Code only: it reads each trial's transcript and vault
//! log from the store, the fixture and the bundle, and it recomputes every number of the result file. `--grade`
//! writes the file and the report. `--grade --check` compares the file with the recomputation, so a number typed by
//! hand, a swapped transcript or log, a dropped trial or an edited skill shows. The owner's two lines, `walkthrough:`
//! and `status:`, are kept.
//!
//! An arm built from a git ref (`--arm old=<ref>`, 30 design change 2) ran that ref's body: its trials carry the ref's
//! hash and a `ref:` line, and grade against that ref's files. A `--bin` arm's trials carry the shipped hash.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::eval::Skill;
use crate::eval_arms::{FILLER, PROTOCOL, body_of};
use crate::eval_fixture::NEAR;
use crate::eval_fixture::{Finding, Fixture};
use crate::eval_kills::{Proof, evidence};
use crate::eval_score::{Graded, score};
use crate::eval_trace::{Trace, sha256};

pub struct Grade {
    pub skill: Skill,
    pub trials: Vec<Graded>,
    pub(crate) holdout: usize,
    /// The hash of the role file and skill body as the files hold them now.
    current: String,
    /// The hash of each git ref an arm ran, by its sha.
    pub(crate) refs: BTreeMap<String, String>,
    pub fixtures: Vec<Fixture>,
}

/// The hash of the role file and the bodies of every skill that role preloads for the measured one,
/// `git hash-object --stdin`: the key of check 10. For `bp-review` that is `bp-reviewer.md` and `bp-review`'s body;
/// for both worker skills it is `bp-worker.md`, then `bp-complete`'s body, then `bp-respond`'s, because the `with`
/// arm of each preloads both and a sibling edit changes what was measured. A run writes the hash beside its
/// transcript, and a grade compares the traces' hash with the files'.
pub fn bundle_hash(root: &Path, skill: &Skill) -> Result<String> {
    let mut text = std::fs::read_to_string(root.join(format!("bundle/roles/{}.md", skill.role_file())))?;
    for name in skill.preloaded() {
        text += &std::fs::read_to_string(root.join("bundle/skills").join(&name).join("SKILL.md"))
            .with_context(|| format!("read the body of {name}"))?;
    }
    hash_object(&text)
}

/// [`bundle_hash`] of the files at the commit `sha`, and the first line of the skill's body there: what an arm built
/// from that commit ran, and the line its preload shows.
pub(crate) fn ref_hash(root: &Path, skill: &Skill, sha: &str) -> Result<(String, String)> {
    let show = |path: String| show_at(root, sha, &path);
    let mut text = show(format!("bundle/roles/{}.md", skill.role_file()))?;
    for name in skill.preloaded() {
        text += &show(format!("bundle/skills/{name}/SKILL.md"))?;
    }
    let body = show(format!("bundle/skills/{}/SKILL.md", skill.name))?;
    Ok((hash_object(&text)?, marker(&body)))
}

/// The file at `path` in the commit `sha`, byte for byte.
pub(crate) fn show_at(root: &Path, sha: &str, path: &str) -> Result<String> {
    let out = Command::new("git").args(["show", &format!("{sha}:{path}")]).current_dir(root).output()?;
    if !out.status.success() {
        bail!("git show {sha}:{path}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The first line of a skill's body: the text a preload of that body shows.
fn marker(skill_md: &str) -> String {
    body_of(skill_md).lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim().to_string()
}

/// What a trial saw besides the bundle: the fixture's base, head, title, issue, rules and stack, a mutant's diff, and
/// the arm protocol. The truth, the match words and the probe are left out, so a regrade after a grader fix keeps its
/// trials. A fixture with no mutant hashes as before, so its stored trials keep their stamps.
pub(crate) fn stamp(fx: &Fixture) -> Result<String> {
    let mut seen = format!("{}\n{}\n{}\n{}\n{:?}\n{:?}", fx.base, fx.head, fx.title, fx.issue, fx.rules, fx.stack);
    if let Some(m) = &fx.mutant {
        seen = format!("{seen}\n{}", m.diff);
    }
    Ok(format!("{} {PROTOCOL}", hash_object(&seen)?))
}

/// A trial's `.hash` file: the bundle hash, then `stamp: <stamp>`, then `ref: <sha>` for an arm built from a git ref.
/// A file from before the stamp has an empty one.
pub(crate) fn read_hash(store: &Path, trial: &str) -> Option<(String, String)> {
    let text = std::fs::read_to_string(store.join(format!("{trial}.hash"))).ok()?;
    let mut lines = text.lines();
    let hash = lines.next()?.trim().to_string();
    Some((hash, lines.find_map(|l| l.strip_prefix("stamp: ")).unwrap_or_default().to_string()))
}

/// The `ref:` line of a trial's `.hash` file: the commit whose body the trial ran.
fn read_ref(store: &Path, trial: &str) -> Option<String> {
    let text = std::fs::read_to_string(store.join(format!("{trial}.hash"))).ok()?;
    text.lines().find_map(|l| l.strip_prefix("ref: ")).map(|r| r.trim().to_string())
}

/// What one row of a review earned (30, design change 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The row substantiates the item: by its words on a past review's fixture, by its place and its evidence on a
    /// planted one.
    Match(usize),
    /// The row sits on an item that another row matched: neither a match nor a false row.
    Duplicate(usize),
    /// A control's row on a defect that the landed head already holds (`known:`).
    Known(usize),
    False,
}

/// The cells of a table row, split at each `|` outside backticks and not escaped, so a command such as `a | b` in a
/// finding stays in one cell. The closing `|` leaves no empty cell.
pub(crate) fn cells(row: &str) -> Vec<&str> {
    let row = row.trim();
    let inner = row.strip_prefix('|').unwrap_or(row);
    let (mut out, mut start, mut ticks, mut escaped) = (Vec::new(), 0, false, false);
    for (i, c) in inner.char_indices() {
        if c == '`' {
            ticks = !ticks;
        } else if c == '|' && !ticks && !escaped {
            out.push(inner[start..i].trim());
            start = i + 1;
        }
        escaped = c == '\\';
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// The verdict of each row. A control's row is `Known` when it is within [`NEAR`] lines of a `known:` entry, or in its
/// file and holds its `match:` words, and false otherwise. On a planted fixture a row with evidence ([`evidence`])
/// matches the first open item whose span it points into; when two items share the span, it must also hold one of the
/// item's `match:` slots. On a past review's fixture a row matches the item its words fit. A row that matches no open
/// item is a duplicate when it sits on a matched one, so a second true row is not a false one: on a planted fixture it
/// points into the item's span; on a past review's it is within [`NEAR`] lines of the item, or in its file and holding
/// one of its slots.
pub fn verdicts(fx: &Fixture, findings: &[Finding], proof: &Proof) -> Vec<Verdict> {
    if fx.kind.as_deref() == Some("control") {
        let known = |f: &Finding| {
            fx.known.iter().position(|k| k.near(&f.location, NEAR) || (k.in_file_at(&f.location) && k.worded(&f.text)))
        };
        return findings.iter().map(|f| known(f).map_or(Verdict::False, Verdict::Known)).collect();
    }
    let planted = fx.genre().is_some();
    let mut matched = BTreeSet::new();
    let first: Vec<Option<Verdict>> = if planted {
        let mut kill = |f: &Finding| {
            let covering: Vec<usize> = (0..fx.truth.len()).filter(|i| fx.truth[*i].covers(&f.location)).collect();
            let shared = covering.len() > 1;
            let open = |i: &usize| !matched.contains(i) && (!shared || fx.truth[*i].touched_by(&f.text));
            let i = covering.into_iter().find(open).filter(|_| evidence(&f.text, proof))?;
            matched.insert(i);
            Some(Verdict::Match(i))
        };
        findings.iter().map(&mut kill).collect()
    } else {
        let mut take = |i: usize| if matched.insert(i) { Verdict::Match(i) } else { Verdict::Duplicate(i) };
        fx.assign(findings).into_iter().map(|a| a.map(&mut take)).collect()
    };
    let on = |f: &Finding, i: usize| {
        let t = &fx.truth[i];
        if planted {
            t.covers(&f.location)
        } else {
            t.near(&f.location, NEAR) || (t.covers(&f.location) && t.touched_by(&f.text))
        }
    };
    let duplicate = |f: &Finding| matched.iter().copied().find(|i| on(f, *i));
    findings
        .iter()
        .zip(first)
        .map(|(f, v)| v.unwrap_or_else(|| duplicate(f).map_or(Verdict::False, Verdict::Duplicate)))
        .collect()
}

impl Grade {
    /// Grades every `<arm>-<fixture>-<n>.jsonl` in `store`. `fixtures` is the skill's fixture directory.
    pub fn read(skill: &Skill, store: &Path, fixtures: &Path, root: &Path) -> Result<Self> {
        let fixtures_all = Fixture::load(fixtures, None)?;
        let body = std::fs::read_to_string(root.join("bundle/skills").join(&skill.name).join("SKILL.md"))
            .with_context(|| format!("read the body of {}", skill.name))?;
        let shipped = marker(&body);
        // A numbered design cite is evidence only when the design exists (30, design change 2).
        let designs = crate::eval_kills::designs(std::env::var_os("BP_REDESIGN").as_deref().map(Path::new));
        let mut refs = BTreeMap::new();
        let mut markers = BTreeMap::new();
        let mut trials = Vec::new();
        // A fixture set no run has reached yet has no store: it grades as no trials.
        let entries = match std::fs::read_dir(store) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            listed => listed.with_context(|| format!("read {}", store.display()))?.collect(),
        };
        let mut names: Vec<_> = entries
            .into_iter()
            .filter_map(|e| e.ok()?.file_name().to_str().map(str::to_string))
            .filter(|n| Path::new(n).extension().is_some_and(|e| e == "jsonl"))
            .collect();
        names.sort();
        for name in names {
            let trial = name.trim_end_matches(".jsonl").to_string();
            let (arm, rest) = trial.split_once('-').context("a trial is <arm>-<fixture>-<n>")?;
            let (fixture, number) = rest.rsplit_once('-').context("a trial is <arm>-<fixture>-<n>")?;
            let fx = fixtures_all
                .iter()
                .find(|f| f.id == fixture)
                .with_context(|| format!("{trial}: no fixture {fixture} in {}", fixtures.display()))?;
            let path = store.join(&name);
            let trace = Trace::parse(&std::fs::read_to_string(&path)?);
            let log_path = store.join(format!("{trial}.vault.log"));
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            let body_ref = read_ref(store, &trial);
            if let Some(sha) = body_ref.as_ref().filter(|sha| !refs.contains_key(*sha)) {
                let (hash, first) =
                    ref_hash(root, skill, sha).with_context(|| format!("{trial}: the body of {sha}"))?;
                refs.insert(sha.clone(), hash);
                markers.insert(sha.clone(), first);
            }
            let marker = body_ref.as_ref().map_or(&shipped, |sha| &markers[sha]);
            let loaded = match arm {
                "without" => !trace.mentions(marker) && !trace.mentions(FILLER),
                "filler" => trace.preloaded(FILLER) && !trace.mentions(marker),
                // `with`, an arm built from a git ref (its own body's first line), or an arm that ran with its own
                // `--bin`: an edited body keeps the body's first line.
                _ => trace.preloaded(marker),
            };
            let (hash, stamp) = read_hash(store, &trial).unzip();
            let mut graded = Graded {
                arm: arm.into(),
                fixture: fixture.into(),
                number: number.parse().with_context(|| format!("{trial}: the trial number is a count"))?,
                trial: trial.clone(),
                sha: sha256(&path)?,
                log_sha: if log_path.is_file() { sha256(&log_path)? } else { String::new() },
                hash,
                stamp: stamp.unwrap_or_default(),
                body_ref,
                models: trace.models(),
                loaded,
                failure: trace.failure().map(str::to_string),
                ..Graded::default()
            };
            // A trial from before the `.head` file ran at the fixture's own head.
            let head = std::fs::read_to_string(store.join(format!("{trial}.head")))
                .map_or_else(|_| fx.head.clone(), |h| h.trim().to_string());
            score(&mut graded, skill, fx, &head, &trace, &log, &designs);
            trials.push(graded);
        }
        let holdout = crate::eval_fixture::holdout(fixtures).len();
        let current = bundle_hash(root, skill)?;
        Ok(Self { skill: skill.clone(), trials, holdout, current, refs, fixtures: fixtures_all })
    }

    /// What makes the store ungradable, one line each: a trial without its hash or vault log, trials run on two
    /// bundles or on one that is not the files', and trial numbers with a gap (a dropped trial).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.trials {
            if t.hash.is_none() {
                out.push(format!("{}: no {}.hash; rerun the trial", t.trial, t.trial));
            }
            if t.log_sha.is_empty() {
                out.push(format!("{}: no {}.vault.log", t.trial, t.trial));
            }
        }
        let shipped = self.trials.iter().filter(|t| t.body_ref.is_none());
        let hashes: BTreeSet<&String> = shipped.filter_map(|t| t.hash.as_ref()).collect();
        if hashes.len() > 1 {
            out.push(format!("the store holds trials of {} bundles: {hashes:?}; rerun them on one", hashes.len()));
        } else if let Some(h) = hashes.first().filter(|h| ***h != self.current) {
            out.push(format!(
                "the trials ran on {h}, the role and skill files hash to {}; rerun the evals",
                self.current
            ));
        }
        let mut by_arm = BTreeMap::<&String, BTreeSet<&String>>::new();
        for t in &self.trials {
            let Some(sha) = &t.body_ref else { continue };
            by_arm.entry(&t.arm).or_default().insert(sha);
            if t.hash.as_ref().is_some_and(|h| *h != self.refs[sha]) {
                out.push(format!(
                    "{}: the trial's hash is not the hash of {sha}'s role and skill files; rerun it",
                    t.trial
                ));
            }
        }
        for (arm, shas) in by_arm.iter().filter(|a| a.1.len() > 1) {
            out.push(format!("the {arm} arm ran {} refs: {shas:?}; rerun it on one", shas.len()));
        }
        let mut stamps = BTreeMap::<&String, BTreeSet<&String>>::new();
        for t in &self.trials {
            stamps.entry(&t.fixture).or_default().insert(&t.stamp);
        }
        for (fixture, seen) in stamps.iter().filter(|s| s.1.len() > 1) {
            out.push(format!(
                "{fixture}: the trials ran on {} fixture or arm protocols: {seen:?}; move the store aside and rerun",
                seen.len()
            ));
        }
        let groups: BTreeSet<(&String, &String)> = self.trials.iter().map(|t| (&t.arm, &t.fixture)).collect();
        for (arm, fixture) in groups {
            let mut numbers: Vec<usize> =
                self.trials.iter().filter(|t| &t.arm == arm && &t.fixture == fixture).map(|t| t.number).collect();
            numbers.sort_unstable();
            if numbers != (1..=numbers.len()).collect::<Vec<_>>() {
                out.push(format!("{arm}-{fixture}: trial numbers {numbers:?} are not 1 to {}", numbers.len()));
            }
        }
        // A planted set's numbers are over every item: an arm that ran part of the set would count the rest as
        // missed, and `holdout_gain` would stand on trials that never ran.
        if self.kill_set() {
            let runs = |arm: &str, id: &str| self.trials.iter().filter(|t| t.arm == arm && t.fixture == id).count();
            let most = self.arms().iter().flat_map(|a| self.fixtures.iter().map(|f| runs(a, &f.id))).max().unwrap_or(0);
            for arm in self.arms() {
                let full = self.fixtures.iter().filter(|f| runs(arm, &f.id) == most).count();
                if full < self.fixtures.len() {
                    let times = if most > 1 { format!(" {most} times") } else { String::new() };
                    out.push(format!("the {arm} arm ran {full} of {} fixtures{times}; rerun it", self.fixtures.len()));
                }
            }
        }
        out
    }

    /// The shipped bundle the trials ran on: their one hash, else the files' (and `problems` says why they differ). An
    /// arm built from a git ref has its own line.
    pub(crate) fn hash(&self) -> &str {
        let ran: BTreeSet<&String> =
            self.trials.iter().filter(|t| t.body_ref.is_none()).filter_map(|t| t.hash.as_ref()).collect();
        if ran.len() == 1 { ran.first().map_or(&self.current, |h| h) } else { &self.current }
    }

    pub fn fixture(&self, id: &str) -> &Fixture {
        self.fixtures.iter().find(|f| f.id == id).expect("a graded trial names a loaded fixture")
    }

    pub fn arm(&self, arm: &str) -> Vec<&Graded> {
        self.trials.iter().filter(|t| t.arm == arm).collect()
    }
}

/// One arm's trials, counted.
pub(crate) struct Stats {
    pub runs: usize,
    pub passes: usize,
    /// Ground-truth items named in the final texts, over all runs, and the items the runs could have named.
    pub prose: usize,
    pub truth: usize,
    pub reached: usize,
    /// The mean share of steps done, over the runs that reached the tool; `None` when none did.
    pub adherence: Option<f64>,
    pub out_of_row: usize,
    pub found: usize,
    pub wrong: usize,
    /// Rows on an item another row matched.
    pub duplicates: usize,
    /// Runs whose table held no finding: a reviewer that returns nothing is not precise.
    pub silent: usize,
    pub loaded: usize,
}

impl Stats {
    pub fn of(runs: &[&Graded]) -> Self {
        let reached: Vec<&&Graded> = runs.iter().filter(|r| r.reached).collect();
        let found: Vec<&(String, String, Verdict)> = runs.iter().flat_map(|r| &r.findings).collect();
        Self {
            runs: runs.len(),
            passes: runs.iter().filter(|r| r.pass).count(),
            prose: runs.iter().map(|r| r.prose_hits).sum(),
            truth: runs.iter().map(|r| r.truth).sum(),
            reached: reached.len(),
            adherence: (!reached.is_empty())
                .then(|| reached.iter().map(|r| r.adherence()).sum::<f64>() / float(reached.len())),
            out_of_row: runs.iter().map(|r| r.out_of_row).sum(),
            found: found.len(),
            wrong: found.iter().filter(|f| f.2 == Verdict::False).count(),
            duplicates: found.iter().filter(|f| matches!(f.2, Verdict::Duplicate(_))).count(),
            silent: runs.iter().filter(|r| r.findings.is_empty()).count(),
            loaded: runs.iter().filter(|r| r.loaded).count(),
        }
    }
}

/// A count as a float. Counts of trials and findings are far below 2^52.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn float(n: usize) -> f64 {
    n as f64
}

/// `k` over `n`, for `n` above 0.
pub(crate) fn share(k: usize, n: usize) -> f64 {
    float(k) / float(n)
}

pub(crate) fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// `git hash-object --stdin`: the hash of the role file and the skill body together, the key of check 10.
pub(crate) fn hash_object(text: &str) -> Result<String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("run git hash-object")?;
    child.stdin.take().context("git's stdin")?.write_all(text.as_bytes())?;
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<Finding> {
        let review = crate::eval_fixture::Review::parse(&format!(
            "head: abc1234\n| severity | location | finding |\n|---|---|---|\n{text}"
        ));
        review.unwrap().findings
    }

    fn fixture(path: &str) -> Fixture {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let id = Path::new(path).file_stem().unwrap().to_str().unwrap();
        Fixture::parse(id, &std::fs::read_to_string(root.join("evals").join(path)).unwrap()).unwrap()
    }

    const NO_DESIGNS: &BTreeMap<String, String> = &BTreeMap::new();

    /// Run 3's `with-pr-20-1`: the first row names item 2's gap at item 1's line, and the third row matches item 2 by
    /// its words. The first is a second true row on item 2, not a false one. A row in item 1's file that names none of
    /// its substance is false. A planted span counts its first killing row.
    #[test]
    fn a_second_row_on_a_matched_item_is_duplicate() {
        let pr_20 = fixture("bp-review/pr-20.md");
        let none = Proof { commands: Vec::new(), designs: NO_DESIGNS };
        let run_3 = rows(
            "| warn | xtask/src/lines.rs:20 | The `#[cfg(test)]` cutoff is not tested by the `file-cap` block, because its only over-cap file has no test module; to pass, add a case where a file has at most 500 lines before `#[cfg(test)]` and more than 500 in total, and assert that `cargo xtask lines` exits 0. |\n\
             | warn | xtask/src/lines.rs:34 | Skipping whole `tests/` and `target` directories is not in the Shape of projects/bp/issues/the-file-cap, which counts each `.rs` file over `crates/` and `xtask/`, and no check block tests it. |\n\
             | warn | projects/bp/issues/the-file-cap | The `file-cap` block tests only 501 lines, so the other side of the boundary is never asserted. |\n\
             | warn | xtask/src/lines.rs:35 | The walk skips every `tests/` dir, which the Shape does not name. |\n\
             | warn | xtask/src/other.rs:3 | An unrelated taste. |\n\
             | warn | xtask/src/lines.rs:3 | The walk leaks a file handle. |",
        );
        assert_eq!(
            verdicts(&pr_20, &run_3, &none),
            [
                Verdict::Duplicate(1),
                Verdict::Match(2),
                Verdict::Match(1),
                Verdict::Duplicate(2),
                Verdict::False,
                Verdict::False
            ]
        );
        let mutant = Fixture::parse(
            "mutant-1-1",
            "kind: mutant\nbase: b\nhead: h\ntitle: T\ngenre: BinaryOperator\nspan: src/a.rs:10-10\nseed: 1\nmutant_diff: |\n    x\ntruth:\n- src/a.rs:10-10 | replace - with /\n---\nbody",
        )
        .unwrap();
        let kill = "| error | src/a.rs:10 | `cargo test -p a -- width` fails: 3 for 9 |";
        let prose = "| warn | src/a.rs:10 | The width divides where it should subtract. |";
        let ran = Proof { commands: vec!["cd /w && cargo test -p a -- width 2>&1 | tail"], designs: NO_DESIGNS };
        let found = |text: &str| verdicts(&mutant, &rows(text), &ran);
        assert_eq!(
            found(&format!("{prose}\n{kill}\n{kill}")),
            [Verdict::Duplicate(0), Verdict::Match(0), Verdict::Duplicate(0)]
        );
        assert_eq!(found(prose), [Verdict::False], "a row with no evidence kills nothing");
        assert_eq!(found(&kill.replace("a.rs:10", "a.rs:40")), [Verdict::False], "outside the span");
        assert_eq!(found(&kill.replace("a.rs:10", "a.rs:1-60")), [Verdict::False], "a range that points at nothing");
        assert_eq!(found(&kill.replace("src/a.rs:10", "`src/a.rs:8-12`")), [Verdict::Match(0)]);
        assert_eq!(found(&kill.replace("a.rs:10", "a.rs:1-20")), [Verdict::False], "a 20-line range");
        assert_eq!(
            found(&kill.replace("a.rs:10", "a.rs:1-4, 5-8, 9-12, 13-16, 17-20, 21-24")),
            [Verdict::False],
            "six narrow ranges that cite 24 lines in total"
        );
        assert_eq!(found(&kill.replace("a.rs:10", "a.rs:10-1")), [Verdict::False], "a reversed range");
        let unran = Proof { commands: vec!["cargo test -p b"], designs: NO_DESIGNS };
        assert_eq!(verdicts(&mutant, &rows(kill), &unran), [Verdict::False], "a command the trial never ran");
    }

    /// Escapes 19 and 20 share a span: a kill must name one item's substance, and the same row twice kills one item.
    #[test]
    fn a_shared_span_needs_the_items_words() {
        let escape = fixture("bp-review-mutants/escape-11-19-20.md");
        let ran = Proof { commands: vec!["printf 'a 2>&1' | cargo test -p xtask trace"], designs: NO_DESIGNS };
        let row = "| error | xtask/src/eval_trace.rs:89 | `cargo test -p xtask trace` shows `2>&1` read as a verb |";
        assert_eq!(
            verdicts(&escape, &rows(&format!("{row}\n{row}")), &ran),
            [Verdict::Match(2), Verdict::Duplicate(2)]
        );
        let bare = "| error | xtask/src/eval_trace.rs:89 | `cargo test -p xtask trace` fails |";
        assert_eq!(verdicts(&escape, &rows(bare), &ran), [Verdict::False], "it names neither item");
    }

    #[test]
    fn a_row_with_a_piped_command_stays_one_row() {
        let found = rows(
            "| error | `src/a.rs:3` | `cargo test -q | grep -c FAILED` prints 1, and `a \\| b` too | `git diff x | wc -l` |",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].location, "`src/a.rs:3`");
        assert_eq!(found[0].text, "`cargo test -q | grep -c FAILED` prints 1, and `a \\| b` too|`git diff x | wc -l`");
        assert_eq!(cells("| a | `b | c` | d \\| e |"), ["a", "`b | c`", "d \\| e"]);
        assert_eq!(cells("| a |  | c"), ["a", "", "c"], "an empty cell stays, and no closing pipe is needed");
    }

    /// A control's row is false unless a `known:` entry covers it: within 3 lines, or by its `match:` words.
    #[test]
    fn a_control_row_on_a_known_defect_is_not_false() {
        let control = Fixture::parse(
            "control-1",
            "kind: control\nbase: b\nhead: h\ntitle: T\nknown: src/t.rs:99 | No if. | match: if|while, call\nknown: src/t.rs:50 | A split.\ntruth:\n---\nbody",
        )
        .unwrap();
        let none = Proof { commands: Vec::new(), designs: NO_DESIGNS };
        let found = |text: &str| verdicts(&control, &rows(text), &none);
        assert_eq!(found("| warn | src/t.rs:102 | anything |"), [Verdict::Known(0)]);
        assert_eq!(found("| warn | src/t.rs:103 | anything |"), [Verdict::False]);
        assert_eq!(found("| warn | src/t.rs:1 | a call after `while` is not counted |"), [Verdict::Known(0)]);
        assert_eq!(
            found("| warn | src/u.rs:1 | a call after `while` is not counted |"),
            [Verdict::False],
            "another file"
        );
        assert_eq!(
            found("| warn | src/t.rs:48 | x |\n| warn | src/t.rs:10 | y |"),
            [Verdict::Known(1), Verdict::False]
        );
        let control_62 = fixture("bp-review-mutants/control-62.md");
        let row =
            "| warn | xtask/src/eval_trace.rs:86 | A bp call after `while` is not counted: the prefix list lacks it. |";
        assert_eq!(verdicts(&control_62, &rows(row), &none), [Verdict::Known(0)]);
        let row = "| warn | xtask/src/eval_trace.rs:86 | A quoted `;` still splits the segment. |";
        assert_eq!(verdicts(&control_62, &rows(row), &none), [Verdict::Known(1)]);
        let row = "| warn | xtask/src/eval_sandbox.rs:40 | The prefix of the temp dir is wrong if TMPDIR is unset |";
        assert_eq!(verdicts(&control_62, &rows(row), &none), [Verdict::False], "the entry's file only");
        let row = "| warn | xtask/src/eval_trace.rs:10 | The prefix differs from the spec. |";
        assert_eq!(verdicts(&control_62, &rows(row), &none), [Verdict::False], "`if` inside `differs`");
    }
}
