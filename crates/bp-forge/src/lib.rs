//! Forge adapter: pull request, check runs, branch base; the write calls sit behind the write feature.
//!
//! The snapshot is the forge state the `Intents` function reads, loaded from a directory of `gh` JSON
//! (`open-prs.json` plus `pr-<n>.json`). [`Gh`] reads the live forge into the same types, and a repo verb writes the
//! snapshot through it (27, design change 9). A test swaps the `gh` on `PATH`, not a trait (26, design change 1).

mod gh;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

pub use gh::Gh;

/// One pull request as `gh pr view --json` writes it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// The forge number.
    pub number: u64,
    /// The forge page; a snapshot written before the Change page lacks it.
    #[serde(default)]
    pub url: Option<String>,
    /// The head branch, `<prefix>/<issue slug>`.
    pub head_ref_name: String,
    /// The head commit.
    pub head_ref_oid: String,
    /// The branch it merges into; a snapshot written before milestone 3 lacks it.
    #[serde(default)]
    pub base_ref_name: Option<String>,
    /// `OPEN`, `MERGED` or `CLOSED`; `gh pr list` omits it for open pull requests.
    #[serde(default)]
    pub state: Option<String>,
    /// Check runs and status contexts at the head.
    #[serde(default)]
    pub status_check_rollup: Vec<CheckRun>,
}

/// One check run (`conclusion`) or status context (`state`).
#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    /// The check run's conclusion; empty while it runs.
    #[serde(default)]
    pub conclusion: Option<String>,
    /// The status context's state.
    #[serde(default)]
    pub state: Option<String>,
}

impl PullRequest {
    /// The last segment of the head branch: the issue's slug (15).
    pub fn slug(&self) -> &str {
        self.head_ref_name.rsplit_once('/').map_or(&self.head_ref_name, |(_, s)| s)
    }

    /// Open: `state` is `OPEN`, or absent as in `gh pr list`.
    pub fn is_open(&self) -> bool {
        self.state.as_deref().is_none_or(|s| s == "OPEN")
    }

    /// Green (15, Controls 3): at least one check `SUCCESS`, and every check `SUCCESS`, `NEUTRAL` or `SKIPPED`, as
    /// GitHub passes a required check. A pending check is not green.
    pub fn is_green(&self) -> bool {
        self.status_check_rollup.iter().any(|c| verdict(c) == Some("SUCCESS"))
            && self.status_check_rollup.iter().all(|c| matches!(verdict(c), Some("SUCCESS" | "NEUTRAL" | "SKIPPED")))
    }

    /// A check ended in a way no wait turns green: it needs a fix and a push, not time.
    pub fn has_failed(&self) -> bool {
        self.status_check_rollup.iter().any(|c| {
            matches!(
                verdict(c),
                Some("FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "STALE")
            )
        })
    }
}

/// A check's verdict: a check run's conclusion once it has one, else a status context's state.
fn verdict(c: &CheckRun) -> Option<&str> {
    c.conclusion.as_deref().filter(|s| !s.is_empty()).or(c.state.as_deref())
}

/// A `pr-<n>.json` file: one `gh pr view`.
fn is_view(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
        && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("pr-"))
}

/// The forge state at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Every pull request known, one per number.
    pub prs: Vec<PullRequest>,
    /// The oldest file's modification time: when the earliest forge read in it was written.
    pub oldest: Option<SystemTime>,
}

impl Snapshot {
    /// Reads `open-prs.json` and every `pr-<n>.json` in `dir`. The open list wins on a shared number. An error
    /// names the file it came from.
    pub fn from_dir(dir: &Path) -> Result<Self, (PathBuf, String)> {
        fn parse<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, (PathBuf, String)> {
            let bytes = std::fs::read(path).map_err(|e| (path.to_path_buf(), e.to_string()))?;
            serde_json::from_slice(&bytes).map_err(|e| (path.to_path_buf(), e.to_string()))
        }
        let open = dir.join("open-prs.json");
        let mut prs: Vec<PullRequest> = parse(&open)?;
        let mut cached: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| (dir.to_path_buf(), e.to_string()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_view(p))
            .collect();
        cached.sort();
        for path in &cached {
            let pr: PullRequest = parse(path)?;
            if !prs.iter().any(|p| p.number == pr.number) {
                prs.push(pr);
            }
        }
        let oldest =
            cached.iter().chain([&open]).filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).min();
        Ok(Self { prs, oldest })
    }

    /// The open pull request whose head branch's last segment is the issue's slug (15).
    pub fn open_for(&self, slug: &str) -> Option<&PullRequest> {
        self.prs.iter().find(|p| p.is_open() && p.slug() == slug)
    }

    /// The latest merged pull request of the issue's slug: a `land` that stopped before its vault commit.
    pub fn merged_for(&self, slug: &str) -> Option<&PullRequest> {
        self.prs.iter().filter(|p| p.state.as_deref() == Some("MERGED") && p.slug() == slug).max_by_key(|p| p.number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_snapshot_and_finds_the_open_pull_request() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("open-prs.json"),
            r#"[{"number":2,"headRefName":"bp/0002.b","headRefOid":"h2","reviews":[{"state":"APPROVED","author":{"login":"r"},"commit":{"oid":"h2"}},{"state":"APPROVED","author":{"login":"r"},"commit":{"oid":"old"}}],"statusCheckRollup":[{"conclusion":"SUCCESS"},{"state":"SUCCESS"}]}]"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("pr-1.json"),
            r#"{"number":1,"headRefName":"bp/0001.a","headRefOid":"h1","state":"MERGED"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("pr-2.json"),
            r#"{"number":2,"headRefName":"bp/0002.b","headRefOid":"old","state":"OPEN"}"#,
        )
        .unwrap();
        let s = Snapshot::from_dir(dir.path()).unwrap();
        assert_eq!(s.prs.len(), 2);
        assert!(s.open_for("0001.a").is_none(), "a merged pull request is not open");
        let pr = s.open_for("0002.b").unwrap();
        assert_eq!(pr.head_ref_oid, "h2");
        assert!(pr.is_green(), "a forge review is read as nothing (37, design change 3)");
        assert_eq!(s.merged_for("0001.a").map(|p| p.number), Some(1));
        assert!(s.merged_for("0002.b").is_none());
        let (path, _) = Snapshot::from_dir(&dir.path().join("none")).unwrap_err();
        assert!(path.ends_with("none/open-prs.json"), "the error names the file");
    }

    #[test]
    fn parses_real_gh_pr_view_output() {
        // `gh pr view 14722 --repo cli/cli --json` with the fields of 2026-10-20, a superset of `FIELDS` now.
        let pr: PullRequest = serde_json::from_str(include_str!("../testdata/pr-view.json")).unwrap();
        assert_eq!((pr.number, pr.head_ref_name.as_str()), (14722, "bdehamer-release-create-atomicity-help"));
        assert!(!pr.is_open(), "merged");
        let head = "9e1e1c929f882fd93e1c67ea520a4529352aab9a";
        assert_eq!(pr.head_ref_oid, head);
        assert_eq!(pr.base_ref_name.as_deref(), Some("trunk"));
        assert!(pr.is_green(), "27 skipped and 12 successful checks are green, as on GitHub");
    }

    #[test]
    fn green_needs_a_success_and_no_check_failed_or_pending() {
        let green = |checks: &str| {
            let pr: PullRequest = serde_json::from_str(&format!(
                r#"{{"number":1,"headRefName":"x","headRefOid":"h","statusCheckRollup":{checks}}}"#
            ))
            .unwrap();
            pr.is_green()
        };
        assert!(green(r#"[{"conclusion":"SUCCESS"},{"conclusion":"NEUTRAL"},{"conclusion":"SKIPPED"}]"#));
        assert!(green(r#"[{"state":"SUCCESS"}]"#), "a status context");
        assert!(!green(r#"[{"conclusion":"SKIPPED"}]"#), "no success");
        assert!(!green(r#"[{"conclusion":"SUCCESS"},{"conclusion":""}]"#), "a check run still running");
        assert!(!green(r#"[{"conclusion":"SUCCESS"},{"state":"PENDING"}]"#), "a pending status context");
        assert!(!green(r#"[{"conclusion":"SUCCESS"},{"conclusion":"CANCELLED"}]"#));
        let failed = |checks: &str| {
            serde_json::from_str::<PullRequest>(&format!(
                r#"{{"number":1,"headRefName":"x","headRefOid":"h","statusCheckRollup":{checks}}}"#
            ))
            .unwrap()
            .has_failed()
        };
        assert!(failed(r#"[{"conclusion":"SUCCESS"},{"conclusion":"CANCELLED"}]"#) && failed(r#"[{"state":"ERROR"}]"#));
        assert!(!failed(r#"[{"conclusion":""},{"state":"PENDING"}]"#) && !failed("[]"), "pending is not failed");
    }

    #[test]
    fn no_checks_is_not_green() {
        let pr: PullRequest = serde_json::from_str(
            r#"{"number":1,"headRefName":"x","headRefOid":"h","statusCheckRollup":[{"conclusion":"FAILURE"}]}"#,
        )
        .unwrap();
        assert!(!pr.is_green());
        let pr: PullRequest = serde_json::from_str(r#"{"number":1,"headRefName":"x","headRefOid":"h"}"#).unwrap();
        assert!(!pr.is_green() && pr.is_open());
    }
}
