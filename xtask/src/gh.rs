//! The fake `gh` of `cargo xtask forge`, run as `xtask gh <T> <args>` from the shim `T/bin/gh` (26, issue 1). It
//! answers the six calls of `bp-forge` and `pr review --approve`, and fails any other. Its login is `FAKE_GH_LOGIN`,
//! default `quokka-agent`. The branches live in the bare copy `T/origin.git`; the pull requests live in `T/forge`, a
//! snapshot directory of 24, so a block reads or edits them as files. It never reaches the network.
//!
//! One call at a time holds `T/forge/lock`, as the forge serialises its own writes. Without it, six `pr create` calls
//! at once made one pull request, and a reader saw half a file. Each file is written by a rename, and only when it
//! changed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

/// Prints what `gh` prints on success; on failure, the error on stderr and exit 1, as `gh` does.
pub fn run(args: &[String]) -> ExitCode {
    let Some((t, args)) = args.split_first() else {
        eprintln!("usage: xtask gh <forge directory> <gh arguments>");
        return ExitCode::from(4);
    };
    let fake = Fake { t: PathBuf::from(t), login: std::env::var("FAKE_GH_LOGIN").unwrap_or("quokka-agent".into()) };
    match fake.call(&args.iter().map(String::as_str).collect::<Vec<_>>()) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::from(1)
        }
    }
}

/// GitHub's own words for a self-approval (15, E3).
const OWN_APPROVAL: &str =
    "failed to create review: GraphQL: Can not approve your own pull request (addPullRequestReview)";

/// A timestamp for the fields the fake has no real time for, so `pr view` matches `gh`'s shape key for key.
const EPOCH: &str = "1970-01-01T00:00:00Z";

/// The fields a pull request holds here, so `--json` fails an unknown field before it reads a row, as `gh` does.
const STORED: [&str; 11] = [
    "number",
    "url",
    "state",
    "title",
    "body",
    "author",
    "baseRefName",
    "headRefName",
    "headRefOid",
    "reviews",
    "statusCheckRollup",
];

struct Fake {
    t: PathBuf,
    login: String,
}

impl Fake {
    fn call(&self, args: &[&str]) -> Result<String> {
        let lock =
            std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.t.join("forge/lock"))?;
        lock.lock().context("lock the fake forge")?;
        let heads = self.heads()?;
        let mut prs = self.load()?;
        self.refresh(&mut prs, &heads)?;
        // Saved before the call too, so a block's snapshot read sees the refresh even when the call fails.
        self.save(&prs)?;
        let out = match args {
            ["api", "user"] => format!("{}\n", json!({ "login": self.login })),
            ["pr", "list", rest @ ..] => {
                let f = flags(rest, &["--head", "--state", "--json"], &[])?;
                let fields = fields(&f)?;
                let state = f.get("--state").copied().unwrap_or("open").to_ascii_uppercase();
                let rows: Vec<Value> = prs
                    .iter()
                    .filter(|p| state == "ALL" || p["state"] == state.as_str())
                    .filter(|p| f.get("--head").is_none_or(|h| p["headRefName"] == *h))
                    .map(|p| select(p, &fields))
                    .collect();
                format!("{}\n", Value::Array(rows))
            }
            ["pr", "view", n, rest @ ..] => {
                let fields = fields(&flags(rest, &["--json"], &[])?)?;
                format!("{}\n", select(&prs[find(&prs, n)?], &fields))
            }
            ["pr", "create", rest @ ..] => {
                let f = flags(rest, &["--head", "--base", "--title", "--body"], &[])?;
                let (head, base) = (need(&f, "--head")?, need(&f, "--base")?);
                let oid = heads.get(head).with_context(|| format!("pull request create failed: push {head} first"))?;
                if !heads.contains_key(base) {
                    bail!("pull request create failed: base branch {base} does not exist");
                }
                if let Some(p) = prs.iter().find(|p| p["state"] == "OPEN" && p["headRefName"] == head) {
                    bail!(
                        "a pull request for branch \"{head}\" into branch \"{base}\" already exists:\n{}",
                        p["url"].as_str().unwrap_or_default()
                    );
                }
                let number = prs.iter().filter_map(|p| p["number"].as_u64()).max().unwrap_or(0) + 1;
                let url = format!("{}/pull/{number}", self.repository()?);
                let mut pr = json!({
                    "number": number, "url": url, "state": "OPEN",
                    "title": need(&f, "--title")?, "body": need(&f, "--body")?,
                    "author": { "id": format!("U_{}", self.login), "is_bot": false, "login": self.login, "name": self.login },
                    "baseRefName": base, "headRefName": head, "headRefOid": oid,
                    "reviews": [], "statusCheckRollup": self.rollup(oid)?, "pushes": [],
                });
                pushed(&mut pr)?;
                prs.push(pr);
                format!("{url}\n")
            }
            ["pr", "edit", n, rest @ ..] => {
                let f = flags(rest, &["--base", "--title", "--body"], &[])?;
                let i = find(&prs, n)?;
                let pr = &mut prs[i];
                if let Some(base) = f.get("--base") {
                    if !heads.contains_key(*base) {
                        bail!("could not edit: base branch {base} does not exist");
                    }
                    pr["baseRefName"] = (*base).into();
                }
                for (flag, field) in [("--title", "title"), ("--body", "body")] {
                    if let Some(v) = f.get(flag) {
                        pr[field] = (*v).into();
                    }
                }
                format!("{}\n", pr["url"].as_str().unwrap_or_default())
            }
            ["pr", "merge", n, rest @ ..] => {
                let f = flags(rest, &["--match-head-commit"], &["--squash"])?;
                if !f.contains_key("--squash") {
                    bail!("the fake gh merges with --squash only");
                }
                let i = find(&prs, n)?;
                self.merge(&mut prs[i], &heads, f.get("--match-head-commit").copied())?;
                String::new()
            }
            ["pr", "review", n, rest @ ..] => {
                let f = flags(rest, &["--body"], &["--approve"])?;
                if !f.contains_key("--approve") {
                    bail!("the fake gh reviews with --approve only");
                }
                let i = find(&prs, n)?;
                self.approve(&mut prs[i])?;
                String::new()
            }
            _ => bail!(
                "unknown command \"{}\": the fake gh answers `api user` and `pr list|view|create|edit|merge|review`",
                args.join(" ")
            ),
        };
        self.save(&prs)?;
        Ok(out)
    }

    /// Refreshes every open pull request from the bare copy and its rollup from `checks/<head>.json`, first on each
    /// call, as the forge sees a push, a deleted branch and a finished check run without being told.
    fn refresh(&self, prs: &mut [Value], heads: &BTreeMap<String, String>) -> Result<()> {
        for pr in prs.iter_mut().filter(|p| p["state"] == "OPEN") {
            // GitHub closes a pull request whose base branch is deleted, for good (15, E7).
            if !pr["baseRefName"].as_str().is_some_and(|b| heads.contains_key(b)) {
                pr["state"] = "CLOSED".into();
                continue;
            }
            if let Some(head) = pr["headRefName"].as_str().and_then(|b| heads.get(b))
                && pr["headRefOid"] != head.as_str()
            {
                pr["headRefOid"] = head.as_str().into();
                pushed(pr)?;
            }
            pr["statusCheckRollup"] = self.rollup(pr["headRefOid"].as_str().unwrap_or_default())?;
        }
        Ok(())
    }

    /// Squashes the head onto the base in the bare copy. Like branch protection, it refuses a base that is not an
    /// ancestor of the head and a rollup that is not green, and needs an approval of the latest push ([`approved`]).
    fn merge(&self, pr: &mut Value, heads: &BTreeMap<String, String>, expect: Option<&str>) -> Result<()> {
        let n = &pr["number"];
        if pr["state"] != "OPEN" {
            bail!("Pull request #{n} is not open");
        }
        let head = pr["headRefOid"].as_str().unwrap_or_default().to_string();
        if expect.is_some_and(|e| e != head) {
            bail!("Pull request #{n} is not mergeable: Head branch was modified. Review and try the merge again.");
        }
        let base_ref = pr["baseRefName"].as_str().unwrap_or_default().to_string();
        let base = heads.get(&base_ref).with_context(|| format!("base branch {base_ref} does not exist"))?;
        if !self.git_cmd(&["merge-base", "--is-ancestor", base, &head]).status().context("run git")?.success() {
            bail!("Pull request #{n} is not mergeable: the head branch is not up to date with the base branch.");
        }
        if !green(&pr["statusCheckRollup"]) {
            bail!("Pull request #{n} is not mergeable: Required status check \"fake\" is expected.");
        }
        if !approved(pr) {
            bail!("Pull request #{n} is not mergeable: the base branch policy prohibits the merge.");
        }
        let message = format!("{} (#{n})", pr["title"].as_str().unwrap_or_default());
        let squash = self.git(&["commit-tree", &format!("{head}^{{tree}}"), "-p", base, "-m", &message])?;
        self.git(&["update-ref", &format!("refs/heads/{base_ref}"), &squash, base])?;
        pr["state"] = "MERGED".into();
        Ok(())
    }

    /// Records the login's approval at the head. GitHub documents no refusal of an approval on an empty diff, so the
    /// fake accepts one, as when a later commit undoes the change (26, issue 5).
    fn approve(&self, pr: &mut Value) -> Result<()> {
        if pr["state"] != "OPEN" {
            bail!("Pull request #{} is not open", pr["number"]);
        }
        if pr["author"]["login"] == self.login.as_str() {
            bail!("{OWN_APPROVAL}");
        }
        let head = pr["headRefOid"].as_str().unwrap_or_default().to_string();
        let date = Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]).output().context("run date")?;
        let reviews = pr["reviews"].as_array_mut().context("reviews is a list")?;
        reviews.push(json!({
            "id": format!("PRR_fake{}", reviews.len() + 1), "author": { "login": self.login },
            "authorAssociation": "MEMBER", "body": "", "submittedAt": String::from_utf8_lossy(&date.stdout).trim(),
            "includesCreatedEdit": false, "reactionGroups": [], "state": "APPROVED", "commit": { "oid": head },
        }));
        Ok(())
    }

    /// The checks at `head`, each filled out to `gh`'s shape: a check run, or a status context when the entry has a
    /// `state` and no `conclusion`.
    fn rollup(&self, head: &str) -> Result<Value> {
        let path = self.t.join(format!("forge/checks/{head}.json"));
        let runs: Vec<Map<String, Value>> = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let run = json!({
            "__typename": "CheckRun", "completedAt": EPOCH, "conclusion": "", "detailsUrl": "", "name": "fake",
            "startedAt": EPOCH, "status": "COMPLETED", "workflowName": "fake",
        });
        let context = json!({ "__typename": "StatusContext", "context": "fake", "startedAt": EPOCH, "state": "", "targetUrl": "" });
        Ok(runs
            .into_iter()
            .map(|given| {
                let template =
                    if given.contains_key("state") && !given.contains_key("conclusion") { &context } else { &run };
                let mut check = template.as_object().cloned().unwrap_or_default();
                check.extend(given);
                Value::Object(check)
            })
            .collect())
    }

    /// Every `pr-<n>.json`, by number.
    fn load(&self) -> Result<Vec<Value>> {
        let mut prs = Vec::new();
        for entry in std::fs::read_dir(self.t.join("forge"))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name.starts_with("pr-") && path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path)?;
                prs.push(serde_json::from_str::<Value>(&text).with_context(|| format!("parse {}", path.display()))?);
            }
        }
        prs.sort_by_key(|p| p["number"].as_u64());
        Ok(prs)
    }

    /// Writes each pull request back and rewrites `open-prs.json`.
    fn save(&self, prs: &[Value]) -> Result<()> {
        for pr in prs {
            write(&self.t.join(format!("forge/pr-{}.json", pr["number"])), &format!("{pr}\n"))?;
        }
        let open: Vec<&Value> = prs.iter().filter(|p| p["state"] == "OPEN").collect();
        write(&self.t.join("forge/open-prs.json"), &format!("{}\n", json!(open)))
    }

    /// Each branch of the bare copy and its commit.
    fn heads(&self) -> Result<BTreeMap<String, String>> {
        let out = self.git(&["for-each-ref", "--format=%(refname:lstrip=2) %(objectname)", "refs/heads"])?;
        Ok(out.lines().filter_map(|l| l.split_once(' ')).map(|(b, o)| (b.to_string(), o.to_string())).collect())
    }

    /// `T/code`'s `origin` url as the forge prints it.
    fn repository(&self) -> Result<String> {
        let code = self.t.join("code").display().to_string();
        let url = self.git_cmd(&["-C", &code, "config", "remote.origin.url"]).output()?;
        let url = String::from_utf8_lossy(&url.stdout);
        Ok(url.trim().trim_end_matches(".git").to_string())
    }

    fn git_cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        // `file` alone: the bare copy is a local path, and nothing the fake runs may reach the network.
        cmd.args(args).current_dir(self.t.join("origin.git")).env("GIT_ALLOW_PROTOCOL", "file");
        // The squash is the forge's commit, as GitHub's is; a runner with no global git identity cannot make it.
        for (k, v) in [("NAME", "GitHub"), ("EMAIL", "noreply@github.com")] {
            cmd.env(format!("GIT_AUTHOR_{k}"), v).env(format!("GIT_COMMITTER_{k}"), v);
        }
        cmd
    }

    /// Stdout of a git call in the bare copy that exited 0, trimmed.
    fn git(&self, args: &[&str]) -> Result<String> {
        let out = self.git_cmd(args).output().context("run git")?;
        if !out.status.success() {
            bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }
}

/// Records a push to the head: its pusher and the reviews before it. The fake cannot see who ran `git push`; the
/// agent's token, the author's, makes every push of a pull request's head, `land`'s restack included.
fn pushed(pr: &mut Value) -> Result<()> {
    let reviews = pr["reviews"].as_array().map_or(0, Vec::len);
    let by = pr["author"]["login"].clone();
    pr["pushes"].as_array_mut().context("pushes is a list")?.push(json!({ "by": by, "reviews": reviews }));
    Ok(())
}

/// A login other than the author and the latest push's pusher approved after that push. With "Require approval of
/// the most recent reviewable push" and "Dismiss stale pull request approvals" on, an approval before a push counts
/// for nothing, even when the push keeps the diff (15, Controls 1).
fn approved(pr: &Value) -> bool {
    let last = pr["pushes"].as_array().and_then(|p| p.last());
    let since = last.and_then(|p| p["reviews"].as_u64()).and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
    let pusher = last.map(|p| &p["by"]);
    pr["reviews"].as_array().into_iter().flatten().skip(since).any(|r| {
        let login = &r["author"]["login"];
        r["state"] == "APPROVED" && login.is_string() && *login != pr["author"]["login"] && Some(login) != pusher
    })
}

/// Green as `bp-forge` reads it (15, Controls 3): at least one `SUCCESS`, and every check `SUCCESS`, `NEUTRAL` or
/// `SKIPPED`. A pending check run has an empty conclusion. Written again here, so the oracle does not share the code
/// it tests.
fn green(rollup: &Value) -> bool {
    let verdicts: Vec<&str> = rollup
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| c["conclusion"].as_str().filter(|s| !s.is_empty()).or(c["state"].as_str()).unwrap_or_default())
        .collect();
    verdicts.contains(&"SUCCESS") && verdicts.iter().all(|v| ["SUCCESS", "NEUTRAL", "SKIPPED"].contains(v))
}

/// Writes `text` unless the file holds it already, through a rename, so a reader never sees half a file.
fn write(path: &Path, text: &str) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename onto {}", path.display()))
}

/// `--name value` pairs and bare `--name` switches. Any other word fails, so a call the fake does not model fails
/// instead of half-working.
fn flags<'a>(words: &[&'a str], valued: &[&str], switches: &[&str]) -> Result<BTreeMap<&'a str, &'a str>> {
    let mut out = BTreeMap::new();
    let mut words = words.iter();
    while let Some(&w) = words.next() {
        if valued.contains(&w) {
            out.insert(w, *words.next().with_context(|| format!("flag needs an argument: {w}"))?);
        } else if switches.contains(&w) {
            out.insert(w, "");
        } else {
            bail!("unknown flag or argument: {w}");
        }
    }
    Ok(out)
}

fn need<'a>(f: &BTreeMap<&str, &'a str>, flag: &str) -> Result<&'a str> {
    f.get(flag).copied().with_context(|| format!("the fake gh needs {flag}"))
}

/// The `--json` fields, each one the fake stores.
fn fields<'a>(f: &BTreeMap<&str, &'a str>) -> Result<Vec<&'a str>> {
    let fields: Vec<&str> = need(f, "--json")?.split(',').collect();
    if let Some(bad) = fields.iter().find(|x| !STORED.contains(x)) {
        bail!("Unknown JSON field: {bad:?}");
    }
    Ok(fields)
}

fn find(prs: &[Value], n: &str) -> Result<usize> {
    let number: u64 = n.parse().with_context(|| format!("the fake gh takes a pull request by number, not {n:?}"))?;
    prs.iter()
        .position(|p| p["number"] == number)
        .with_context(|| format!("GraphQL: Could not resolve to a PullRequest with the number of {number}."))
}

/// The `--json` fields of one pull request, in `gh`'s shape.
fn select(pr: &Value, fields: &[&str]) -> Value {
    Value::Object(fields.iter().map(|&f| (f.to_string(), pr[f].clone())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_or_field_the_fake_does_not_model_fails() {
        let f = flags(&["--head", "bp/q", "--squash"], &["--head"], &["--squash"]).unwrap();
        assert_eq!((f["--head"], f.contains_key("--squash")), ("bp/q", true));
        assert!(flags(&["--merge"], &[], &["--squash"]).is_err(), "an unmodelled switch");
        assert!(flags(&["--head"], &["--head"], &[]).is_err(), "a flag with no value");
        assert!(fields(&flags(&[], &["--json"], &[]).unwrap()).is_err(), "no --json, even with no rows");
        let f = flags(&["--json", "headRefOid"], &["--json"], &[]).unwrap();
        assert_eq!(
            select(&json!({ "number": 1, "headRefOid": "h" }), &fields(&f).unwrap()),
            json!({ "headRefOid": "h" })
        );
        let f = flags(&["--json", "number,mergeable"], &["--json"], &[]).unwrap();
        assert!(fields(&f).is_err(), "an unknown field fails as in gh");
    }

    #[test]
    fn green_passes_success_neutral_and_skipped_only() {
        let g = |checks: Value| green(&checks);
        assert!(g(json!([{ "conclusion": "SUCCESS" }, { "conclusion": "SKIPPED" }, { "conclusion": "NEUTRAL" }])));
        assert!(g(json!([{ "conclusion": "", "state": "SUCCESS" }])), "a status context");
        assert!(!g(json!([])) && !g(json!([{ "conclusion": "SKIPPED" }])), "no success");
        assert!(!g(json!([{ "conclusion": "SUCCESS" }, { "conclusion": "", "status": "IN_PROGRESS" }])), "pending");
        assert!(!g(json!([{ "conclusion": "SUCCESS" }, { "conclusion": "FAILURE" }])));
        assert!(!g(json!([{ "conclusion": "SUCCESS" }, { "state": "PENDING" }])));
    }

    #[test]
    fn concurrent_calls_serialise() {
        const N: usize = 6;
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().canonicalize().unwrap();
        crate::forge::make(&t, "bp", "https://github.invalid/o/r").unwrap();
        let code = t.join("code");
        for i in 0..N {
            let branch = format!("bp/c{i}");
            crate::forge::git(&code, &["checkout", "-q", "-B", &branch, "master"]).unwrap();
            std::fs::write(code.join("README.md"), format!("{i}\n")).unwrap();
            crate::forge::git(&code, &["commit", "-q", "--no-verify", "-am", &branch]).unwrap();
            crate::forge::git(&code, &["push", "-q", "--no-verify", "origin", &branch]).unwrap();
        }
        let fake = |login: String| Fake { t: t.clone(), login };
        let all = |calls: Vec<(String, Vec<String>)>| {
            std::thread::scope(|s| {
                let runs: Vec<_> = calls
                    .into_iter()
                    .map(|(login, args)| {
                        let fake = fake(login);
                        s.spawn(move || fake.call(&args.iter().map(String::as_str).collect::<Vec<_>>()))
                    })
                    .collect();
                runs.into_iter().map(|r| r.join().unwrap().map_err(|e| format!("{e:#}"))).collect::<Vec<_>>()
            })
        };
        let create = |i: usize| -> Vec<String> {
            ["pr", "create", "--head", &format!("bp/c{i}"), "--base", "master", "--title", "t", "--body", "b"]
                .map(String::from)
                .to_vec()
        };
        let made = all((0..N).map(|i| ("quokka-agent".to_string(), create(i))).collect());
        assert!(made.iter().all(Result::is_ok), "{made:?}");
        let prs = fake("quokka-agent".into()).load().unwrap();
        let numbers: Vec<_> = prs.iter().map(|p| p["number"].as_u64().unwrap()).collect();
        assert_eq!(numbers, (1..=N as u64).collect::<Vec<_>>(), "one pull request per call");

        let review = ["pr", "review", "1", "--approve"].map(String::from).to_vec();
        let approved = all((0..N).map(|i| (format!("reviewer-{i}"), review.clone())).collect());
        assert!(approved.iter().all(Result::is_ok), "{approved:?}");
        let pr = &fake("quokka-agent".into()).load().unwrap()[0];
        assert_eq!(pr["reviews"].as_array().unwrap().len(), N, "one review per call");
    }

    #[test]
    fn a_push_after_the_approval_needs_a_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().canonicalize().unwrap();
        crate::forge::make(&t, "bp", "https://github.invalid/o/r").unwrap();
        let code = t.join("code");
        let commit = |text: &str| {
            std::fs::write(code.join("README.md"), text).unwrap();
            crate::forge::git(&code, &["commit", "-q", "--no-verify", "-am", text]).unwrap();
            crate::forge::git(&code, &["push", "-q", "--no-verify", "origin", "bp/p"]).unwrap();
            let head = Command::new("git").arg("-C").arg(&code).args(["rev-parse", "HEAD"]).output().unwrap();
            String::from_utf8(head.stdout).unwrap().trim().to_string()
        };
        crate::forge::git(&code, &["checkout", "-q", "-b", "bp/p"]).unwrap();
        let h1 = commit("one\n");
        let call = |login: &str, args: &[&str]| Fake { t: t.clone(), login: login.into() }.call(args);
        call("quokka-agent", &["pr", "create", "--head", "bp/p", "--base", "master", "--title", "t", "--body", "b"])
            .unwrap();
        let approve = || call("quokka-reviewer", &["pr", "review", "1", "--approve"]).unwrap();
        approve();
        let h2 = commit("two\n");
        for h in [&h1, &h2] {
            std::fs::write(t.join(format!("forge/checks/{h}.json")), r#"[{"conclusion":"SUCCESS"}]"#).unwrap();
        }
        let merge = || call("quokka-agent", &["pr", "merge", "1", "--squash"]);
        let refused = merge().unwrap_err().to_string();
        assert!(refused.contains("base branch policy prohibits the merge"), "{refused}");
        approve();
        merge().unwrap();
        assert_eq!(
            call("quokka-agent", &["pr", "view", "1", "--json", "state"]).unwrap().trim(),
            r#"{"state":"MERGED"}"#
        );
    }
}
