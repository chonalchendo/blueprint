//! The live forge over `gh` (22, constraint 1). Each call parses `gh`'s JSON into the snapshot types; a call that
//! fails, exits non-zero or prints what does not parse is `could_not_run`. No call approves: an approval comes only
//! from the reviewer identity, whose credential never enters an agent's environment (15, Controls 1).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use bp_vault::Refusal;
use serde::Deserialize;

use crate::PullRequest;

/// The fields every pull request read asks for, so a list row and a view parse into the same type.
const FIELDS: &str = "number,state,author,baseRefName,headRefName,headRefOid,reviews,statusCheckRollup";

/// `gh`, run in a clone of the project repository: `gh` finds the repository from the clone's `origin`.
#[derive(Debug, Clone)]
pub struct Gh {
    dir: PathBuf,
    /// A `PATH` for `gh` alone. A test puts a fake first on it; a process-wide `PATH` change would be unsafe.
    path: Option<OsString>,
}

impl Gh {
    /// `gh` from `PATH`, run in `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), path: None }
    }

    /// The token's own login (`gh api user`).
    pub fn login(&self) -> Result<String, Refusal> {
        #[derive(Deserialize)]
        struct User {
            login: String,
        }
        self.json::<User>(&["api", "user"]).map(|u| u.login)
    }

    /// Every pull request, open or not, whose head is `branch` (`gh pr list --head`).
    pub fn prs_for_head(&self, branch: &str) -> Result<Vec<PullRequest>, Refusal> {
        self.json(&["pr", "list", "--head", branch, "--state", "all", "--json", FIELDS])
    }

    /// One pull request (`gh pr view`).
    pub fn pr(&self, number: u64) -> Result<PullRequest, Refusal> {
        self.json(&["pr", "view", &number.to_string(), "--json", FIELDS])
    }

    /// Opens a pull request of `head` into `base` and returns its number, the last segment of the url `gh` prints.
    #[cfg(feature = "write")]
    pub fn create(&self, head: &str, base: &str, title: &str, body: &str) -> Result<u64, Refusal> {
        let args = ["pr", "create", "--head", head, "--base", base, "--title", title, "--body", body];
        let out = self.run(&args)?;
        let text = String::from_utf8_lossy(&out);
        text.trim().rsplit('/').next().and_then(|n| n.parse().ok()).ok_or_else(|| {
            Refusal::failed(Self::subject(&args), format!("gh printed no pull request url: {:?}", text.trim()))
        })
    }

    /// Moves the pull request onto `base` (`gh pr edit --base`).
    #[cfg(feature = "write")]
    pub fn retarget(&self, number: u64, base: &str) -> Result<(), Refusal> {
        self.run(&["pr", "edit", &number.to_string(), "--base", base]).map(drop)
    }

    /// Squash-merges the pull request, only while its head is `head`: a push after the reads makes the forge refuse.
    #[cfg(feature = "write")]
    pub fn merge(&self, number: u64, head: &str) -> Result<(), Refusal> {
        self.run(&["pr", "merge", &number.to_string(), "--squash", "--match-head-commit", head]).map(drop)
    }

    fn json<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<T, Refusal> {
        let out = self.run(args)?;
        serde_json::from_slice(&out)
            .map_err(|e| Refusal::failed(Self::subject(args), format!("gh printed JSON this tool cannot read: {e}")))
    }

    /// Stdout of a call that exited 0.
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, Refusal> {
        let mut cmd = Command::new("gh");
        cmd.args(args).current_dir(&self.dir).stdin(Stdio::null());
        if let Some(path) = &self.path {
            cmd.env("PATH", path);
        }
        let out = cmd.output().map_err(|e| {
            Refusal::failed(Self::subject(args), format!("{e}; install gh and put it on PATH, then `gh auth login`"))
        })?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(Refusal::failed(
                Self::subject(args),
                format!(
                    "gh exited {}: {}; fix the cause in {}, then rerun",
                    out.status,
                    err.trim(),
                    self.dir.display()
                ),
            ));
        }
        Ok(out.stdout)
    }

    fn subject(args: &[&str]) -> String {
        format!("gh {}", args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use bp_vault::Code;

    use super::*;

    /// A `gh` that runs `script`, alone on its `PATH`.
    fn fake(dir: &std::path::Path, script: &str) -> Gh {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("gh"), format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        Gh { dir: dir.to_path_buf(), path: Some(format!("{}:/usr/bin:/bin", bin.display()).into()) }
    }

    // One test writes and runs every fake: a fork in a parallel test while a fake is open for writing makes its exec
    // fail with "text file busy" on Linux.
    #[test]
    fn parses_each_call_and_a_failed_call_could_not_run() {
        let ok = tempfile::tempdir().unwrap();
        let gh = fake(
            ok.path(),
            r#"case "$1 $2" in
  "api user") echo '{"login":"quokka-agent","id":1}' ;;
  "pr list") echo '[{"number":3,"headRefName":"bp/q","headRefOid":"h","baseRefName":"master","author":{"login":"quokka-agent"}}]' ;;
  "pr view") echo '{"number":3,"headRefName":"bp/q","headRefOid":"h","reviews":[{"state":"APPROVED","author":{"login":"r"},"commit":{"oid":"h"}}]}' ;;
  "pr create") echo 'https://github.com/o/r/pull/3' ;;
  "pr edit") [ "$*" = "pr edit 3 --base master" ] || exit 9 ;;
  *) [ "$*" = "pr merge 3 --squash --match-head-commit h" ] || exit 9 ;;
esac"#,
        );
        assert_eq!(gh.login().unwrap(), "quokka-agent");
        let prs = gh.prs_for_head("bp/q").unwrap();
        assert_eq!(
            (prs[0].base_ref_name.as_deref(), prs[0].author.as_ref().map(|a| a.login.as_str())),
            (Some("master"), Some("quokka-agent"))
        );
        let pr = gh.pr(3).unwrap();
        assert_eq!(pr.reviews[0].author.as_ref().unwrap().login, "r");
        #[cfg(feature = "write")]
        {
            assert_eq!(gh.create("bp/q", "master", "t", "b").unwrap(), 3);
            gh.retarget(3, "master").unwrap();
            assert_eq!(gh.retarget(3, "bp/p").unwrap_err().code, Code::CouldNotRun, "the fake exits 9");
            gh.merge(3, "h").unwrap();
            assert_eq!(gh.merge(3, "other").unwrap_err().code, Code::CouldNotRun, "the fake exits 9");
        }

        let bad = tempfile::tempdir().unwrap();
        let gh = fake(bad.path(), "echo 'HTTP 401: Bad credentials' >&2; exit 1");
        let e = gh.login().unwrap_err();
        assert_eq!((e.code, e.exit, e.subject.as_str()), (Code::CouldNotRun, 1, "gh api user"));
        assert!(e.fix.contains("Bad credentials"), "the fix carries gh's own words: {}", e.fix);

        let garbled = tempfile::tempdir().unwrap();
        assert_eq!(fake(garbled.path(), "echo not-json").pr(1).unwrap_err().code, Code::CouldNotRun);

        let none = tempfile::tempdir().unwrap();
        let gh = Gh { dir: none.path().to_path_buf(), path: Some(none.path().as_os_str().to_owned()) };
        assert_eq!(gh.login().unwrap_err().code, Code::CouldNotRun, "no gh on PATH");
    }
}
