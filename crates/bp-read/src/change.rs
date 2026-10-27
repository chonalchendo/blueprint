//! The Change page object (18, Change; 32, design change 5): one issue's gates, its stack, the deltas its pull request
//! carries, its diff, and the land command or the refusal `land` would give. The gates are `bp-change`'s, and the
//! refusal is built by [`crate::landing`], as `land` builds it, so the page and the verb never disagree (21, rule 1).
//! `land`'s own pre-gates past the claim and the reviewer (the role, a clean worktree) depend on the human's shell,
//! so the page does not test them.

use std::collections::BTreeSet;
use std::path::Path;

use bp_change::IssueStatus;
use bp_change::intents::{Base, Gate, GateState, Intent, gates};
use bp_vault::{Body, Code, DeltaState, Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

pub use crate::diff::{DIFF_CAP, Diff, DiffFile};
use crate::inputs::{base, blocks, field, held, pull_request, reviewer, runs};
use crate::moving::{head_words, unread};
pub use crate::stack::Layer;
use crate::{Forge, Intents, landing};

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

/// The Change page of the issue `issue` in `project`. `clone` is the clone the view reads the forge from: when it
/// holds the project's repository, the diff, the check set the head names, the default branch of a claim that records
/// no base and the diff comparison of a stale approval come from it. Without it those gates read "not read" and the
/// command is withheld. `text` asks for the diff text. An id that names no issue of the project refuses `no_node`.
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
    let (base, named) = clone_reads(vault, index, &id, claim.as_ref(), pr, top.as_deref())?;
    let (read, blocks) = (pr.map(|p| pull_request(p, &reviewer)), blocks(vault, id.as_str())?);
    let g = read.as_ref().map(|r| gates(r, &base, &runs, &blocks, named));
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
    // With no unmet gate, a gate that is not `pass` is one this read could not make: it withholds the command.
    let unread_gate = g.as_ref().and_then(|g| g.gates.iter().find(|g| g.state != GateState::Pass));
    let (command, wait) = match (&refused, &unread, unread_gate) {
        _ if resolved => (None, Some("landed".to_string())),
        (Some(_), ..) => (None, None),
        (None, Some(u), _) => (None, Some(u.clone())),
        (None, None, Some(g)) => (
            None,
            Some(format!(
                "{}: {}; start bp-view with --clone at the project's clone, or git fetch in it",
                g.name, g.detail
            )),
        ),
        (None, None, None) => (Some(format!("bp land {id}")), None),
    };
    let words = crate::stack::verdict_words(verdict.intent, &verdict.note);
    let (stack, waits_for) = crate::stack::stack(index, &intents, &id, claim.as_ref(), &words)?;
    let diff = match (top.as_deref(), pr) {
        (Some(top), Some(p)) => crate::diff::diff(top, p, text),
        (None, Some(_)) => Diff::none("no clone holds the project; start bp-view with --clone at the project's clone"),
        (_, None) => Diff::default(),
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

/// What `land` reads from the clone at `top`, as it reads it: the claim's base, origin's default branch when the claim
/// records none, and whether the head names the check set now. Without a clone, a claim with no base stays `Unknown`
/// and the check set is not read (`None`).
fn clone_reads(
    vault: &Vault,
    index: &Index,
    id: &Id,
    claim: Option<&Record>,
    pr: Option<&bp_forge::PullRequest>,
    top: Option<&Path>,
) -> Result<(Base, Option<bool>), Refusal> {
    let mut base = claim.map(|c| base(index, c)).transpose()?.unwrap_or_default();
    if base == Base::Unknown
        && let Some(b) = top.and_then(|t| crate::git::default_branch(t).ok())
    {
        base = Base::Branch(b);
    }
    let named = match (claim, pr, top) {
        (Some(_), Some(p), Some(top)) => {
            crate::checkset::check_set(vault, index, id).ok().and_then(|s| s.named_by(top, &p.head_ref_oid).ok())
        }
        _ => None,
    };
    Ok((base, named))
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::*;

    const REPO: &str = "https://forge.test/o/bp";

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// git with the owner's config left out, so signing and hooks cannot reach the fixture.
    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    const BLOCK: &str = "## Check\n\n~~~check shell {name=a expect=0}\ntrue\n~~~\n";

    fn issue(slug: &str, status: &str, more: &str) -> (String, String) {
        let text = format!(
            "---\nkind: issue\ntitle: {slug}\nstatus: {status}\nunder: projects/bp/milestones/m\n{more}---\n\n{BLOCK}"
        );
        (format!("projects/bp/issues/{slug}.md"), text)
    }

    fn claim(issue: &str, more: &str) -> (String, String) {
        let from = crate::clock::instant(crate::clock::now().unwrap());
        let text = format!("---\nkind: claim\nunder: {issue}\nfrom: {from}\nowner: worker\n{more}---\n");
        (format!("{issue}/claims/s1.md"), text)
    }

    /// A pull request of `slug` at `head` onto `base`, approved by `rev` and green when `approved`.
    fn pr(n: u64, slug: &str, head: &str, base: &str, approved: bool) -> String {
        let reviews = if approved {
            format!(r#"[{{"author":{{"login":"rev"}},"state":"APPROVED","commit":{{"oid":"{head}"}}}}]"#)
        } else {
            "[]".into()
        };
        format!(
            r#"{{"number":{n},"url":"https://forge.test/o/bp/pull/{n}","headRefName":"bp/{slug}","headRefOid":"{head}","baseRefName":"{base}","reviews":{reviews},"statusCheckRollup":[{{"conclusion":"SUCCESS"}}]}}"#
        )
    }

    struct Fx {
        _dir: tempfile::TempDir,
        vault: Vault,
        clone: PathBuf,
        head: String,
    }

    /// A clone whose head adds a 300 KB file onto `master`, and a vault of project `bp` in git: `0001.land` approved,
    /// green and run at the head; `0002.above` cut from it with no pull request; `0003.wait` depends on it; an
    /// unclaimed, an unpushed, an unapproved and a no-base issue; and project `nr` with no reviewer.
    fn fixture() -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let (root, clone) = (dir.path().join("v"), dir.path().join("c"));
        std::fs::create_dir_all(&clone).unwrap();
        git(&clone, &["init", "-q", "-b", "master"]);
        git(&clone, &["remote", "add", "origin", REPO]);
        write(&clone, "README", "base\n");
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-qm", "base"]);
        let base = git(&clone, &["rev-parse", "HEAD"]);
        git(&clone, &["update-ref", "refs/remotes/origin/master", &base]);
        git(&clone, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/master"]);
        let big = "a line of the big file\n".repeat(30_000);
        write(&clone, "big.txt", &big);
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-qm", "change"]);
        let head = git(&clone, &["rev-parse", "HEAD"]);

        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        let hash = Body::parse(BLOCK).named_checks()[0].hash();
        let project = |reviewer: &str| {
            format!("---\nkind: project\ntitle: B\nstatus: active\n{reviewer}repository: {REPO}\n---\n")
        };
        let i = |slug: &str| format!("projects/bp/issues/{slug}");
        let on_master = format!("base_branch: master\nbase: {base}\n");
        let run = |issue: &str| {
            let at = "at: 2026-10-27T09:00:00Z\nverdict: pass";
            let header =
                format!("kind: run\nunder: {issue}\ncheck: a\ncheck_hash: {hash}\ncommit: {head}\nattempt: 1\n{at}");
            (format!("{issue}/runs/a-1.md"), format!("---\n{header}\n---\n"))
        };
        let mut files = vec![
            (".gitignore".into(), ".bp/\n".into()),
            ("shared/.keep".into(), String::new()),
            ("projects/bp/project.md".into(), project("reviewer: rev\n")),
            ("projects/bp/designs/system.md".into(), "---\nkind: design\ntitle: S\nstatus: active\n---\n".into()),
            (
                "projects/bp/milestones/m.md".into(),
                format!("---\nkind: milestone\ntitle: M\nstatus: approved\n---\n\n{}", BLOCK.replace("=a ", "=m ")),
            ),
            issue("0001.land", "in-progress", ""),
            claim(&i("0001.land"), &on_master),
            run(&i("0001.land")),
            issue("0002.above", "in-progress", "depends:\n  - projects/bp/issues/0001.land\n"),
            claim(&i("0002.above"), "base_issue: projects/bp/issues/0001.land\nbase_branch: bp/0001.land\n"),
            issue("0003.wait", "open", "depends:\n  - projects/bp/issues/0001.land\n"),
            issue("0004.open", "open", ""),
            issue("0005.unpushed", "in-progress", ""),
            claim(&i("0005.unpushed"), &on_master),
            issue("0006.unapproved", "in-progress", ""),
            claim(&i("0006.unapproved"), &on_master),
            issue("0007.nobase", "in-progress", ""),
            claim(&i("0007.nobase"), ""),
            run(&i("0007.nobase")),
            ("projects/nr/project.md".into(), project("")),
            ("projects/nr/issues/0001.x.md".into(), "---\nkind: issue\ntitle: X\nstatus: in-progress\n---\n".into()),
            claim("projects/nr/issues/0001.x", ""),
        ];
        let prs = [
            pr(1, "0001.land", &head, "master", true),
            pr(3, "0006.unapproved", &head, "master", false),
            pr(4, "0007.nobase", &head, "release", true),
        ];
        files.push((".bp/forge/bp/open-prs.json".into(), format!("[{}]", prs.join(","))));
        for (rel, text) in &files {
            write(&root, rel, text);
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "claims"]);
        Fx { _dir: dir, vault: Vault { root }, clone, head }
    }

    fn page(fx: &Fx, project: &str, slug: &str, clone: bool, text: bool) -> Change {
        let index = Index::open(&fx.vault).unwrap();
        let id = format!("projects/{project}/issues/{slug}");
        change(&fx.vault, &index, project, &id, clone.then_some(fx.clone.as_path()), text).unwrap()
    }

    fn refusal(c: &Change) -> (&str, &str) {
        c.refusal.as_ref().map(|r| (r.code.as_str(), r.fix.as_str())).unwrap_or_default()
    }

    fn states(c: &Change) -> Vec<(&str, GateState)> {
        c.gates.iter().map(|g| (g.name.as_str(), g.state)).collect()
    }

    #[test]
    fn every_gate_holds_so_the_page_shows_land_and_the_stack() {
        use GateState::Pass;
        let fx = fixture();
        let c = page(&fx, "bp", "0001.land", true, false);
        let all =
            [("base", Pass), ("approving review", Pass), ("forge checks", Pass), ("run a", Pass), ("check set", Pass)];
        assert_eq!(states(&c), all);
        assert_eq!((c.command.as_deref(), c.wait.as_deref()), (Some("bp land projects/bp/issues/0001.land"), None));
        assert!(c.refusal.is_none());
        let stack: Vec<(&str, usize, &str, bool)> =
            c.stack.iter().map(|l| (l.id.as_str(), l.layer, l.state.as_str(), l.current)).collect();
        assert_eq!(stack[1], ("projects/bp/issues/0002.above", 2, "no pull request", false));
        assert_eq!((stack.len(), stack[0].0, stack[0].3), (2, "projects/bp/issues/0001.land", true));
        assert_eq!(c.waits_for, ["projects/bp/issues/0003.wait"]);
        let files: Vec<(&str, u64)> = c.diff.files.iter().map(|f| (f.path.as_str(), f.additions)).collect();
        assert_eq!((files, c.diff.text.as_deref(), c.diff.reason.as_deref()), (vec![("big.txt", 30_000)], None, None));
    }

    #[test]
    fn the_diff_text_is_cut_at_a_line_end_under_the_cap() {
        let fx = fixture();
        let c = page(&fx, "bp", "0001.land", true, true);
        let text = c.diff.text.unwrap();
        assert_eq!(c.diff.truncated, Some(true));
        assert!(text.len() <= DIFF_CAP && text.len() > DIFF_CAP - 20 && text.ends_with('\n'), "{}", text.len());
    }

    #[test]
    fn without_a_clone_the_check_set_is_not_read_and_the_command_waits() {
        let fx = fixture();
        let c = page(&fx, "bp", "0001.land", false, false);
        let last = c.gates.last().map(|g| (g.name.as_str(), g.state));
        assert_eq!(last, Some(("check set", GateState::NotRead)));
        assert!(c.refusal.is_none() && c.command.is_none());
        assert!(c.wait.as_deref().is_some_and(|w| w.starts_with("check set: no clone holds")), "{:?}", c.wait);
        assert_eq!(c.diff.reason.as_deref().map(|r| r.starts_with("no clone holds the project")), Some(true));
    }

    #[test]
    fn a_changed_milestone_block_refuses_checks_changed() {
        let fx = fixture();
        let m = fx.vault.root.join("projects/bp/milestones/m.md");
        let text = std::fs::read_to_string(&m).unwrap() + "\n~~~check shell {name=more expect=0}\ntrue\n~~~\n";
        std::fs::write(&m, text).unwrap();
        git(&fx.vault.root, &["commit", "-qam", "more"]);
        let c = page(&fx, "bp", "0001.land", true, false);
        assert_eq!(c.gates.last().map(|g| g.state), Some(GateState::Fail));
        let (code, fix) = refusal(&c);
        assert_eq!(code, "checks_changed");
        assert!(fix.starts_with("an error block of projects/bp/issues/0001.land, its milestone"), "{fix}");
        assert!(c.command.is_none() && c.wait.is_none());
    }

    #[test]
    fn a_claim_with_no_base_is_tested_against_the_default_branch() {
        let fx = fixture();
        // With the clone, origin's default branch is master, and the pull request targets release.
        let c = page(&fx, "bp", "0007.nobase", true, false);
        assert_eq!(c.gates[0].state, GateState::Fail);
        let (code, fix) = refusal(&c);
        assert_eq!(code, "base_mismatch");
        assert!(fix.starts_with("pull request 4 targets release;"), "{fix}");
        assert_eq!(
            c.diff.reason.as_deref(),
            Some(format!("the clone lacks origin/release or {}; git fetch in the clone", &fx.head[..7]).as_str())
        );
        // Without it, the base is pending and the command waits.
        let c = page(&fx, "bp", "0007.nobase", false, false);
        assert_eq!(c.gates[0].state, GateState::Pending);
        assert_eq!(c.gates[0].detail, "the claim records no base; land tests origin's default branch");
        assert!(c.refusal.is_none() && c.command.is_none());
        assert!(c.wait.as_deref().is_some_and(|w| w.starts_with("base: the claim records no base")), "{:?}", c.wait);
    }

    #[test]
    fn each_refusal_is_land_s_in_its_order() {
        let fx = fixture();
        let unclaimed = page(&fx, "bp", "0004.open", true, false);
        assert_eq!(refusal(&unclaimed), ("illegal_transition", "bp cut projects/bp/issues/0004.open, then rerun"));
        assert!(unclaimed.gates.is_empty() && unclaimed.command.is_none());
        let reviewer = page(&fx, "nr", "0001.x", true, false);
        assert_eq!(refusal(&reviewer).0, "reviewer_invalid");
        let unpushed = page(&fx, "bp", "0005.unpushed", true, false);
        let fix = "bp/0005.unpushed has no open pull request; bp pr projects/bp/issues/0005.unpushed, then the reviewer \
                   approves it";
        assert_eq!(refusal(&unpushed), ("review_missing", fix));
        let unapproved = page(&fx, "bp", "0006.unapproved", true, false);
        assert_eq!(unapproved.gates[1].state, GateState::Pending);
        let (code, fix) = refusal(&unapproved);
        assert_eq!(code, "review_missing");
        assert!(fix.starts_with("the human approves pull request 3 at"), "{fix}");
        for c in [&unclaimed, &reviewer, &unpushed, &unapproved] {
            assert!(c.command.is_none() && c.wait.is_none(), "{}", c.id);
        }
    }

    #[test]
    fn a_stale_snapshot_gives_the_wait_and_no_command() {
        let fx = fixture();
        let snapshot = fx.vault.root.join(".bp/forge/bp/open-prs.json");
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&snapshot).unwrap().set_modified(hour_ago).unwrap();
        let c = page(&fx, "bp", "0001.land", true, false);
        assert!(c.refusal.is_none() && c.command.is_none());
        assert!(c.wait.as_deref().is_some_and(|w| w.starts_with("forge not read since ")), "{:?}", c.wait);
    }
}
