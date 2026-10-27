//! The Change page object (18, Change; 32, design change 5): one issue's gates, its stack, the deltas its pull request
//! carries, its diff, and the land command or the refusal `land` would give. The gates are `bp-change`'s, and the
//! refusal is built by [`crate::landing`], as `land` builds it, so the page and the verb never disagree (21, rule 1).
//! `land`'s own pre-gates past the claim and the reviewer (the role, a clean worktree) depend on the human's shell,
//! so the page does not test them.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use bp_change::IssueStatus;
use bp_change::intents::{Gate, Intent, gates};
use bp_vault::{Body, Code, DeltaState, Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use crate::inputs::{base, blocks, field, held, pull_request, reviewer, runs, status, text};
use crate::moving::{head_words, unread};
use crate::{Forge, Intents, landing};

/// The diff text the page carries at most; past it the text is cut at a line and `truncated` is set.
pub const DIFF_CAP: usize = 200 * 1024;

/// One issue's Change page. Exactly one of `command`, `refusal` and `wait` ends it.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Change {
    /// The issue's id.
    pub id: String,
    /// The issue's title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The issue's intent, as `list` prints it; absent for a settled issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    /// The intent's note.
    pub note: String,
    /// The head in words, as Moving prints it: "green at head", "red at head", "checks pending", "no pull request"
    /// or "forge not read".
    pub head: String,
    /// The pull request of the claim's branch, open, or merged under a held claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<ChangePullRequest>,
    /// Every landing gate in `land`'s order; empty without a pull request.
    pub gates: Vec<Gate>,
    /// The refusal `land` would give at this head: the first unmet gate, or a missing claim, reviewer or pull
    /// request.
    #[ts(optional = false)]
    pub refusal: Option<ChangeRefusal>,
    /// The stack in land order, the layers below first, then this one, then the layers cut from it.
    pub stack: Vec<Layer>,
    /// The live issues that depend on this one and hold no pull request: they wait, outside the stack (18, Rejected).
    pub waits_for: Vec<String>,
    /// The proposed deltas the issue caused, which its pull request carries.
    pub deltas: Vec<ChangeDelta>,
    /// What the pull request changes.
    pub diff: Diff,
    /// `bp land <id>`, when every gate holds and the forge was read within the minute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// What the page waits for when no refusal stands and no command shows: a stale forge, or a settled issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<String>,
    /// The forge the gates read.
    pub forge: Forge,
    /// The seconds since the project's snapshot was read; absent when none exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_age: Option<i64>,
}

/// The pull request the gates read.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct ChangePullRequest {
    /// The forge number.
    pub number: u64,
    /// The forge page; a snapshot written before the field lacks it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The branch it merges into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// The head commit.
    pub head: String,
    /// `OPEN` or `MERGED`.
    pub state: String,
}

/// The code and the fix of the refusal `land` would print.
#[derive(Serialize, TS)]
pub struct ChangeRefusal {
    /// The refusal code.
    pub code: String,
    /// The fix, in words.
    pub fix: String,
}

/// One layer of the stack.
#[derive(Serialize, TS)]
pub struct Layer {
    /// The issue's id.
    pub id: String,
    /// Its place in land order, from 1 at the bottom.
    pub layer: usize,
    /// "landed", or its intent and note in words.
    pub state: String,
    /// It is the issue the page shows.
    pub current: bool,
}

/// A delta the pull request carries.
#[derive(Serialize, TS)]
pub struct ChangeDelta {
    /// The delta record's id.
    pub id: String,
    /// The design it changes.
    pub target: String,
    /// `proposed`, `accepted` or `rejected`.
    pub state: String,
}

/// The pull request's diff: the file stat, and the text on request.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Diff {
    /// One row per changed file; empty when no clone holds the head.
    pub files: Vec<DiffFile>,
    /// The diff text, asked for with `?diff=1`, at most [`DIFF_CAP`] bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The text was cut at the cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

/// One changed file: its lines added and removed; a binary file counts none.
#[derive(Serialize, TS)]
pub struct DiffFile {
    /// The path in the repository.
    pub path: String,
    /// Lines added.
    pub additions: u64,
    /// Lines removed.
    pub deletions: u64,
}

/// The Change page of the issue `issue` in `project`. `clone` is the clone the view reads the forge from: when it
/// holds the project's repository, the diff and the diff comparison of a stale approval come from it. `text` asks for
/// the diff text. An id that names no issue of the project refuses `no_node`.
///
/// # Panics
///
/// Never: `Intents::new` reads the world when it is handed a project.
pub fn change(
    vault: &Vault,
    index: &Index,
    project: &str,
    issue: &str,
    clone: Option<&Path>,
    text: bool,
) -> Result<Change, Refusal> {
    let id = crate::subject(index, issue)?;
    let parts: Vec<&str> = id.as_str().split('/').collect();
    let header =
        index.header(id.as_str())?.filter(|_| matches!(parts[..], ["projects", p, "issues", _] if p == project));
    let Some(header) = header else {
        return Err(Refusal::new(
            Code::NoNode,
            id.as_str(),
            "a change page shows one issue of its project: /<project>/change/projects/<project>/issues/<slug>",
            format!("bp list issue --project {project}"),
        ));
    };
    let status = crate::inputs::text(&header, "status");
    let intents = Intents::new(vault, index, Some(&BTreeSet::from([project])))?;
    let verdict = intents.row(Kind::Issue, id.as_str(), status.as_deref())?;
    let world = intents.world.as_ref().expect("an issue's project was handed in");
    let age = world.forge_age();
    let unread = unread(world, age);
    let claim = held(index, &id)?;
    let pr = world.pr(id.as_str()).or_else(|| world.merged(id.as_str()).filter(|_| claim.is_some()));
    let runs = runs(index, id.as_str())?;
    let head = head_words(pr, unread.is_some(), || Ok(runs.clone()))?;
    let reviewer = reviewer(index, id.as_str())?;
    let top = clone.and_then(|c| crate::snapshot::clone_of(c, index, project).ok());
    let base = claim.as_ref().map(|c| base(index, c)).transpose()?.unwrap_or_default();
    let (read, blocks) = (pr.map(|p| pull_request(p, &reviewer)), blocks(vault, id.as_str())?);
    let g = read.as_ref().map(|r| gates(r, &base, &runs, &blocks));
    // `land`'s order: the claim, the reviewer, the pull request, then the gates.
    let resolved = status.as_deref() == Some(IssueStatus::Resolved.as_str());
    let refused = match (&claim, pr, g.as_ref().and_then(|g| g.unmet.as_ref())) {
        _ if resolved => None,
        (None, ..) => Some(landing::unheld(&id, status.as_deref())),
        _ if reviewer.is_empty() => Some(landing::reviewer_invalid(&id, "the project names no reviewer")),
        (_, None, _) => Some(landing::no_pull_request(&id)),
        (Some(c), Some(p), Some(unmet)) => {
            let from = field(c, "base");
            let compare = top.as_deref().zip(from.as_deref());
            let mut r = landing::refusal(unmet, &id, p, &reviewer, compare);
            if p.state.as_deref() == Some("MERGED") {
                r.fix = landing::after_merge(p.number, &id);
            }
            Some(r)
        }
        _ => None,
    };
    let (command, wait) = match (&refused, &unread) {
        _ if resolved => (None, Some("landed".to_string())),
        (Some(_), _) => (None, None),
        (None, Some(u)) => (None, Some(u.clone())),
        (None, None) => (Some(format!("bp land {id}")), None),
    };
    let (stack, waits_for) =
        stack(index, &intents, &id, claim.as_ref(), &verdict_words(verdict.intent, &verdict.note))?;
    let diff = match (top.as_deref(), pr) {
        (Some(top), Some(p)) => diff(top, p, text),
        _ => Diff { files: Vec::new(), text: None, truncated: None },
    };
    Ok(Change {
        id: id.to_string(),
        title: crate::inputs::text(&header, "title"),
        intent: verdict.intent,
        note: verdict.note,
        head: head.to_string(),
        pull_request: pr.map(|p| ChangePullRequest {
            number: p.number,
            url: p.url.clone(),
            base: p.base_ref_name.clone(),
            head: p.head_ref_oid.clone(),
            state: p.state.clone().unwrap_or_else(|| "OPEN".into()),
        }),
        gates: g.map(|g| g.gates).unwrap_or_default(),
        refusal: refused.map(|r| ChangeRefusal { code: r.code.to_string(), fix: r.fix }),
        stack,
        waits_for,
        deltas: carried_deltas(index, &id)?
            .into_iter()
            .map(|(design, d)| ChangeDelta {
                state: DeltaState::of(&Body::parse(&d.body)).as_str().to_string(),
                id: d.id,
                target: design.to_string(),
            })
            .collect(),
        diff,
        command,
        wait,
        forge: if age.is_some() { Forge::Snapshot } else { Forge::NotRead },
        forge_age: age,
    })
}

/// The proposed deltas `issue` caused, each with the design it changes: what its pull request carries (27, design
/// change 8). `pr` writes them into the pull request's body; the Change page lists them.
///
/// # Panics
///
/// Never: the index holds ids.
pub fn carried_deltas(index: &Index, issue: &Id) -> Result<Vec<(Id, Record)>, Refusal> {
    let project = issue.project().expect("an issue sits under a project");
    let all = Filter { kind: Kind::Design, status: None, project: Some(project), under: None, limit: crate::ALL };
    let mut out = Vec::new();
    for row in index.list(&all)?.0 {
        let design = Id::parse(&row.id).expect("the index holds ids");
        for d in index.records(design.as_str(), Kind::Delta)? {
            let caused = d.header.get("cause").and_then(Value::as_str) == Some(issue.as_str());
            if caused && DeltaState::of(&Body::parse(&d.body)) == DeltaState::Proposed {
                out.push((design.clone(), d));
            }
        }
    }
    Ok(out)
}

/// An intent and its note in words: "waiting: waits for 0412 to land".
fn verdict_words(intent: Option<Intent>, note: &str) -> String {
    let intent =
        intent.and_then(|i| Some(serde_json::to_value(i).ok()?.as_str()?.replace('-', " "))).unwrap_or_default();
    match (intent.is_empty(), note.is_empty()) {
        (true, _) => "no intent".to_string(),
        (false, true) => intent,
        (false, false) => format!("{intent}: {note}"),
    }
}

/// The stack of `id` from each claim's `base_issue` (18, Change 2), in land order, and the issues that wait on it
/// outside the stack. Below: the claim's `base_issue`, then that issue's held claim's, or its landed one's once it
/// resolved. Above: each issue whose held claim names a layer as its `base_issue`. Each walk stops at
/// [`STACK_DEPTH`](bp_change::intents::STACK_DEPTH) steps, the deepest stack `cut` makes.
fn stack(
    index: &Index,
    intents: &Intents,
    id: &Id,
    claim: Option<&Record>,
    words: &str,
) -> Result<(Vec<Layer>, Vec<String>), Refusal> {
    let depth = bp_change::intents::STACK_DEPTH;
    let state = |n: &str| -> Result<String, Refusal> {
        let s = status(index, n)?;
        if s.as_deref() == Some(IssueStatus::Resolved.as_str()) {
            return Ok("landed".into());
        }
        let v = intents.row(Kind::Issue, n, s.as_deref())?;
        Ok(verdict_words(v.intent, &v.note))
    };
    let mut below = Vec::new();
    let mut on = claim.and_then(|c| field(c, "base_issue"));
    for _ in 0..depth {
        let Some(next) = on.take() else { break };
        let Ok(n) = Id::parse(&next) else { break };
        let c = match held(index, &n)? {
            Some(h) => Some(h),
            None => landed(index, &n)?,
        };
        on = c.and_then(|c| field(&c, "base_issue"));
        below.push((next.clone(), state(&next)?));
    }
    let mut above = Vec::new();
    let mut level = vec![id.clone()];
    for _ in 0..depth {
        let mut next = Vec::new();
        for l in &level {
            for n in index.inbound(l)? {
                let on = held(index, &n)?.and_then(|h| field(&h, "base_issue"));
                if on.as_deref() == Some(l.as_str()) {
                    above.push((n.to_string(), state(n.as_str())?));
                    next.push(n);
                }
            }
        }
        level = next;
    }
    let mut layers: Vec<(String, String, bool)> = below.into_iter().rev().map(|(i, s)| (i, s, false)).collect();
    if claim.is_some() {
        layers.push((id.to_string(), words.to_string(), true));
    }
    layers.extend(above.into_iter().map(|(i, s)| (i, s, false)));
    let stacked: BTreeSet<&str> = layers.iter().map(|(i, ..)| i.as_str()).collect();
    let world = intents.world.as_ref().expect("an issue's project was handed in");
    let mut waits = Vec::new();
    for n in index.inbound(id)? {
        let Some(h) = index.header(n.as_str())? else { continue };
        let depends = h.get("depends").and_then(Value::as_array).is_some_and(|d| d.iter().any(|t| t == id.as_str()));
        let live = text(&h, "status").is_some_and(|s| bp_change::intents::is_live_issue(&s));
        if depends && live && world.pr(n.as_str()).is_none() && !stacked.contains(n.as_str()) {
            waits.push(n.to_string());
        }
    }
    let stack = layers
        .into_iter()
        .enumerate()
        .map(|(i, (id, state, current))| Layer { id, layer: i + 1, state, current })
        .collect();
    Ok((stack, waits))
}

/// The claim `issue` landed under: the latest released `landed`.
fn landed(index: &Index, issue: &Id) -> Result<Option<Record>, Refusal> {
    let claims = index.records(issue.as_str(), Kind::Claim)?;
    Ok(claims
        .into_iter()
        .rev()
        .find(|c| bp_change::intents::release(&c.body).get("reason").map(String::as_str) == Some("landed")))
}

/// The diff of `p` against its base as the clone at `top` holds it: `git diff` from their merge base, the file stat,
/// and the text when asked. A clone that lacks the head or the base gives no files.
fn diff(top: &Path, p: &bp_forge::PullRequest, text: bool) -> Diff {
    let range = format!("refs/remotes/origin/{}...{}", p.base_ref_name.as_deref().unwrap_or("HEAD"), p.head_ref_oid);
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["diff", "--no-ext-diff", "--no-color"])
            .args(args)
            .args(["--end-of-options", &range])
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)
    };
    let files = git(&["--numstat"]).map_or_else(Vec::new, |stat| {
        String::from_utf8_lossy(&stat)
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                let (add, del, path) = (f.next()?, f.next()?, f.next()?);
                Some(DiffFile {
                    path: path.into(),
                    additions: add.parse().unwrap_or(0),
                    deletions: del.parse().unwrap_or(0),
                })
            })
            .collect()
    });
    let (text, truncated) = match git(&[]).filter(|_| text) {
        None => (None, None),
        Some(bytes) if bytes.len() <= DIFF_CAP => (Some(String::from_utf8_lossy(&bytes).into_owned()), None),
        Some(bytes) => {
            // Cut at the last line end inside the cap, so no line shows half.
            let end = bytes[..DIFF_CAP].iter().rposition(|&b| b == b'\n').map_or(DIFF_CAP, |i| i + 1);
            (Some(String::from_utf8_lossy(&bytes[..end]).into_owned()), Some(true))
        }
    };
    Diff { files, text, truncated }
}
