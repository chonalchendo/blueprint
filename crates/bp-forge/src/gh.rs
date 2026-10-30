//! The live forge over `gh` (22, constraint 1). Each call parses `gh`'s JSON into the snapshot types; a call that
//! fails, exits non-zero or prints what does not parse is `could_not_run`. No call approves: an approval comes only
//! from the reviewer identity, whose credential never enters an agent's environment (15, Controls 1).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bp_vault::Refusal;
use serde::Deserialize;

use crate::{PullRequest, Snapshot};

/// The fields every pull request read asks for, so a list row and a view parse into the same type.
const FIELDS: &str = "number,url,state,author,baseRefName,headRefName,headRefOid,reviews,statusCheckRollup";

/// How many open pull requests the snapshot lists; `gh pr list` stops at 30 unless told.
const OPEN_LIMIT: &str = "1000";

/// Whether this process has run `gh`: one process runs one verb, so this is whether the verb reached the forge.
static CALLED: AtomicBool = AtomicBool::new(false);

/// `gh`, run in a clone of the project repository: `gh` finds the repository from the clone's `origin`.
#[derive(Debug, Clone)]
pub struct Gh {
    dir: PathBuf,
    /// A `PATH` for `gh` alone. A test puts a fake first on it; a process-wide `PATH` change would be unsafe.
    path: Option<OsString>,
    /// How long one call may run before it is killed; none waits as long as `gh` takes.
    deadline: Option<Duration>,
}

impl Gh {
    /// `gh` from `PATH`, run in `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into(), path: None, deadline: None }
    }

    /// The same `gh`, with each call killed after `limit` and refused `could_not_run`: a read verb must not hang on
    /// the network (34, design change 3).
    #[must_use]
    pub fn within(self, limit: Duration) -> Self {
        Self { deadline: Some(limit), ..self }
    }

    /// Whether this process has run `gh` yet. A repo verb refused before any forge call refreshes no snapshot (27,
    /// design change 9).
    pub fn called() -> bool {
        CALLED.load(Ordering::Relaxed)
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

    /// The pull request's body (`gh pr view --json body`), which `pr` keeps current.
    pub fn body(&self, number: u64) -> Result<String, Refusal> {
        #[derive(Deserialize)]
        struct Body {
            body: String,
        }
        self.json::<Body>(&["pr", "view", &number.to_string(), "--json", "body"]).map(|b| b.body)
    }

    /// Rewrites the snapshot in `dir` (27, design change 9): `gh pr list --state open` into `open-prs.json`, and
    /// `gh pr view <n>` into `pr-<n>.json` for each pull request whose slug `held` names. The held set is read from the
    /// open list and from the last snapshot, so a pull request merged under a held claim stays in view. Every call
    /// runs before any file is written, and each file goes in by a rename: a reader sees each file whole.
    pub fn snapshot(&self, dir: &Path, held: impl Fn(&str) -> bool) -> Result<(), Refusal> {
        let args = ["pr", "list", "--state", "open", "--limit", OPEN_LIMIT, "--json", FIELDS];
        let (prs, listed): (Vec<PullRequest>, _) = self.read(&args)?;
        let last = Snapshot::from_dir(dir).map(|s| s.prs).unwrap_or_default();
        let numbers: BTreeSet<u64> = prs.iter().chain(&last).filter(|p| held(p.slug())).map(|p| p.number).collect();
        let mut files = Vec::with_capacity(numbers.len() + 1);
        for n in &numbers {
            let (_, viewed): (PullRequest, _) = self.read(&["pr", "view", &n.to_string(), "--json", FIELDS])?;
            files.push((format!("pr-{n}.json"), viewed));
        }
        files.push(("open-prs.json".into(), listed));
        let failed = |e: std::io::Error| Refusal::failed(dir.display().to_string(), e.to_string());
        std::fs::create_dir_all(dir).map_err(failed)?;
        for (name, bytes) in &files {
            let (path, tmp) = (dir.join(name), dir.join(format!(".{name}.{}", std::process::id())));
            std::fs::write(&tmp, bytes)
                .and_then(|()| std::fs::rename(&tmp, &path))
                .map_err(|e| Refusal::failed(path.display().to_string(), e.to_string()))?;
        }
        // A view of a claim no longer held would outlive its pull request's close.
        for path in std::fs::read_dir(dir).map_err(failed)?.flatten().map(|e| e.path()) {
            if crate::is_view(&path) && !files.iter().any(|(name, _)| path.ends_with(name)) {
                std::fs::remove_file(path).map_err(failed)?;
            }
        }
        Ok(())
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

    /// Replaces the pull request's body (`gh pr edit --body`).
    #[cfg(feature = "write")]
    pub fn describe(&self, number: u64, body: &str) -> Result<(), Refusal> {
        self.run(&["pr", "edit", &number.to_string(), "--body", body]).map(drop)
    }

    /// Squash-merges the pull request, only while its head is `head`: a push after the reads makes the forge refuse.
    #[cfg(feature = "write")]
    pub fn merge(&self, number: u64, head: &str) -> Result<(), Refusal> {
        self.run(&["pr", "merge", &number.to_string(), "--squash", "--match-head-commit", head]).map(drop)
    }

    fn json<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<T, Refusal> {
        self.read(args).map(|(t, _)| t)
    }

    /// The parsed stdout and its bytes, as `gh` printed them.
    fn read<T: serde::de::DeserializeOwned>(&self, args: &[&str]) -> Result<(T, Vec<u8>), Refusal> {
        let out = self.run(args)?;
        let t = serde_json::from_slice(&out)
            .map_err(|e| Refusal::failed(Self::subject(args), format!("gh printed JSON this tool cannot read: {e}")))?;
        Ok((t, out))
    }

    /// Stdout of a call that exited 0.
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, Refusal> {
        CALLED.store(true, Ordering::Relaxed);
        let mut cmd = Command::new("gh");
        cmd.args(args).current_dir(&self.dir).stdin(Stdio::null());
        if let Some(path) = &self.path {
            cmd.env("PATH", path);
        }
        let out = match self.deadline {
            None => cmd.output(),
            Some(limit) => output_within(&mut cmd, limit),
        }
        .map_err(|e| {
            let fix = if e.kind() == std::io::ErrorKind::TimedOut {
                e.to_string()
            } else {
                format!("{e}; install gh and put it on PATH, then `gh auth login`")
            };
            Refusal::failed(Self::subject(args), fix)
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

/// `cmd`'s output, or `TimedOut` once it has run `limit`: the child is killed then. Each pipe drains on its own
/// thread, so a full pipe never stalls the child while the poll waits.
fn output_within(cmd: &mut Command, limit: Duration) -> std::io::Result<Output> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= limit {
            // The child may have exited between the poll and the kill; either way it is gone.
            let _ = child.kill();
            child.wait()?;
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("gh did not answer within {limit:?}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let joined = |h: JoinHandle<Vec<u8>>| h.join().unwrap_or_default();
    Ok(Output { status, stdout: joined(stdout), stderr: joined(stderr) })
}

/// Reads a child's pipe to its end on a thread of its own.
fn drain(pipe: Option<impl Read + Send + 'static>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error leaves what was read; the exit status still decides the call.
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
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
        Gh { dir: dir.to_path_buf(), path: Some(format!("{}:/usr/bin:/bin", bin.display()).into()), deadline: None }
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
  "pr view") [ "$5" = body ] && { echo '{"body":"b"}'; exit; }; echo '{"number":'$3',"headRefName":"bp/q","headRefOid":"h","state":"OPEN","reviews":[{"state":"APPROVED","author":{"login":"r"},"commit":{"oid":"h"}}]}' ;;
  "pr create") echo 'https://github.com/o/r/pull/3' ;;
  "pr edit") [ "$*" = "pr edit 3 --base master" ] || [ "$*" = "pr edit 3 --body b" ] || exit 9 ;;
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
        assert_eq!(gh.body(3).unwrap(), "b");

        // The snapshot: the open list, a view per held pull request, and no view left of one no longer held.
        let snap = ok.path().join("snap");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("pr-9.json"), r#"{"number":9,"headRefName":"bp/gone","headRefOid":"x"}"#).unwrap();
        gh.snapshot(&snap, |slug| slug == "q").unwrap();
        let s = Snapshot::from_dir(&snap).unwrap();
        assert!(s.oldest.is_some() && s.open_for("q").is_some_and(|p| p.base_ref_name.as_deref() == Some("master")));
        assert!(snap.join("pr-3.json").is_file() && !snap.join("pr-9.json").exists(), "a view per held claim only");
        gh.snapshot(&snap, |_| false).unwrap();
        assert!(!snap.join("pr-3.json").exists() && snap.join("open-prs.json").is_file());
        #[cfg(feature = "write")]
        {
            assert_eq!(gh.create("bp/q", "master", "t", "b").unwrap(), 3);
            gh.retarget(3, "master").unwrap();
            gh.describe(3, "b").unwrap();
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
        let gh = Gh { dir: none.path().to_path_buf(), path: Some(none.path().as_os_str().to_owned()), deadline: None };
        assert_eq!(gh.login().unwrap_err().code, Code::CouldNotRun, "no gh on PATH");

        // A deadline kills a call that hangs, and leaves a quick one as it was.
        let slow = tempfile::tempdir().unwrap();
        let gh = fake(
            slow.path(),
            "[ \"$1\" = api ] && exec sleep 30; echo '{\"number\":1,\"headRefName\":\"bp/q\",\"headRefOid\":\"h\"}'",
        )
        .within(Duration::from_millis(300));
        let start = Instant::now();
        let e = gh.login().unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
        assert_eq!((e.code, e.subject.as_str()), (Code::CouldNotRun, "gh api user"));
        assert!(e.fix == "gh did not answer within 300ms", "{}", e.fix);
        assert_eq!(gh.pr(1).unwrap().head_ref_oid, "h");
    }
}
