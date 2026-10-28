//! The Milestone page object (18, Milestone; 32, design change 8): `show` of one milestone with its checklist. The
//! rows are the finish line's blocks with their last runs, the deltas it carries, the project's concerns and
//! waivers, and the issues under it with their intents from the one `Intents` call. It ends in the milestone row's own
//! Moving command or wait, from the one function that builds both: `bp transition <id> approved` while it is planned,
//! `bp stamp <id>` when it needs acceptance. A milestone with no intent has no Moving row, so the page says why in
//! words. A row whose record kind the vault holds none of reads "not in vault" (18, Open).

use std::collections::{BTreeMap, BTreeSet};

use bp_change::intents::Intent;
use bp_change::{IssueStatus, MilestoneStatus};
use bp_vault::{Body, Code, DeltaState, Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde::Serialize;
use ts_rs::TS;

use crate::inputs::{open_waivers, text};
use crate::moving::{Facts, Next};
use crate::{ALL, ChangeDelta, Intents, ListRow, Show, show};

/// One milestone's page. `command` is present only when its Moving row carries one.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Milestone {
    /// The `show` object: the header, the lead, the section headings and the body.
    #[serde(flatten)]
    pub show: Show,
    /// The milestone's intent, as `list` prints it; absent when none holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<Intent>,
    /// The intent's note with the command; without it, the Moving row's wait, or why no row exists, in words.
    pub note: String,
    /// The named check blocks of its body, in order, each with its last run.
    pub finish_line: Vec<FinishBlock>,
    /// The delta records of the project's designs that name it as their milestone.
    pub deltas: Vec<ChangeDelta>,
    /// Each concern's latest answer, then each waiver the stamp asks again or this milestone holds.
    pub concerns: Vec<Concern>,
    /// The issues under it, as `list issue --under` prints them.
    pub issues: Vec<ListRow>,
    /// The command the milestone's Moving row carries, when it carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The record kinds a checklist row reads that the vault holds none of: those rows read "not in vault".
    pub rows_not_in_vault: Vec<String>,
}

/// One block of the finish line.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct FinishBlock {
    /// The block's name.
    pub name: String,
    /// Its runner: `shell`, `scenario`, `deps` and the rest of 15's grammar.
    pub kind: String,
    /// `error` or `warn`; a block without one is `error` (15).
    pub severity: String,
    /// The latest run of the block's current text; absent when none exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<LastRun>,
}

/// A run record, reduced to what the checklist shows.
#[derive(Serialize, TS)]
pub struct LastRun {
    /// The run record's id.
    pub id: String,
    /// `pass`, `fail`, `timed-out`, `skipped` or `could-not-run`.
    pub verdict: String,
    /// When it ran.
    pub at: String,
    /// The commit it ran at.
    pub head: String,
}

/// A concern's latest answer, or a waiver of a finish-line block.
#[derive(Serialize, TS)]
#[ts(optional_fields)]
pub struct Concern {
    /// The concern's slug, or the waived block as `--waive` names it.
    pub key: String,
    /// The design that answers it, or the reason it was waived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Until when a waiver holds; absent means the next stamp asks again (13).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    /// A concern's `designed`, `waived` or `open`; a waiver's `waived`, or `re-asked` when the stamp asks again.
    pub state: String,
}

/// The Milestone page of `id` in `project`. An id that names no milestone refuses `no_node`; when issues name it as
/// their milestone, the fix names them (18, An error case).
pub fn milestone(vault: &Vault, index: &Index, project: &str, id: &str) -> Result<Milestone, Refusal> {
    let id = crate::subject(index, id)?;
    let parts: Vec<&str> = id.as_str().split('/').collect();
    let header =
        index.header(id.as_str())?.filter(|_| matches!(parts[..], ["projects", p, "milestones", _] if p == project));
    let Some(header) = header else { return Err(missing(index, project, &id)?) };
    let status = text(&header, "status").unwrap_or_default();
    let intents = Intents::new(vault, index, Some(&BTreeSet::from([project])))?;
    let verdict = intents.row(Kind::Milestone, id.as_str(), Some(&status))?;
    let under =
        Filter { kind: Kind::Issue, status: None, project: Some(project), under: Some(id.as_str()), limit: ALL };
    let mut issues = Vec::new();
    for row in index.list(&under)?.0 {
        let v = intents.row(Kind::Issue, &row.id, row.status.as_deref())?;
        issues.push(ListRow { row, stale: None, note: v.intent.map(|_| v.note), intent: v.intent });
    }
    let show = show(vault, Some(index), &id, None, None, false)?;
    let runs = index.records(id.as_str(), Kind::Run)?;
    let finish_line = Body::parse(show.body.as_deref().unwrap_or_default())
        .named_checks()
        .into_iter()
        .map(|c| {
            let name = c.attrs["name"].clone();
            FinishBlock {
                last_run: last_run(&runs, &name, &c.hash()),
                kind: c.runner.clone(),
                severity: c.attrs.get("severity").cloned().unwrap_or_else(|| "error".into()),
                name,
            }
        })
        .collect();
    let mut concerns = concerns(index, project)?;
    concerns.extend(waivers(vault, index, project, &id)?);
    // A milestone row's facts are its intent and note: the claim, the pull request and the snapshot are an issue's.
    let (command, note) = match verdict.intent {
        Some(intent) => match crate::moving::next(id.as_str(), intent, &verdict.note, &Facts::default()) {
            Next::Command(c) => (Some(c), verdict.note.clone()),
            Next::Wait(w) => (None, w),
        },
        None => (None, wait(&id, &status, &issues)),
    };
    Ok(Milestone {
        intent: verdict.intent,
        note,
        finish_line,
        deltas: carried(index, project, &id)?,
        concerns,
        issues,
        command,
        rows_not_in_vault: missing_kinds(index, &[Kind::Run, Kind::Delta, Kind::ConcernAnswer, Kind::Waiver])?,
        show,
    })
}

/// The latest run of the block `name` at its text now, by time then attempt. A run of an older text says nothing,
/// as for `stamp`'s failed blocks.
fn last_run(runs: &[Record], name: &str, hash: &str) -> Option<LastRun> {
    let field = |r: &Record, k: &str| text(&r.header, k).unwrap_or_default();
    let mine = runs.iter().filter(|r| field(r, "check") == name && field(r, "check_hash") == hash);
    let attempt = |r: &Record| r.header.get("attempt").and_then(serde_json::Value::as_u64).unwrap_or_default();
    mine.max_by_key(|r| (field(r, "at"), attempt(r))).map(|r| LastRun {
        id: r.id.clone(),
        verdict: field(r, "verdict"),
        at: field(r, "at"),
        head: field(r, "commit"),
    })
}

/// Why a milestone with no intent, so no Moving row, waits: it is done or abandoned, or approved with an issue open
/// or a file of the project that does not parse (21: a planned milestone always has an intent).
fn wait(id: &Id, status: &str, issues: &[ListRow]) -> String {
    let settled =
        |s: Option<&str>| matches!(s.map(str::parse), Some(Ok(IssueStatus::Resolved | IssueStatus::Abandoned)));
    let open = issues.iter().filter(|i| !settled(i.row.status.as_deref())).count();
    match status.parse() {
        Ok(MilestoneStatus::Done) => "accepted: done".to_string(),
        Ok(MilestoneStatus::Abandoned) => "abandoned".to_string(),
        _ if open > 0 => {
            let n = if open == 1 { "1 issue is".to_string() } else { format!("{open} issues are") };
            format!("{n} not resolved or abandoned; bp list issue --under {id}")
        }
        _ => format!("a file of the project does not parse; bp list --count-only names it, then bp stamp {id}"),
    }
}

/// The delta records of the project's designs whose `milestone` is `id`, with their states.
///
/// # Panics
///
/// Never: the index holds ids.
fn carried(index: &Index, project: &str, id: &Id) -> Result<Vec<ChangeDelta>, Refusal> {
    let mut out = Vec::new();
    for design in index.ids_of(Kind::Design, project)? {
        for d in index.records(&design, Kind::Delta)? {
            if text(&d.header, "milestone").as_deref() == Some(id.as_str()) {
                let state = DeltaState::of(&Body::parse(&d.body)).as_str().to_string();
                out.push(ChangeDelta { id: d.id, target: design.clone(), state });
            }
        }
    }
    Ok(out)
}

/// Each concern's latest answer in the project (13, ConcernAnswer): the record with the latest stamp.
pub(crate) fn concerns(index: &Index, project: &str) -> Result<Vec<Concern>, Refusal> {
    let mut latest: BTreeMap<String, Record> = BTreeMap::new();
    for r in index.records(&format!("projects/{project}/project"), Kind::ConcernAnswer)? {
        let concern = text(&r.header, "concern").unwrap_or_default();
        if latest.get(&concern).is_none_or(|l| l.id < r.id) {
            latest.insert(concern, r);
        }
    }
    Ok(latest
        .into_iter()
        .map(|(key, r)| Concern {
            answer: text(&r.header, "design").or_else(|| text(&r.header, "reason")),
            valid_until: text(&r.header, "valid_until").filter(|t| !t.is_empty()),
            state: text(&r.header, "state").unwrap_or_default(),
            key,
        })
        .collect())
}

/// The project's open waivers, which the next stamp asks again (`re-asked`), then this milestone's other waivers.
fn waivers(vault: &Vault, index: &Index, project: &str, id: &Id) -> Result<Vec<Concern>, Refusal> {
    let open = open_waivers(vault, index, project, crate::clock::now()?)?;
    let row = |r: &Record, key: String, state: &str| Concern {
        key,
        answer: text(&r.header, "reason"),
        valid_until: text(&r.header, "valid_until").filter(|t| !t.is_empty()),
        state: state.to_string(),
    };
    let mut out = Vec::new();
    for w in &open {
        if let Some(r) = index.records(w.milestone.as_str(), Kind::Waiver)?.iter().find(|r| r.id == w.waiver.as_str()) {
            out.push(row(r, w.key(), "re-asked"));
        }
    }
    for r in index.records(id.as_str(), Kind::Waiver)? {
        if !open.iter().any(|w| w.waiver.as_str() == r.id) {
            let key = text(&r.header, "check").unwrap_or_else(|| r.id.rsplit('/').next().unwrap_or_default().into());
            out.push(row(&r, key, "waived"));
        }
    }
    Ok(out)
}

/// The kinds among `kinds` that the vault holds no record of.
pub(crate) fn missing_kinds(index: &Index, kinds: &[Kind]) -> Result<Vec<String>, Refusal> {
    let mut out = Vec::new();
    for &kind in kinds {
        if index.list(&Filter { kind, status: None, project: None, under: None, limit: 0 })?.1 == 0 {
            out.push(kind.as_str().to_string());
        }
    }
    Ok(out)
}

/// The refusal for a milestone id the vault lacks. Issues that name it get the fix the integrity check implies: make
/// the milestone, or move each issue.
fn missing(index: &Index, project: &str, id: &Id) -> Result<Refusal, Refusal> {
    let f = Filter { kind: Kind::Issue, status: None, project: None, under: Some(id.as_str()), limit: ALL };
    let (rows, total) = index.list(&f)?;
    let rule = "a milestone page shows one milestone of its project: /<project>/milestone/projects/<project>/milestones/<slug>";
    // `create` makes the id from the title's slug, so the title is the missing slug in words (19, principle 4: no
    // placeholder a human must fill but `<why>`).
    let slug = id.as_str().rsplit('/').next().unwrap_or_default();
    let mut title = slug.replace('-', " ");
    if let Some(c) = title.get_mut(..1) {
        c.make_ascii_uppercase();
    }
    let fix = match rows.first() {
        None => format!("bp list milestone --project {project}"),
        Some(first) => format!(
            "{} it, but the vault holds no milestone there; bp create milestone --project {project} --title \
             \"{title}\" makes it, or bp list milestone --project {project} names the milestones and bp set {} under \
             one of them moves the issue; bp list issue --under {id} lists them",
            if total == 1 { "1 issue names".to_string() } else { format!("{total} issues name") },
            first.id
        ),
    };
    Ok(Refusal::new(Code::NoNode, id.as_str(), rule, fix))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "## Finish line\n\n```check shell {name=a expect=0}\ntrue\n```\n\n```check scenario {name=b \
                        measure=ratio max=1 severity=warn}\ntrue\n```\n";

    fn vault(files: &[(&str, String)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let mut all = vec![
            ("projects/bp/project.md", "---\nkind: project\ntitle: B\nstatus: active\n---\n".to_string()),
            ("shared/.keep", String::new()),
        ];
        all.extend(files.iter().cloned());
        for (rel, text) in &all {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let vault = Vault { root: dir.path().to_path_buf() };
        (dir, vault)
    }

    fn node(kind: &str, status: &str, more: &str) -> String {
        format!("---\nkind: {kind}\ntitle: T\nstatus: {status}\n{more}---\n\n")
    }

    /// An issue's `## Check` naming both finish-line blocks, so the line is covered and no derivation is due.
    const COVERS: &str = "## Check\n\n```check shell {name=a expect=0}\ntrue\n```\n\n```check shell {name=b expect=0}\n\
                          true\n```\n";

    fn page(vault: &Vault, id: &str) -> Result<Milestone, Refusal> {
        milestone(vault, &Index::open(vault).unwrap(), "bp", id)
    }

    /// The staged copy's planned milestone: no runs, and no delta, concern or waiver record in the vault.
    #[test]
    fn a_planned_milestone_with_no_records_reads_not_run_and_not_in_vault() {
        let (_d, v) = vault(&[("projects/bp/milestones/m.md", node("milestone", "planned", "") + LINE)]);
        let m = page(&v, "projects/bp/milestones/m").unwrap();
        let rows: Vec<(&str, &str, &str, bool)> = m
            .finish_line
            .iter()
            .map(|b| (b.name.as_str(), b.kind.as_str(), b.severity.as_str(), b.last_run.is_some()))
            .collect();
        assert_eq!(rows, [("a", "shell", "error", false), ("b", "scenario", "warn", false)]);
        assert_eq!(m.rows_not_in_vault, ["run", "delta", "concern-answer", "waiver"]);
        assert!(m.deltas.is_empty() && m.concerns.is_empty() && m.issues.is_empty());
        let approve = "bp transition projects/bp/milestones/m approved";
        assert_eq!((m.intent, m.command.as_deref()), (Some(Intent::NeedsMilestoneApproval), Some(approve)));
        assert_eq!(m.show.header.get("title").and_then(|t| t.as_str()), Some("T"), "the show object is flat");
    }

    /// 32, design change 8: the page's chip is the milestone row's own Moving command, from the one function.
    #[test]
    fn the_command_is_the_moving_rows_for_a_planned_and_a_settled_milestone() {
        let (_d, v) = vault(&[
            ("projects/bp/milestones/p.md", node("milestone", "planned", "") + LINE),
            ("projects/bp/milestones/s.md", node("milestone", "approved", "") + LINE),
            ("projects/bp/issues/0001.a.md", node("issue", "resolved", "under: projects/bp/milestones/s\n") + COVERS),
        ]);
        let index = Index::open(&v).unwrap();
        let moving = crate::moving::moving(&v, &index, "bp").unwrap();
        let row =
            |id: &str| moving.groups.iter().flat_map(|g| &g.rows).find(|r| r.id == id).and_then(|r| r.command.clone());
        for (id, want) in [
            ("projects/bp/milestones/p", "bp transition projects/bp/milestones/p approved"),
            ("projects/bp/milestones/s", "bp stamp projects/bp/milestones/s"),
        ] {
            assert_eq!(page(&v, id).unwrap().command, row(id), "{id}");
            assert_eq!(row(id).as_deref(), Some(want), "{id}");
        }
    }

    #[test]
    fn a_settled_milestone_shows_the_stamp_and_its_rows() {
        let hash = Body::parse(LINE).named_checks()[0].hash();
        let run = |n: u64, verdict: &str, hash: &str| {
            let h = format!("kind: run\nunder: projects/bp/milestones/m\ncheck: a\ncheck_hash: {hash}\ncommit: c{n}\n");
            (format!("{h}attempt: {n}\nat: 2026-10-2{n}T09:00:00Z\nverdict: {verdict}\n"), n)
        };
        let (r1, r2, r3) = (run(1, "fail", &hash), run(2, "pass", &hash), run(3, "fail", "older"));
        let delta = "---\nkind: delta\nunder: projects/bp/designs/system\nmilestone: projects/bp/milestones/m\n\
                     stamp: s1\n---\n\n## Proposed\n\nx\n";
        let answer = |state: &str, at: &str| {
            format!(
                "---\nkind: concern-answer\nunder: projects/bp/project\nconcern: privacy\nstate: {state}\n\
                     reason: local only\ndecided_at: {at}\n---\n"
            )
        };
        let (_d, v) = vault(&[
            ("projects/bp/milestones/m.md", node("milestone", "approved", "") + LINE),
            ("projects/bp/designs/system.md", node("design", "active", "")),
            ("projects/bp/designs/system/deltas/s1.md", delta.into()),
            ("projects/bp/concerns/privacy/2026-10-01T00-00-00Z.md", answer("open", "2026-10-01T00:00:00Z")),
            ("projects/bp/concerns/privacy/2026-10-02T00-00-00Z.md", answer("waived", "2026-10-02T00:00:00Z")),
            ("projects/bp/issues/0001.a.md", node("issue", "resolved", "under: projects/bp/milestones/m\n") + COVERS),
            ("projects/bp/milestones/m/runs/a-1.md", format!("---\n{}---\n", r1.0)),
            ("projects/bp/milestones/m/runs/a-2.md", format!("---\n{}---\n", r2.0)),
            ("projects/bp/milestones/m/runs/a-3.md", format!("---\n{}---\n", r3.0)),
        ]);
        let m = page(&v, "projects/bp/milestones/m").unwrap();
        let last = m.finish_line[0].last_run.as_ref().map(|r| (r.verdict.as_str(), r.head.as_str()));
        assert_eq!(last, Some(("pass", "c2")), "the run of an older text says nothing");
        assert_eq!(m.command.as_deref(), Some("bp stamp projects/bp/milestones/m"));
        assert_eq!(m.note, "bp stamp projects/bp/milestones/m");
        let delta = m.deltas.iter().map(|d| (d.target.as_str(), d.state.as_str())).collect::<Vec<_>>();
        assert_eq!(delta, [("projects/bp/designs/system", "proposed")]);
        let concern =
            m.concerns.iter().map(|c| (c.key.as_str(), c.state.as_str(), c.answer.as_deref())).collect::<Vec<_>>();
        assert_eq!(concern, [("privacy", "waived", Some("local only"))], "the latest answer");
        assert_eq!(m.issues.iter().map(|i| i.row.id.as_str()).collect::<Vec<_>>(), ["projects/bp/issues/0001.a"]);
        assert_eq!(m.rows_not_in_vault, ["waiver"]);
    }

    #[test]
    fn an_open_issue_withholds_the_stamp_and_a_missing_milestone_names_its_issues() {
        let issue = node("issue", "open", "under: projects/bp/milestones/m\n") + COVERS;
        let (_d, v) = vault(&[
            ("projects/bp/milestones/m.md", node("milestone", "approved", "") + LINE),
            ("projects/bp/issues/0001.a.md", issue.clone()),
            ("projects/bp/issues/0002.b.md", issue),
            ("projects/bp/issues/0003.c.md", node("issue", "open", "under: projects/bp/milestones/gone\n")),
        ]);
        let m = page(&v, "projects/bp/milestones/m").unwrap();
        assert_eq!(m.command, None);
        assert_eq!(m.note, "2 issues are not resolved or abandoned; bp list issue --under projects/bp/milestones/m");
        let refusal = page(&v, "projects/bp/milestones/gone").err().unwrap();
        assert_eq!(refusal.code, Code::NoNode);
        assert!(
            refusal.fix.starts_with("1 issue names it, but the vault holds no milestone there;"),
            "{}",
            refusal.fix
        );
        let create = "bp create milestone --project bp --title \"Gone\" makes it, or bp list milestone --project bp \
                      names the milestones and bp set projects/bp/issues/0003.c under one of them moves the issue";
        assert!(refusal.fix.contains(create), "{}", refusal.fix);
        assert!(!refusal.fix.contains('<'), "no placeholder: {}", refusal.fix);
        assert_eq!(page(&v, "projects/bp/issues/0001.a").err().map(|r| r.code), Some(Code::NoNode), "not a milestone");
    }
}
