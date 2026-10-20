//! `bp cut <issue>` and `bp release <issue>`: a claim opens and closes, each in one commit with `Bp-Claim` (15, the
//! repo verbs; 26, design changes 2 and 7). `cut` runs in a clone of the project repository. It fetches with no lock
//! held, then under the lock adds the worktree and the branch at their fixed names and commits the claim and the issue
//! at `in-progress`. `release`
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
    let index = Index::open(&vault)?;
    let id = subject(&index, &c.issue)?;
    let (project, slug) = (id.project().unwrap_or_default(), id.as_str().rsplit('/').next().unwrap_or_default());
    let branch = format!("{project}/{slug}");
    let top = clone_of(cwd, &index, project)?;
    let worktree = place(&top, slug)?;
    // The fetch has no time bound, so it runs with no lock held: every refusal comes before it and holds under the
    // lock too, as `stamp` checks around its blocks (26, design change 2).
    vault.clean([&id])?;
    let check = |index: &Index| gate(index, &id, &vault.text(&id)?, role.as_str(), session.as_deref(), &branch);
    if let Some(repeat) = check(&index)? {
        return repeated(&top, &branch, repeat);
    }
    drop(index);
    git(&top, &["fetch", "-q", "origin"])?;
    let start = c.base.as_deref().unwrap_or("origin/HEAD");
    let start = git(&top, &["rev-parse", "--verify", "-q", "--end-of-options", &format!("{start}^{{commit}}")])
        .map_err(|e| match &c.base {
            Some(b) => Refusal::new(
                Code::UnknownValue,
                format!("--base {b}"),
                "a value comes from the refs git resolves",
                "name a commit, branch or tag the clone holds: git fetch, then rerun",
            ),
            None => Refusal::failed(e.subject, "origin has no default branch here: git remote set-head origin --auto"),
        })?;
    let mut writer = vault.writer()?;
    let from = crate::clock::instant(crate::clock::now()?);
    let claim = crate::record::free(&vault.root, &format!("{id}/{}/{}", Kind::Claim.dir(), from.replace(':', "-")));
    writer.clean([&id, &claim])?;
    if let Some(repeat) = check(writer.index())? {
        return repeated(&top, &branch, repeat);
    }
    let text = vault.text(&id)?;
    let added = attach(&top, &worktree, &branch, &start)?;
    // A kept branch carries its commits: its base is where it left the start, not the start's head.
    let base = if added == Added::Both { start } else { git(&worktree, &["merge-base", "HEAD", &start])? };
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
    let written = writer.commit("cut", role.as_str(), &format!("cut {id}"), &changes).inspect_err(|_| {
        // Nothing of a refused cut stays: what it added goes with the claim. The undo's own failure is not the error
        // the caller needs.
        if added != Added::None {
            let _ = git(&top, &["worktree", "remove", "--force", &worktree.display().to_string()]);
        }
        if added == Added::Both {
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
    let mut writer = vault.writer()?;
    let id = subject(writer.index(), &r.issue)?;
    let Some(h) = held(writer.index(), &id)? else { return Ok(Wrote::Already(json!({ "id": id }))) };
    let claim = Id::parse(&h.id).expect("the index holds ids");
    writer.clean([&id, &claim])?;
    let text = vault.text(&id)?;
    let to = crate::clock::instant(crate::clock::now()?);
    let released =
        format!("{}\n\n## Released\n\nto: {to}\nreason: {}\n", vault.text(&claim)?.trim_end_matches('\n'), r.reason);
    let mut changes = BTreeMap::from([(claim.clone(), released)]);
    // A converted claim may sit on an issue whose status never moved (21, check 2); only `in-progress` returns.
    let was = bp_vault::File::from_bytes(id.clone(), text.as_bytes())?.header.and_then(|h| h.status);
    if was.as_deref() == Some("in-progress") {
        changes.insert(id.clone(), bp_vault::set_field(&text, "status", &"open".into()));
    }
    writer.trailer(format!("Bp-Claim: {claim}"));
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
/// `illegal_transition` unless the issue (`text`) is `open`.
fn gate(
    index: &Index,
    id: &Id,
    text: &str,
    role: &str,
    session: Option<&str>,
    branch: &str,
) -> Result<Option<Value>, Refusal> {
    if let Some(h) = held(index, id)? {
        let (owner, held_session) = (field(&h, "owner"), field(&h, "session"));
        if owner.as_deref() == Some(role) && held_session.as_deref() == session {
            let (worktree, base) = (field(&h, "worktree"), field(&h, "base"));
            return Ok(Some(json!({ "id": id, "claim": h.id, "branch": branch, "worktree": worktree, "base": base })));
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
    let file = bp_vault::File::from_bytes(id.clone(), text.as_bytes())?;
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
    // From a linked worktree too, the top is the main worktree's, so a new worktree never nests in another.
    Repo::find(cwd)
        .filter(|r| r.remote().is_some_and(|url| bp_vault::git::same_repository(&url, &repository)))
        .and_then(|r| r.main_top())
        .ok_or_else(|| {
            Refusal::new(
                Code::NoClone,
                cwd.display().to_string(),
                "a repo verb runs inside a clone of the project's repository (26, design change 2)",
                format!("cd into a clone of {repository}, or git clone {repository}, then rerun"),
            )
        })
}

/// A repeat `cut` exits 3 with the claim's fields. A worktree a human removed comes back from its branch first.
fn repeated(top: &Path, branch: &str, repeat: Value) -> Result<Wrote, Refusal> {
    if let Some(w) = repeat["worktree"].as_str().map(Path::new).filter(|w| !w.exists()) {
        attach(top, w, branch, repeat["base"].as_str().unwrap_or_default())?;
    }
    Ok(Wrote::Already(repeat))
}

/// What `attach` added to the clone, so a failed commit takes back only that.
#[derive(PartialEq)]
enum Added {
    None,
    Worktree,
    Both,
}

/// Puts `branch` at `worktree`: a kept worktree stays, a kept branch gets a new worktree, else both start at `start`.
fn attach(top: &Path, worktree: &Path, branch: &str, start: &str) -> Result<Added, Refusal> {
    let head = format!("refs/heads/{branch}");
    if checked_out(worktree).as_deref() == Some(&head) {
        return Ok(Added::None);
    }
    let path = worktree.display().to_string();
    if git(top, &["rev-parse", "--verify", "-q", &head]).is_err() {
        git(top, &["worktree", "add", "-q", "--no-track", "-b", branch, &path, start])?;
        return Ok(Added::Both);
    }
    // The branch outlived its worktree. A directory deleted by hand leaves git's record of it, which `prune` drops.
    git(top, &["worktree", "prune"])?;
    git(top, &["worktree", "add", "-q", &path, branch])?;
    Ok(Added::Worktree)
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
        // A credential prompt would wait on a terminal no agent reads; the fetch fails instead.
        .env("GIT_TERMINAL_PROMPT", "0")
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
