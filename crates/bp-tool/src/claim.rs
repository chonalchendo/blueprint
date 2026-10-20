//! `bp cut <issue>` and `bp release <issue>`: a claim opens and closes, each in one commit with `Bp-Claim` (15, the
//! repo verbs; 26, design changes 2 and 7). `cut` runs in a clone of the project repository. It fetches, adds the
//! worktree and the branch at their fixed names, then commits the claim and the issue at `in-progress`. `release`
//! appends the claim's `## Released` and moves the issue back to `open`; the worktree stays for the next claim.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use bp_change::intents;
use bp_vault::git::Repo;
use bp_vault::{Code, Header, Id, Index, Kind, Record, Refusal, Vault};
use clap::Args;
use serde_json::{Value, json};

use crate::{Wrote, node_id, subject};

/// `bp cut <issue> [--base <ref>]`.
#[derive(Args)]
pub struct Cut {
    /// The issue, for example `projects/bp/issues/cut-and-release`.
    pub issue: String,
    /// The commit to cut from, any ref git resolves; origin's default branch when absent.
    #[arg(long)]
    pub base: Option<String>,
}

/// `bp release <issue> --reason <escalated|dropped|lost>`.
#[derive(Args)]
pub struct Release {
    /// The issue whose active claim closes.
    pub issue: String,
    /// Why: `escalated` blocks the issue until a write touches it (21); `landed` is `land`'s own.
    #[arg(long, value_parser = ["escalated", "dropped", "lost"])]
    pub reason: String,
}

pub(crate) fn cut(cwd: &Path, c: &Cut) -> Result<Wrote, Refusal> {
    node_id(&c.issue)?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let session = std::env::var("BP_SESSION").ok().filter(|s| !s.is_empty());
    let now = crate::clock::now()?;
    // The lock comes before the reads and stays through the fetch, so no second `cut` claims between the check and
    // the commit.
    let mut writer = vault.writer()?;
    let id = subject(writer.index(), &c.issue)?;
    let from = crate::clock::instant(now);
    let claim = crate::record::free(&vault.root, &format!("{id}/{}/{}", Kind::Claim.dir(), from.replace(':', "-")));
    writer.clean([&id, &claim])?;
    let text = vault.text(&id)?;
    let file = bp_vault::File::from_bytes(id.clone(), text.as_bytes())?;
    let (project, slug) = (id.project().unwrap_or_default(), id.as_str().rsplit('/').next().unwrap_or_default());
    let branch = format!("{project}/{slug}");
    if let Some(repeat) = gate(writer.index(), &id, &file, role.as_str(), session.as_deref(), &branch)? {
        return Ok(Wrote::Already(repeat));
    }
    let top = clone_of(cwd, writer.index(), project)?;
    let worktree = place(&top, slug)?;
    git(&top, &["fetch", "-q", "origin"])?;
    let start = c.base.as_deref().unwrap_or("origin/HEAD");
    let base = git(&top, &["rev-parse", "--verify", "-q", "--end-of-options", &format!("{start}^{{commit}}")])
        .map_err(|e| match &c.base {
            Some(b) => Refusal::new(
                Code::UnknownValue,
                format!("--base {b}"),
                "a value comes from the refs git resolves",
                "name a commit, branch or tag the clone holds: git fetch, then rerun",
            ),
            None => Refusal::failed(e.subject, "origin has no default branch here: git remote set-head origin --auto"),
        })?;
    let reuse = checked_out(&worktree).as_deref() == Some(&format!("refs/heads/{branch}"));
    // A kept worktree carries its commits: its base is where its branch left the start, not the start's head.
    let base = if reuse {
        git(&worktree, &["merge-base", "HEAD", &base])?
    } else {
        let path = worktree.display().to_string();
        git(&top, &["worktree", "add", "-q", "--no-track", "-b", &branch, &path, &base])?;
        base
    };
    let mut fields = BTreeMap::from([
        ("from".to_string(), Value::from(from)),
        ("owner".into(), role.as_str().into()),
        ("worktree".into(), worktree.display().to_string().into()),
        ("base".into(), base.clone().into()),
    ]);
    if let Some(s) = &session {
        fields.insert("session".into(), s.as_str().into());
    }
    let header = Header {
        kind: Kind::Claim,
        title: None,
        status: None,
        under: Some(id.clone()),
        aliases: Vec::new(),
        links: BTreeMap::new(),
        unresolved: Vec::new(),
        fields,
    };
    let changes = BTreeMap::from([
        (claim.clone(), format!("---\n{}---\n", header.emit())),
        (id.clone(), bp_vault::set_field(&text, "status", &"in-progress".into())),
    ]);
    writer.trailer(format!("Bp-Claim: {claim}"));
    writer.at(now);
    let written = writer.commit("cut", role.as_str(), &format!("cut {id}"), &changes).inspect_err(|_| {
        // Nothing of a refused cut stays: the worktree and the branch it added go with the claim. Their own failure
        // is not the error the caller needs.
        if !reuse {
            let _ = git(&top, &["worktree", "remove", "--force", &worktree.display().to_string()]);
            let _ = git(&top, &["branch", "-D", &branch]);
        }
    })?;
    Ok(Wrote::Changed(json!({
        "id": id,
        "claim": claim,
        "status": "in-progress",
        "branch": branch,
        "worktree": worktree,
        "base": base,
        "commit": written.commit,
        "findings": written.findings,
    })))
}

pub(crate) fn release(cwd: &Path, r: &Release) -> Result<Wrote, Refusal> {
    node_id(&r.issue)?;
    let vault = Vault::discover(cwd)?;
    let role = crate::set::role()?;
    let now = crate::clock::now()?;
    let mut writer = vault.writer()?;
    let id = subject(writer.index(), &r.issue)?;
    let Some(h) = held(writer.index(), &id)? else { return Ok(Wrote::Already(json!({ "id": id }))) };
    let claim = Id::parse(&h.id).expect("the index holds ids");
    writer.clean([&id, &claim])?;
    let text = vault.text(&id)?;
    let to = crate::clock::instant(now);
    let released =
        format!("{}\n\n## Released\n\nto: {to}\nreason: {}\n", vault.text(&claim)?.trim_end_matches('\n'), r.reason);
    let mut changes = BTreeMap::from([(claim.clone(), released)]);
    // A converted claim may sit on an issue whose status never moved (21, check 2); only `in-progress` returns.
    let was = bp_vault::File::from_bytes(id.clone(), text.as_bytes())?.header.and_then(|h| h.status);
    if was.as_deref() == Some("in-progress") {
        changes.insert(id.clone(), bp_vault::set_field(&text, "status", &"open".into()));
    }
    writer.trailer(format!("Bp-Claim: {claim}"));
    writer.at(now);
    let written = writer.commit("release", role.as_str(), &format!("release {id} {}", r.reason), &changes)?;
    Ok(Wrote::Changed(json!({
        "id": id,
        "claim": claim,
        "reason": r.reason,
        "status": if changes.contains_key(&id) { Some("open") } else { was.as_deref() },
        "worktree": field(&h, "worktree"),
        "commit": written.commit,
        "findings": written.findings,
    })))
}

/// The claim and status `cut` starts from: the active claim's repeat (exit 3, its fields) or `already_claimed`, then
/// `illegal_transition` unless the issue is `open`.
fn gate(
    index: &Index,
    id: &Id,
    file: &bp_vault::File,
    role: &str,
    session: Option<&str>,
    branch: &str,
) -> Result<Option<Value>, Refusal> {
    if let Some(h) = held(index, id)? {
        let (owner, held_session) = (field(&h, "owner"), field(&h, "session"));
        if owner.as_deref() == Some(role) && held_session.as_deref() == session {
            return Ok(Some(json!({ "id": id, "claim": h.id, "branch": branch, "worktree": field(&h, "worktree") })));
        }
        let by = owner.unwrap_or_default();
        let by = held_session.map_or(by.clone(), |s| format!("{by}, session {s}"));
        return Err(Refusal::new(
            Code::AlreadyClaimed,
            id.as_str(),
            "at most one claim per issue is held (13, Claim); a repeat from the same owner and session exits 3",
            format!("{} is held by {by}; bp release {id} --reason <escalated|dropped|lost>, then bp cut {id}", h.id),
        ));
    }
    let status = file.header.as_ref().filter(|h| h.kind == Kind::Issue).and_then(|h| h.status.clone());
    if status.as_deref() != Some("open") {
        let fix = match status.as_deref() {
            Some("abandoned") => format!("bp transition {id} open, then bp cut {id}"),
            Some("in-progress") => {
                format!("{id} is in-progress with no active claim (21, check 2); no verb moves it yet")
            }
            Some(s) => format!("an issue at {s} is not claimed again; bp show {id}"),
            None => format!("cut claims an issue; bp show {id}"),
        };
        return Err(Refusal::new(
            Code::IllegalTransition,
            format!("{id} in-progress"),
            "cut moves an issue from open to in-progress (25, design change 3)",
            fix,
        ));
    }
    Ok(None)
}

/// The top of the clone of `project`'s repository that holds `cwd`: a repo verb runs nowhere else (26, design change
/// 2).
fn clone_of(cwd: &Path, index: &Index, project: &str) -> Result<PathBuf, Refusal> {
    let repository = index
        .header(&format!("projects/{project}/project"))?
        .and_then(|h| h.get("repository")?.as_str().map(str::to_string))
        .unwrap_or_default();
    Repo::find(cwd)
        .filter(|r| r.remote().is_some_and(|url| bp_vault::git::same_repository(&url, &repository)))
        .map(|r| r.top)
        .ok_or_else(|| {
            Refusal::failed(
                cwd.display().to_string(),
                format!("a repo verb runs in a clone of {project}'s repository; cd into a clone of {repository:?}"),
            )
        })
}

/// The issue's active claim: the latest with no `to` (13, at most one).
fn held(index: &Index, issue: &Id) -> Result<Option<Record>, Refusal> {
    Ok(index
        .records(issue.as_str(), Kind::Claim)?
        .into_iter()
        .rev()
        .find(|c| intents::release(&c.body).get("to").is_none_or(String::is_empty)))
}

fn field(r: &Record, name: &str) -> Option<String> {
    r.header.get(name).and_then(Value::as_str).map(str::to_string)
}

/// The fixed worktree of `slug`: `<top>/../<name>-worktrees/<slug>`, canonical, so it names one directory however the
/// clone was reached (26, design change 2).
fn place(top: &Path, slug: &str) -> Result<PathBuf, Refusal> {
    let parent = top.parent().unwrap_or(top);
    let parent = parent.canonicalize().map_err(|e| Refusal::failed(parent.display().to_string(), e.to_string()))?;
    let name = top.file_name().unwrap_or_default().to_string_lossy();
    Ok(parent.join(format!("{name}-worktrees")).join(slug))
}

/// The ref a linked worktree at `path` has checked out, read from its git directory without a process.
fn checked_out(path: &Path) -> Option<String> {
    let repo = Repo::find(path).filter(|r| r.top == path)?;
    let head = std::fs::read_to_string(repo.git.join("HEAD")).ok()?;
    head.trim().strip_prefix("ref: ").map(str::to_string)
}

/// Runs git in the code repository and returns its stdout, trimmed. A failure is `could_not_run` with git's own words.
fn git(dir: &Path, args: &[&str]) -> Result<String, Refusal> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("LC_ALL", "C")
        .args(args)
        .output()
        .map_err(|e| Refusal::failed("git", format!("{e}; install git")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    Err(Refusal::failed(
        format!("git {}", args.join(" ")),
        format!("{}; fix it in {}, then rerun", String::from_utf8_lossy(&out.stderr).trim(), dir.display()),
    ))
}
