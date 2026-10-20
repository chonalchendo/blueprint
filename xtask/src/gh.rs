//! The fake `gh` of `cargo xtask forge`, run as `xtask gh <T> <args>` from the shim `T/bin/gh` (26, issue 1). It
//! answers the six calls of `bp-forge` and `pr review --approve`, and fails any other. Its login is `FAKE_GH_LOGIN`,
//! default `quokka-agent`. The branches live in the bare copy `T/origin.git`; the pull requests live in `T/forge`, a
//! snapshot directory of 24, so a block reads or edits them as files. It never reaches the network.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

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

struct Fake {
    t: PathBuf,
    login: String,
}

impl Fake {
    fn call(&self, args: &[&str]) -> Result<String> {
        let heads = self.heads()?;
        let mut prs = self.load()?;
        // Each call first refreshes every open head from the bare copy and its rollup from `checks/<head>.json`, as
        // the forge sees a push and a finished check run without being told.
        for pr in prs.iter_mut().filter(|p| p["state"] == "OPEN") {
            if let Some(head) = pr["headRefName"].as_str().and_then(|b| heads.get(b)) {
                pr["headRefOid"] = head.as_str().into();
            }
            pr["statusCheckRollup"] = self.rollup(pr["headRefOid"].as_str().unwrap_or_default())?;
        }
        // Saved before the call too, so a block's snapshot read sees the refresh even when the call fails.
        self.save(&prs)?;
        let out = match args {
            ["api", "user"] => format!("{}\n", json!({ "login": self.login })),
            ["pr", "list", rest @ ..] => {
                let f = flags(rest, &["--head", "--state", "--json"], &[])?;
                let state = f.get("--state").copied().unwrap_or("open").to_ascii_uppercase();
                let rows: Vec<Value> = prs
                    .iter()
                    .filter(|p| state == "ALL" || p["state"] == state.as_str())
                    .filter(|p| f.get("--head").is_none_or(|h| p["headRefName"] == *h))
                    .map(|p| select(p, need(&f, "--json")?))
                    .collect::<Result<_>>()?;
                format!("{}\n", Value::Array(rows))
            }
            ["pr", "view", n, rest @ ..] => {
                let f = flags(rest, &["--json"], &[])?;
                format!("{}\n", select(&prs[find(&prs, n)?], need(&f, "--json")?)?)
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
                prs.push(json!({
                    "number": number, "url": url, "state": "OPEN",
                    "title": need(&f, "--title")?, "body": need(&f, "--body")?,
                    "author": { "id": format!("U_{}", self.login), "is_bot": false, "login": self.login, "name": self.login },
                    "baseRefName": base, "headRefName": head, "headRefOid": oid,
                    "reviews": [], "statusCheckRollup": self.rollup(oid)?,
                }));
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
                self.approve(&mut prs[i], &heads)?;
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

    /// Squashes the head onto the base in the bare copy. Like branch protection, it refuses a base that is not an
    /// ancestor of the head, and needs another login's approval at the head, or at an earlier head with the head's
    /// patch id (15, E7).
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
        if !self.git_ok(&["merge-base", "--is-ancestor", base, &head])? {
            bail!("Pull request #{n} is not mergeable: the head branch is not up to date with the base branch.");
        }
        if !self.approved(pr, base, &head)? {
            bail!("Pull request #{n} is not mergeable: the base branch policy prohibits the merge.");
        }
        let message = format!("{} (#{n})", pr["title"].as_str().unwrap_or_default());
        let squash = self.git(&["commit-tree", &format!("{head}^{{tree}}"), "-p", base, "-m", &message])?;
        self.git(&["update-ref", &format!("refs/heads/{base_ref}"), &squash, base])?;
        pr["state"] = "MERGED".into();
        Ok(())
    }

    /// Records the login's approval at the head, with the head's patch id for a later head to match.
    fn approve(&self, pr: &mut Value, heads: &BTreeMap<String, String>) -> Result<()> {
        if pr["state"] != "OPEN" {
            bail!("Pull request #{} is not open", pr["number"]);
        }
        if pr["author"]["login"] == self.login.as_str() {
            bail!("{OWN_APPROVAL}");
        }
        let head = pr["headRefOid"].as_str().unwrap_or_default().to_string();
        if let Some(base) = pr["baseRefName"].as_str().and_then(|b| heads.get(b)) {
            let mut ids = self.patch_ids()?;
            ids.insert(head.clone(), self.patch_id(base, &head)?.into());
            std::fs::write(self.t.join("forge/patch-ids.json"), Value::Object(ids).to_string())?;
        }
        let at = now()?;
        let reviews = pr["reviews"].as_array_mut().context("reviews is a list")?;
        reviews.push(json!({
            "id": format!("PRR_fake{}", reviews.len() + 1), "author": { "login": self.login },
            "authorAssociation": "MEMBER", "body": "", "submittedAt": at, "includesCreatedEdit": false,
            "reactionGroups": [], "state": "APPROVED", "commit": { "oid": head },
        }));
        Ok(())
    }

    fn approved(&self, pr: &Value, base: &str, head: &str) -> Result<bool> {
        let author = pr["author"]["login"].as_str();
        let at: Vec<&str> = pr["reviews"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|r| r["state"] == "APPROVED" && r["author"]["login"].as_str().is_some_and(|l| Some(l) != author))
            .filter_map(|r| r["commit"]["oid"].as_str())
            .collect();
        if at.contains(&head) || at.is_empty() {
            return Ok(!at.is_empty());
        }
        let (ids, now) = (self.patch_ids()?, self.patch_id(base, head)?);
        Ok(at.iter().any(|oid| ids.get(*oid).and_then(Value::as_str) == Some(now.as_str())))
    }

    /// `git patch-id` of the diff from the merge base to `head`: equal for a rebase that keeps the change.
    fn patch_id(&self, base: &str, head: &str) -> Result<String> {
        let diff = self.git(&["diff", &format!("{base}...{head}")])?;
        let mut child = self
            .git_cmd(&["patch-id", "--stable"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("run git patch-id")?;
        child.stdin.take().context("patch-id stdin")?.write_all(format!("{diff}\n").as_bytes())?;
        let out = child.wait_with_output()?;
        Ok(String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap_or_default().to_string())
    }

    fn patch_ids(&self) -> Result<Map<String, Value>> {
        match std::fs::read_to_string(self.t.join("forge/patch-ids.json")) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(e.into()),
        }
    }

    /// The check runs at `head`, each filled out to `gh`'s check run shape.
    fn rollup(&self, head: &str) -> Result<Value> {
        let path = self.t.join(format!("forge/checks/{head}.json"));
        let runs: Vec<Map<String, Value>> = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let template = json!({
            "__typename": "CheckRun", "completedAt": EPOCH, "conclusion": "", "detailsUrl": "", "name": "fake",
            "startedAt": EPOCH, "status": "COMPLETED", "workflowName": "fake",
        });
        Ok(runs
            .into_iter()
            .map(|given| {
                let mut run = template.as_object().cloned().unwrap_or_default();
                run.extend(given);
                Value::Object(run)
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
            std::fs::write(self.t.join(format!("forge/pr-{}.json", pr["number"])), format!("{pr}\n"))?;
        }
        let open: Vec<&Value> = prs.iter().filter(|p| p["state"] == "OPEN").collect();
        std::fs::write(self.t.join("forge/open-prs.json"), format!("{}\n", json!(open)))?;
        Ok(())
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

    /// Whether a git call that answers by its exit code said yes.
    fn git_ok(&self, args: &[&str]) -> Result<bool> {
        Ok(self.git_cmd(args).status().context("run git")?.success())
    }
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

fn now() -> Result<String> {
    let out = Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]).output().context("run date")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn need<'a>(f: &BTreeMap<&str, &'a str>, flag: &str) -> Result<&'a str> {
    f.get(flag).copied().with_context(|| format!("the fake gh needs {flag}"))
}

fn find(prs: &[Value], n: &str) -> Result<usize> {
    let number: u64 = n.parse().with_context(|| format!("the fake gh takes a pull request by number, not {n:?}"))?;
    prs.iter()
        .position(|p| p["number"] == number)
        .with_context(|| format!("GraphQL: Could not resolve to a PullRequest with the number of {number}."))
}

/// The `--json` fields of one pull request, in `gh`'s shape.
fn select(pr: &Value, fields: &str) -> Result<Value> {
    let mut out = Map::new();
    for field in fields.split(',') {
        out.insert(field.into(), pr.get(field).with_context(|| format!("Unknown JSON field: {field:?}"))?.clone());
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flag_the_fake_does_not_model_fails() {
        let f = flags(&["--head", "bp/q", "--squash"], &["--head"], &["--squash"]).unwrap();
        assert_eq!((f["--head"], f.contains_key("--squash")), ("bp/q", true));
        assert!(flags(&["--merge"], &[], &["--squash"]).is_err(), "an unmodelled switch");
        assert!(flags(&["--head"], &["--head"], &[]).is_err(), "a flag with no value");
        let pr = json!({ "number": 1, "headRefOid": "h" });
        assert_eq!(select(&pr, "headRefOid").unwrap(), json!({ "headRefOid": "h" }));
        assert!(select(&pr, "mergeable").is_err(), "an unknown field fails as in gh");
    }
}
