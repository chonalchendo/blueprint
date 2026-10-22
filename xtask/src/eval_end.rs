//! What a worker trial left in its sandbox when the agent stopped (29, design change 2). The transcript shows what the
//! worker did; the stop of 21's `complete` and `respond` rows is a fact of the vault and the clone. The runner reads
//! those facts once and appends them to the trial's vault log as `end:` lines, so the log's hash in the result file
//! covers them, and the grader recomputes the outcome from the stored files alone.
//!
//! ```text
//! end: done true            walk --for <action> --done, which is also true for an escalated claim
//! end: checks pass          the latest run of each error block of the issue passed
//! end: file <path>          complete: a file that the worker's branch changed against the dependency head or the base
//! end: moved yes            respond: the worker's head is not the review's head
//! end: disposition 2 fixed  one line per recorded disposition: the finding, `fixed` or `declined`, then
//!                           `reasoned` when a decline names a reason
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use bp_operation::Action;
use serde_json::Value;

use crate::eval::Skill;
use crate::eval_fixture::Fixture;
use crate::eval_sandbox::{Sandbox, capture};

/// A disposition as recorded: the finding it answers, `fixed` or `declined`, and whether a decline gives a reason.
pub struct Answer {
    pub finding: usize,
    pub fixed: bool,
    pub reasoned: bool,
}

#[derive(Default)]
pub struct End {
    pub done: bool,
    pub checks: bool,
    pub files: Vec<String>,
    pub moved: bool,
    pub answers: Vec<Answer>,
    /// The reason of the last `release` commit in the log, if the claim was released.
    pub released: Option<String>,
}

impl End {
    pub fn parse(log: &str) -> Self {
        let mut end = Self::default();
        let release = log.lines().filter_map(|l| l.strip_prefix("release ")).next_back();
        end.released = release.and_then(|r| r.split_whitespace().last()).map(str::to_string);
        for line in log.lines().filter_map(|l| l.strip_prefix("end: ")) {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["done", v] => end.done = *v == "true",
                ["checks", v] => end.checks = *v == "pass",
                ["file", path] => end.files.push((*path).to_string()),
                ["moved", v] => end.moved = *v == "yes",
                ["disposition", n, how, rest @ ..] => end.answers.push(Answer {
                    finding: n.parse().unwrap_or(0),
                    fixed: *how == "fixed",
                    reasoned: rest == ["reasoned"],
                }),
                _ => {}
            }
        }
        end
    }

    /// The worker released the claim `escalated`: that ends its dispatch (21), so the stop reads true.
    pub fn escalated(&self) -> bool {
        self.released.as_deref() == Some("escalated")
    }

    /// Whether every changed file sits in a crate that the landed issue changed: the first two path parts, as
    /// `replay-evidence.sh` compares them. A run that changed nothing is within scope and not done.
    pub fn within(&self, scope: &[String]) -> bool {
        self.files.iter().all(|f| scope.contains(&crate_of(f)))
    }
}

/// The first two parts of a path: `crates/bp-tool` for `crates/bp-tool/src/cut.rs`, `justfile` for itself.
pub fn crate_of(path: &str) -> String {
    path.splitn(3, '/').take(2).collect::<Vec<_>>().join("/")
}

impl Sandbox {
    /// The `end:` lines of a worker trial. A review has none: its record is in the log already.
    pub fn end_state(&self, skill: &Skill, fx: &Fixture) -> Result<String> {
        let mut out = Vec::new();
        let walk = self
            .command("bp", &self.code())
            .args(["walk", &self.issue, "--for", &skill.action.to_string(), "--done", "--json"])
            .output()?;
        let done = serde_json::from_slice::<Value>(&walk.stdout).is_ok_and(|v| v["done"] == true);
        out.push(format!("end: done {done}"));
        let dir = self.vault.join(&self.issue);
        if skill.action == Action::Complete {
            out.push(format!("end: checks {}", if latest_runs_pass(&dir.join("runs")) { "pass" } else { "fail" }));
        }
        if let Some(w) = self.worktree()? {
            let git = |args: &[&str]| capture(Command::new("git").args(args).current_dir(&w));
            if skill.action == Action::Complete {
                let names = git(&["diff", "--name-only", &self.scope_base, "HEAD"])?;
                out.extend(names.lines().map(|f| format!("end: file {f}")));
            } else {
                out.push(format!("end: moved {}", if git(&["rev-parse", "HEAD"])? == fx.head { "no" } else { "yes" }));
            }
        }
        if skill.action == Action::Respond {
            out.extend(dispositions(&dir.join("reviews")));
        }
        Ok(out.join("\n") + "\n")
    }

    /// The worktree of the issue's branch, if the worker cut one: `git worktree list` pairs a path with its branch.
    fn worktree(&self) -> Result<Option<PathBuf>> {
        let list = capture(Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(self.code()))?;
        let branch = format!("branch refs/heads/bp/{}", self.issue.rsplit('/').next().unwrap_or_default());
        let found = list.split("\n\n").find(|rec| rec.lines().any(|l| l == branch));
        Ok(found.and_then(|rec| rec.lines().find_map(|l| l.strip_prefix("worktree "))).map(PathBuf::from))
    }
}

/// The `key: value` lines of a record's header.
fn header(file: &Path) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let head = text.strip_prefix("---\n").and_then(|t| t.split_once("\n---\n")).map_or("", |(h, _)| h);
    head.lines().filter_map(|l| l.split_once(": ")).map(|(k, v)| (k.into(), v.trim_matches('"').into())).collect()
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    all.sort();
    all
}

/// Whether the issue has an error run and the latest run of each error block, by its time and then its attempt,
/// passed. A worker that failed a block and then fixed it has passed; the replay's rule counted that run against it.
fn latest_runs_pass(runs: &Path) -> bool {
    let mut latest: BTreeMap<String, ((String, u32), bool)> = BTreeMap::new();
    for h in files_in(runs).iter().map(|f| header(f)).filter(|h| h.get("severity").is_some_and(|s| s == "error")) {
        let when = (h["at"].clone(), h["attempt"].parse().unwrap_or(0));
        let slot = latest.entry(h["check"].clone()).or_default();
        if when >= slot.0 {
            *slot = (when, h["verdict"] == "pass");
        }
    }
    !latest.is_empty() && latest.values().all(|v| v.1)
}

/// One `end:` line for each disposition under the issue's reviews.
fn dispositions(reviews: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for review in files_in(reviews) {
        for file in files_in(&review.join("dispositions")) {
            let h = header(&file);
            let how = h.get("as").map_or("", String::as_str);
            let reasoned = how == "declined" && h.get("reason").is_some_and(|r| !r.trim().is_empty());
            let finding = h.get("finding").map_or("0", String::as_str);
            out.push(format!("end: disposition {finding} {how}{}", if reasoned { " reasoned" } else { "" }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_lines_are_read_and_the_scope_is_by_crate() {
        let log = "cut x\n\nrelease projects/bp/issues/x escalated\nend: done true\nend: checks pass\nend: file crates/a/src/x.rs\nend: file justfile\nend: moved yes\nend: disposition 1 fixed\nend: disposition 2 declined reasoned\nend: disposition 3 declined\n";
        let end = End::parse(log);
        assert!(end.done && end.checks && end.moved && end.escalated());
        let answers: Vec<_> = end.answers.iter().map(|a| (a.finding, a.fixed, a.reasoned)).collect();
        assert_eq!(answers, [(1, true, false), (2, false, true), (3, false, false)]);
        let scope = |s: &[&str]| s.iter().copied().map(String::from).collect::<Vec<_>>();
        assert!(end.within(&scope(&["crates/a", "justfile"])));
        assert!(!end.within(&scope(&["crates/a"])), "justfile is outside");
        assert!(!End::parse("").done && End::parse("").within(&[]), "an empty log has not stopped");
        assert_eq!(crate_of(".github/workflows/ci.yml"), ".github/workflows");
        assert_eq!(crate_of("justfile"), "justfile");
    }
}
