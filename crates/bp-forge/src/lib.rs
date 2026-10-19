//! Forge adapter: pull request, reviews, check runs, branch base; the write calls sit behind the write feature.
//!
//! This milestone holds the snapshot: the forge state the `Intents` function reads, loaded from a directory of
//! `gh` JSON (`open-prs.json` plus `pr-<n>.json`). The live `gh` reads land with the repo verbs.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One pull request as `gh pr view --json` writes it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    /// The forge number.
    pub number: u64,
    /// The head branch, `<prefix>/<issue slug>`.
    pub head_ref_name: String,
    /// The head commit.
    pub head_ref_oid: String,
    /// `OPEN`, `MERGED` or `CLOSED`; `gh pr list` omits it for open pull requests.
    #[serde(default)]
    pub state: Option<String>,
    /// Reviews at any head.
    #[serde(default)]
    pub reviews: Vec<Review>,
    /// Check runs and status contexts at the head.
    #[serde(default)]
    pub status_check_rollup: Vec<CheckRun>,
}

/// One review.
#[derive(Debug, Clone, Deserialize)]
pub struct Review {
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`.
    pub state: String,
    /// The commit the review was left on.
    #[serde(default)]
    pub commit: Option<Commit>,
}

/// A commit as `gh` writes it.
#[derive(Debug, Clone, Deserialize)]
pub struct Commit {
    /// The full sha.
    pub oid: String,
}

/// One check run (`conclusion`) or status context (`state`).
#[derive(Debug, Clone, Deserialize)]
pub struct CheckRun {
    /// The check run's conclusion.
    #[serde(default)]
    pub conclusion: Option<String>,
    /// The status context's state.
    #[serde(default)]
    pub state: Option<String>,
}

impl PullRequest {
    /// Open: `state` is `OPEN`, or absent as in `gh pr list`.
    pub fn is_open(&self) -> bool {
        self.state.as_deref().is_none_or(|s| s == "OPEN")
    }

    /// At least one check, and every check succeeded.
    pub fn is_green(&self) -> bool {
        !self.status_check_rollup.is_empty()
            && self
                .status_check_rollup
                .iter()
                .all(|c| c.conclusion.as_deref().or(c.state.as_deref()) == Some("SUCCESS"))
    }

    /// Some review approved `head` (21: an approval is for one head; a push needs a new one).
    pub fn is_approved(&self, head: &str) -> bool {
        self.reviews.iter().any(|r| r.state == "APPROVED" && r.commit.as_ref().is_some_and(|c| c.oid == head))
    }
}

/// The forge state at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Every pull request known, one per number.
    pub prs: Vec<PullRequest>,
}

impl Snapshot {
    /// Reads `open-prs.json` and every `pr-<n>.json` in `dir`. The open list wins on a shared number. An error
    /// names the file it came from.
    pub fn from_dir(dir: &Path) -> Result<Self, (PathBuf, String)> {
        fn parse<T: serde::de::DeserializeOwned>(path: PathBuf) -> Result<T, (PathBuf, String)> {
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => return Err((path, e.to_string())),
            };
            serde_json::from_slice(&bytes).map_err(|e| (path, e.to_string()))
        }
        let mut prs: Vec<PullRequest> = parse(dir.join("open-prs.json"))?;
        let mut cached: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| (dir.to_path_buf(), e.to_string()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"))
                    && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("pr-"))
            })
            .collect();
        cached.sort();
        for path in cached {
            let pr: PullRequest = parse(path)?;
            if !prs.iter().any(|p| p.number == pr.number) {
                prs.push(pr);
            }
        }
        Ok(Self { prs })
    }

    /// The open pull request whose head branch's last segment is the issue's slug (15).
    pub fn open_for(&self, slug: &str) -> Option<&PullRequest> {
        self.prs.iter().find(|p| p.is_open() && p.head_ref_name.rsplit('/').next() == Some(slug))
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
            r#"[{"number":2,"headRefName":"bp/0002.b","headRefOid":"h2","reviews":[{"state":"APPROVED","commit":{"oid":"h2"}},{"state":"APPROVED","commit":{"oid":"old"}}],"statusCheckRollup":[{"conclusion":"SUCCESS"},{"state":"SUCCESS"}]}]"#,
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
        assert!(pr.is_green() && pr.is_approved("h2"));
        assert!(!pr.is_approved("h3"), "an approval of another head does not count");
        let (path, _) = Snapshot::from_dir(&dir.path().join("none")).unwrap_err();
        assert!(path.ends_with("none/open-prs.json"), "the error names the file");
    }

    #[test]
    fn no_checks_is_not_green() {
        let pr: PullRequest = serde_json::from_str(
            r#"{"number":1,"headRefName":"x","headRefOid":"h","statusCheckRollup":[{"conclusion":"FAILURE"}]}"#,
        )
        .unwrap();
        assert!(!pr.is_green());
        let pr: PullRequest = serde_json::from_str(r#"{"number":1,"headRefName":"x","headRefOid":"h"}"#).unwrap();
        assert!(!pr.is_green() && !pr.is_approved("h") && pr.is_open());
    }
}
