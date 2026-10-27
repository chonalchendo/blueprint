//! The Moving page object (18, Moving; 32, design change 7): the project's rows with an intent, in five groups by
//! the act the human owes. Each row's intent and note are the ones `list` prints, from the same `Intents` calls, and
//! every word the page shows is built here: the holder, the head in words, and the command or the wait. The client
//! computes nothing.

use std::collections::BTreeSet;

use bp_change::intents::{Intent, failed_at, release};
use bp_vault::{Id, Index, Kind, Refusal, Vault};
use serde::Serialize;
use ts_rs::TS;

use crate::inputs::{claims, field, held, runs};
use crate::pages::{Read, no_project};
use crate::{Forge, World};

/// A snapshot read this many seconds ago or less is read: the land command shows (32, design change 4). The view
/// refreshes at most once a minute, so with the forge on a fresh snapshot is never older.
const FRESH: i64 = 60;

/// The five groups in page order: id, words, and the intents each holds (32, design change 7). The first two owe the
/// human a decision. Every intent sits in exactly one group.
const GROUPS: [(&str, &str, &[Intent]); 5] = [
    ("needs-landing", "needs landing", &[Intent::NeedsLanding]),
    (
        "needs-acceptance",
        "needs acceptance",
        &[Intent::NeedsAcceptance, Intent::NeedsMilestoneApproval, Intent::ReAsked],
    ),
    ("blocked", "blocked", &[Intent::Blocked, Intent::Drifted]),
    ("ready", "ready", &[Intent::Ready]),
    (
        "working",
        "working",
        &[
            Intent::Working,
            Intent::Waiting,
            Intent::NeedsReview,
            Intent::NeedsResponse,
            Intent::NeedsApproval,
            Intent::NeedsDerivation,
            Intent::NeedsShaping,
        ],
    ),
];

/// A project's Moving page: its issues, milestones and designs that carry an intent, grouped.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Moving {
    /// The project's name, as its routes hold it.
    pub project: String,
    /// The forge the issues' intents read.
    pub forge: Forge,
    /// The seconds since the project's snapshot was read; absent when none exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forge_age: Option<i64>,
    /// The five groups, in [`GROUPS`] order, each present when empty.
    pub groups: Vec<MovingGroup>,
}

/// One group of Moving.
#[derive(Serialize, TS)]
pub struct MovingGroup {
    /// The group's id: `needs-landing`, `needs-acceptance`, `blocked`, `ready` or `working`.
    pub group: String,
    /// The group's words.
    pub label: String,
    /// The intents it holds.
    pub intents: Vec<Intent>,
    /// Its rows, by intent in that order, then as `list` orders them.
    pub rows: Vec<MovingRow>,
}

/// One row of Moving: exactly one of `command` and `wait`.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct MovingRow {
    /// The node's id.
    pub id: String,
    /// `issue`, `milestone` or `design`.
    pub kind: String,
    /// The node's title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The intent, as `list` prints it.
    pub intent: Intent,
    /// The intent's note, as `list` prints it.
    pub note: String,
    /// The owner of the issue's held claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<String>,
    /// An issue's head in words: "green at head", "red at head", "checks pending", "no pull request" or "forge not
    /// read".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// The tool's own command for the human's next act.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// What the row waits for, in words, when no command of the human's moves it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<String>,
}

/// The facts of one row past its intent and note: what the claim and the forge say.
#[derive(Debug, Default)]
struct Facts {
    /// The held claim's owner.
    holder: Option<String>,
    /// The held claim's date, `YYYY-MM-DD`.
    since: Option<String>,
    /// The pull request's head commit.
    sha: Option<String>,
    /// When the snapshot is not fresh: "forge not read since <time>", or "forge not read" when none exists.
    unread: Option<String>,
    /// An escalated issue's latest released claim: its owner and its release date, `YYYY-MM-DD`.
    escalated: Option<(String, String)>,
}

/// A row's next act: the human's command, or the wait in words.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    Command(String),
    Wait(String),
}

/// The Moving page of `project`. A project the index lacks refuses `no_node`.
pub fn moving(vault: &Vault, index: &Index, project: &str) -> Result<Moving, Refusal> {
    let id = format!("projects/{project}/project");
    if index.header(&id)?.is_none() {
        return Err(no_project(id));
    }
    let read = Read::of(vault, index, project)?;
    // The forge the rows' heads read: the project's own snapshot, as the issues' intents read it.
    let world = World::read(&vault.root, &BTreeSet::from([project]))?;
    let age = world.forge_age();
    let unread = match age {
        Some(a) if a <= FRESH => None,
        Some(a) => Some(format!("forge not read since {}", crate::clock::instant(world.now - a))),
        None => Some("forge not read".to_string()),
    };
    let mut groups: Vec<MovingGroup> = GROUPS
        .iter()
        .map(|(group, label, intents)| MovingGroup {
            group: (*group).to_string(),
            label: (*label).to_string(),
            intents: intents.to_vec(),
            rows: Vec::new(),
        })
        .collect();
    for row in read.issues.rows.iter().chain(&read.milestones).chain(&read.designs) {
        let (Some(intent), Some(note)) = (row.intent, row.note.as_deref()) else { continue };
        let mut facts = Facts { unread: unread.clone(), ..Facts::default() };
        let mut head = None;
        if row.row.kind == "issue" {
            let id = crate::node_id(&row.row.id)?;
            let claim = held(index, &id)?;
            facts.holder = claim.as_ref().and_then(|c| field(c, "owner"));
            facts.since = claim.as_ref().and_then(|c| field(c, "from")).map(|t| t.chars().take(10).collect());
            if intent == Intent::Blocked && note == "escalated" {
                facts.escalated = escalation(index, &id)?;
            }
            let pr = world.pr(id.as_str()).or_else(|| world.merged(id.as_str()).filter(|_| claim.is_some()));
            // A stale snapshot verifies no gate, so the head states none (18, An error case). The wait gives the time.
            head = Some(match pr {
                _ if unread.is_some() => "forge not read",
                None => "no pull request",
                Some(p) if p.has_failed() || failed_at(&runs(index, id.as_str())?, &p.head_ref_oid) => "red at head",
                Some(p) if p.is_green() => "green at head",
                Some(_) => "checks pending",
            });
            facts.sha = pr.map(|p| p.head_ref_oid.clone());
        }
        let (command, wait) = match next(&row.row.id, intent, note, &facts) {
            Next::Command(c) => (Some(c), None),
            Next::Wait(w) => (None, Some(w)),
        };
        groups[group_of(intent)].rows.push(MovingRow {
            id: row.row.id.clone(),
            kind: row.row.kind.clone(),
            title: row.row.title.clone(),
            intent,
            note: note.to_string(),
            holder: facts.holder,
            head: head.map(str::to_string),
            command,
            wait,
        });
    }
    // A stable sort: within one intent, rows keep `list`'s order.
    for g in &mut groups {
        let order = g.intents.clone();
        g.rows.sort_by_key(|r| order.iter().position(|i| *i == r.intent));
    }
    let forge = if age.is_some() { Forge::Snapshot } else { Forge::NotRead };
    Ok(Moving { project: project.to_string(), forge, forge_age: age, groups })
}

/// The index in [`GROUPS`] of the group that holds `intent`.
///
/// # Panics
///
/// Never: every intent sits in a group, which a test holds.
fn group_of(intent: Intent) -> usize {
    GROUPS.iter().position(|(_, _, intents)| intents.contains(&intent)).expect("every intent sits in a group")
}

/// The owner and release date of the latest released claim of `issue`: who escalated it, and when.
fn escalation(index: &Index, issue: &Id) -> Result<Option<(String, String)>, Refusal> {
    let records = index.records(issue.as_str(), Kind::Claim)?;
    let latest = records.iter().zip(claims(&records)).filter(|(_, c)| c.released.is_some()).max_by_key(|(_, c)| c.at);
    Ok(latest.and_then(|(r, _)| {
        let date = release(&r.body).get("to")?.chars().take(10).collect();
        Some((field(r, "owner")?, date))
    }))
}

/// The next act of the node `id` (18, Moving; 21): the tool's own command where the human owes one, else the wait in
/// words. Every string comes from the intent, its note and the facts; nothing is guessed.
fn next(id: &str, intent: Intent, note: &str, f: &Facts) -> Next {
    let with = |words: &str| if note.is_empty() { words.to_string() } else { format!("{words}: {note}") };
    match intent {
        Intent::Ready => Next::Command(format!("bp cut {id}")),
        // The gates cannot be verified on a stale snapshot, so the land is withheld (18, An error case).
        Intent::NeedsLanding => match &f.unread {
            None => Next::Command(format!("bp land {id}")),
            Some(unread) => Next::Wait(unread.clone()),
        },
        // The note names the stamp, with each failed block's waive (21, needs acceptance).
        Intent::NeedsAcceptance => Next::Command(named(note).map_or_else(|| format!("bp stamp {id}"), str::to_string)),
        Intent::NeedsMilestoneApproval => Next::Command(format!("bp transition {id} approved")),
        // The waive belongs to the next milestone's stamp, so this row waits, and the decision sits on that milestone's
        // needs-acceptance row (21, re-asked).
        Intent::ReAsked => Next::Wait(match (note.split_once("; "), note.find("--waive ")) {
            (Some((blocks, _)), Some(at)) => {
                format!("re-asked: {blocks}; waive again at the next stamp with {}", &note[at..])
            }
            _ => with("re-asked"),
        }),
        // An escalation waits on the human's answer, a write to the issue (21, blocked).
        Intent::Blocked if note == "escalated" => Next::Wait(match &f.escalated {
            Some((owner, date)) => format!("escalated by {owner} on {date}: answer in the issue"),
            None => with("blocked"),
        }),
        // An integrity finding names its rerun; a missing or abandoned dependency names its target and waits.
        Intent::Blocked => named(note).map_or_else(|| Next::Wait(with("blocked")), |c| Next::Command(c.to_string())),
        // The triage is a session act (21, triage drift).
        Intent::Drifted => Next::Wait(with("drifted")),
        Intent::Working if note == "stale" => Next::Command(format!("bp release {id} --reason lost")),
        Intent::Working => match (&f.holder, &f.since) {
            (Some(h), Some(d)) if note.is_empty() => Next::Wait(format!("held by {h} since {d}")),
            (Some(h), Some(d)) => Next::Wait(format!("held by {h} since {d}; {note}")),
            _ => Next::Wait(with("working")),
        },
        Intent::Waiting => Next::Wait(note.to_string()),
        Intent::NeedsReview => Next::Wait(with("review dispatch")),
        Intent::NeedsResponse => Next::Wait(with("response dispatch")),
        Intent::NeedsApproval => Next::Wait(match &f.sha {
            Some(sha) => format!("awaits the reviewer's approval at {}", &sha[..sha.len().min(7)]),
            None => "awaits the reviewer's approval".to_string(),
        }),
        Intent::NeedsDerivation => Next::Wait(with("session derives")),
        Intent::NeedsShaping => Next::Wait(with("session shapes")),
    }
}

/// The tool command a note names: from its first `bp ` word to the end ("…; rerun bp check m --name integrity").
fn named(note: &str) -> Option<&str> {
    let at = if note.starts_with("bp ") { 0 } else { note.find(" bp ")? + 1 };
    Some(&note[at..])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn every_intent_sits_in_exactly_one_group() {
        // `ALL` is a hand list, so tie it to the enum: the TS union ts-rs derives names every variant.
        let ts = Intent::inline(&ts_rs::Config::new());
        let variants: Vec<&str> = ts.split('|').map(|v| v.trim().trim_matches('"')).collect();
        let all: Vec<String> =
            Intent::ALL.iter().map(|i| serde_json::to_value(i).unwrap().as_str().unwrap().to_string()).collect();
        assert_eq!(variants, all, "Intent::ALL holds every variant of the enum, in order");
        for intent in Intent::ALL {
            let n = GROUPS.iter().filter(|(_, _, intents)| intents.contains(&intent)).count();
            assert_eq!(n, 1, "{intent:?} sits in {n} groups");
        }
        let placed: usize = GROUPS.iter().map(|(_, _, intents)| intents.len()).sum();
        assert_eq!(placed, Intent::ALL.len(), "a group names an intent twice");
        let ids: Vec<&str> = GROUPS.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(ids, ["needs-landing", "needs-acceptance", "blocked", "ready", "working"]);
    }

    #[test]
    fn each_intent_ends_in_its_command_or_its_wait() {
        let id = "projects/bp/issues/0411.x";
        let m = "projects/bp/milestones/m";
        let held = Facts { holder: Some("worker".into()), since: Some("2026-10-13".into()), ..Facts::default() };
        let at = Facts { sha: Some("cc8a400e1f".into()), ..Facts::default() };
        let unread = Facts { unread: Some("forge not read since 2026-10-27T09:00:00Z".into()), ..Facts::default() };
        let escalated = Facts { escalated: Some(("worker".into(), "2026-10-20".into())), ..Facts::default() };
        let none = Facts::default();
        let cmd = |s: &str| Next::Command(s.to_string());
        let wait = |s: &str| Next::Wait(s.to_string());
        let table: Vec<(Intent, &str, &Facts, Next)> = vec![
            (Intent::Ready, "", &none, cmd("bp cut projects/bp/issues/0411.x")),
            (Intent::NeedsLanding, "", &none, cmd("bp land projects/bp/issues/0411.x")),
            (Intent::NeedsLanding, "", &unread, wait("forge not read since 2026-10-27T09:00:00Z")),
            (
                Intent::NeedsAcceptance,
                "bp stamp projects/bp/milestones/m",
                &none,
                cmd("bp stamp projects/bp/milestones/m"),
            ),
            (
                Intent::NeedsAcceptance,
                "a failed; bp stamp projects/bp/milestones/m --waive a --reason <why>",
                &none,
                cmd("bp stamp projects/bp/milestones/m --waive a --reason <why>"),
            ),
            (Intent::NeedsMilestoneApproval, "", &none, cmd("bp transition projects/bp/issues/0411.x approved")),
            (
                Intent::ReAsked,
                "f, g; bp stamp <next> --waive m/f --reason <why> --waive m/g --reason <why>",
                &none,
                wait(
                    "re-asked: f, g; waive again at the next stamp with \
                     --waive m/f --reason <why> --waive m/g --reason <why>",
                ),
            ),
            (
                Intent::Blocked,
                "integrity: r1; rerun bp check projects/bp/project --name integrity",
                &none,
                cmd("bp check projects/bp/project --name integrity"),
            ),
            (Intent::Blocked, "escalated", &escalated, wait("escalated by worker on 2026-10-20: answer in the issue")),
            (Intent::Blocked, "escalated", &none, wait("blocked: escalated")),
            (Intent::Blocked, "depends on missing x", &none, wait("blocked: depends on missing x")),
            (Intent::Blocked, "run failed at head", &held, wait("blocked: run failed at head")),
            (
                Intent::Drifted,
                "d/runs/r; bp walk d --for triage-drift",
                &none,
                wait("drifted: d/runs/r; bp walk d --for triage-drift"),
            ),
            (Intent::Working, "", &held, wait("held by worker since 2026-10-13")),
            (Intent::Working, "checks not green", &held, wait("held by worker since 2026-10-13; checks not green")),
            (Intent::Working, "checks not green", &none, wait("working: checks not green")),
            (Intent::Working, "stale", &held, cmd("bp release projects/bp/issues/0411.x --reason lost")),
            (
                Intent::Waiting,
                "waits for projects/bp/issues/0410.y",
                &none,
                wait("waits for projects/bp/issues/0410.y"),
            ),
            (Intent::NeedsReview, "", &at, wait("review dispatch")),
            (
                Intent::NeedsResponse,
                "no disposition: findings 2, 3",
                &at,
                wait("response dispatch: no disposition: findings 2, 3"),
            ),
            (Intent::NeedsApproval, "", &at, wait("awaits the reviewer's approval at cc8a400")),
            (
                Intent::NeedsDerivation,
                "a uncovered; bp walk m --for derive",
                &none,
                wait("session derives: a uncovered; bp walk m --for derive"),
            ),
            (
                Intent::NeedsShaping,
                "c open; bp walk d --for shape",
                &none,
                wait("session shapes: c open; bp walk d --for shape"),
            ),
        ];
        let mut seen = BTreeSet::new();
        for (intent, note, facts, want) in table {
            let subject = if matches!(intent, Intent::NeedsAcceptance) { m } else { id };
            assert_eq!(next(subject, intent, note, facts), want, "{intent:?} {note:?}");
            seen.insert(intent);
        }
        assert_eq!(seen.len(), Intent::ALL.len(), "the table covers every intent");
    }

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn issue(slug: &str, status: &str, more: &str) -> String {
        format!("---\nkind: issue\ntitle: {slug}\nstatus: {status}\nunder: projects/bp/milestones/m\n{more}---\n")
    }

    fn claim(issue: &str, from: &str) -> String {
        format!("---\nkind: claim\nunder: projects/bp/issues/{issue}\nfrom: {from}\nowner: worker\n---\n")
    }

    /// A project with rows of seven intents and a snapshot that holds one open pull request, and a project `q` whose
    /// root design needs shaping.
    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let now = crate::clock::instant(crate::clock::now().unwrap());
        write(root, "projects/bp/project.md", "---\nkind: project\ntitle: B\nstatus: active\nreviewer: rev\n---\n");
        write(root, "projects/bp/designs/system.md", "---\nkind: design\ntitle: S\nstatus: active\n---\n");
        write(root, "projects/bp/milestones/m.md", "---\nkind: milestone\ntitle: M\nstatus: planned\n---\n");
        write(root, "projects/bp/issues/0001.ready.md", &issue("0001.ready", "open", ""));
        write(root, "projects/bp/issues/0002.held.md", &issue("0002.held", "in-progress", ""));
        write(root, "projects/bp/issues/0002.held/claims/s1.md", &claim("0002.held", &now));
        write(root, "projects/bp/issues/0003.stale.md", &issue("0003.stale", "in-progress", ""));
        write(root, "projects/bp/issues/0003.stale/claims/s1.md", &claim("0003.stale", "2026-01-01T00:00:00Z"));
        let depends = "depends:\n  - projects/bp/issues/0002.held\n";
        write(root, "projects/bp/issues/0004.wait.md", &issue("0004.wait", "open", depends));
        write(root, "projects/bp/issues/0005.review.md", &issue("0005.review", "in-progress", ""));
        write(root, "projects/bp/issues/0005.review/claims/s1.md", &claim("0005.review", &now));
        write(root, "projects/bp/issues/0006.done.md", &issue("0006.done", "resolved", ""));
        write(root, "projects/bp/issues/0007.escalated.md", &issue("0007.escalated", "open", ""));
        let released = "\n## Released\n\nto: 2026-10-20T09:00:00Z\nreason: escalated\n";
        let escalated = claim("0007.escalated", "2026-10-20T08:00:00Z") + released;
        write(root, "projects/bp/issues/0007.escalated/claims/s1.md", &escalated);
        // A second project with no milestone and an open concern: its root design needs shaping.
        write(root, "projects/q/project.md", "---\nkind: project\ntitle: Q\nstatus: active\n---\n");
        write(root, "projects/q/designs/system.md", "---\nkind: design\ntitle: QS\nstatus: active\n---\n\nBody.\n");
        let answer = "---\nkind: concern-answer\nunder: projects/q/project\nconcern: c\nstate: open\n\
                      decided_at: 2026-10-19T10:00:00Z\n---\n";
        write(root, "projects/q/concerns/c/2026-10-19T10-00-00Z.md", answer);
        let pr = r#"[{"number":1,"headRefName":"bp/0005.review","headRefOid":"abc1234def","statusCheckRollup":[]}]"#;
        write(root, ".bp/forge/bp/open-prs.json", pr);
        write(root, "shared/.keep", "");
        let vault = Vault { root: root.to_path_buf() };
        (dir, vault)
    }

    #[test]
    fn the_rows_per_intent_are_the_homes_counts() {
        let (_dir, vault) = vault();
        let index = Index::open(&vault).unwrap();
        let m = moving(&vault, &index, "bp").unwrap();
        let home = crate::home(&vault, &index, "bp").unwrap();
        assert_eq!(counts(&m), home.intents);
        let want = BTreeMap::from([
            (Intent::Ready, 1),
            (Intent::Working, 2),
            (Intent::Waiting, 1),
            (Intent::NeedsReview, 1),
            (Intent::NeedsMilestoneApproval, 1),
            (Intent::Blocked, 1),
        ]);
        assert_eq!(counts(&m), want);
        assert!(matches!(m.forge, Forge::Snapshot) && m.forge_age.is_some_and(|a| a <= FRESH));
        let row = |id: &str| m.groups.iter().flat_map(|g| &g.rows).find(|r| r.id.ends_with(id)).unwrap();
        let ready = row("0001.ready");
        assert_eq!(ready.command.as_deref(), Some("bp cut projects/bp/issues/0001.ready"));
        assert_eq!((ready.head.as_deref(), ready.holder.as_deref()), (Some("no pull request"), None));
        let held = row("0002.held");
        assert!(held.wait.as_deref().is_some_and(|w| w.starts_with("held by worker since 20")), "{:?}", held.wait);
        assert_eq!(
            row("0003.stale").command.as_deref(),
            Some("bp release projects/bp/issues/0003.stale --reason lost")
        );
        assert_eq!(row("0004.wait").wait.as_deref(), Some("waits for projects/bp/issues/0002.held"));
        let review = row("0005.review");
        assert_eq!((review.head.as_deref(), review.wait.as_deref()), (Some("checks pending"), Some("review dispatch")));
        assert_eq!(review.holder.as_deref(), Some("worker"));
        let milestone = row("milestones/m");
        assert_eq!((milestone.kind.as_str(), milestone.head.as_deref()), ("milestone", None));
        assert_eq!(milestone.command.as_deref(), Some("bp transition projects/bp/milestones/m approved"));
        // Working holds its own intent first, then the others in group order; an empty group stays.
        let working: Vec<Intent> = m.groups[4].rows.iter().map(|r| r.intent).collect();
        assert_eq!(working, [Intent::Working, Intent::Working, Intent::Waiting, Intent::NeedsReview]);
        let blocked = row("0007.escalated");
        assert_eq!(blocked.wait.as_deref(), Some("escalated by worker on 2026-10-20: answer in the issue"));
        assert_eq!(blocked.holder, None);
        assert!(m.groups[0].rows.is_empty() && m.groups[3].rows.len() == 1);
        for r in m.groups.iter().flat_map(|g| &g.rows) {
            assert!(r.command.is_some() != r.wait.is_some(), "{}", r.id);
        }
    }

    #[test]
    fn a_stale_snapshot_withholds_the_head() {
        let (_dir, vault) = vault();
        let snapshot = vault.root.join(".bp/forge/bp/open-prs.json");
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&snapshot).unwrap().set_modified(hour_ago).unwrap();
        let index = Index::open(&vault).unwrap();
        let m = moving(&vault, &index, "bp").unwrap();
        assert!(m.forge_age.is_some_and(|a| a > FRESH), "{:?}", m.forge_age);
        let issues: Vec<&MovingRow> = m.groups.iter().flat_map(|g| &g.rows).filter(|r| r.kind == "issue").collect();
        assert!(!issues.is_empty());
        for r in issues {
            assert_eq!(r.head.as_deref(), Some("forge not read"), "{}", r.id);
        }
    }

    #[test]
    fn a_design_row_waits_on_the_session() {
        let (_dir, vault) = vault();
        let index = Index::open(&vault).unwrap();
        let m = moving(&vault, &index, "q").unwrap();
        assert_eq!(counts(&m), crate::home(&vault, &index, "q").unwrap().intents);
        let rows: Vec<&MovingRow> = m.groups.iter().flat_map(|g| &g.rows).collect();
        let [design] = rows.as_slice() else { panic!("one row: {:?}", rows.iter().map(|r| &r.id).collect::<Vec<_>>()) };
        assert_eq!((design.id.as_str(), design.kind.as_str()), ("projects/q/designs/system", "design"));
        assert_eq!(design.intent, Intent::NeedsShaping);
        assert_eq!(
            design.wait.as_deref(),
            Some("session shapes: c open; bp walk projects/q/designs/system --for shape")
        );
        assert_eq!((design.command.as_deref(), design.holder.as_deref(), design.head.as_deref()), (None, None, None));
        assert!(m.groups[4].rows.iter().any(|r| r.id == design.id), "needs shaping sits in working");
    }

    #[test]
    fn a_project_the_index_lacks_refuses_no_node() {
        let (_dir, vault) = vault();
        let index = Index::open(&vault).unwrap();
        assert_eq!(moving(&vault, &index, "nope").err().map(|r| r.code), Some(bp_vault::Code::NoNode));
    }

    fn counts(m: &Moving) -> BTreeMap<Intent, usize> {
        let mut out = BTreeMap::new();
        for r in m.groups.iter().flat_map(|g| &g.rows) {
            *out.entry(r.intent).or_default() += 1;
        }
        out
    }
}
