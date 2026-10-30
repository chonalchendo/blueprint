//! Composes the `Intents` function's inputs from the index and the forge snapshot. The function itself is pure
//! and lives in `bp-change`; this is the IO around it (23, rule 2).

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use bp_change::MilestoneStatus;
use bp_change::intents::{self, Base, Dependency, IntegrityRun, Issue, Milestone, Verdict};
use bp_design::DesignStatus;
use bp_reasoning::LearningStatus;
use bp_vault::{Body, Code, DeltaState, DesignLevel, Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde_json::{Map, Value};

use crate::World;
use crate::inputs::{blocks, claims, derivation, drift, findings, pull_request, reviewer, runs, text};

/// What one call reads to compute intents: the index, the world when an issue is in view, and what each project
/// holds for every node in it, read once per project (24, call budget).
pub struct Intents<'a> {
    vault: &'a Vault,
    index: &'a Index,
    /// The clock and the forge, read only when an issue is in view.
    pub world: Option<World>,
    integrity: RefCell<BTreeMap<String, Option<IntegrityRun>>>,
    /// Each design's latest acceptance, read once per call for `stale`.
    accepted: RefCell<BTreeMap<String, Option<String>>>,
    /// Each learning's creation, read once per call for `stale` with no evidence row. Every learning lives in
    /// `shared/learnings` (13), so one read serves them all.
    created: OnceCell<BTreeMap<String, String>>,
    /// Each project's unproven concerns, and whether a milestone is `planned` or `approved` (needs shaping).
    shaping: RefCell<BTreeMap<String, (Vec<String>, bool)>>,
    /// Whether a file of each project fails to parse (needs acceptance).
    broken: RefCell<BTreeMap<String, bool>>,
    /// Each project's open waivers, as milestone and block (re-asked).
    reasked: RefCell<BTreeMap<String, Vec<(String, String)>>>,
}

impl<'a> Intents<'a> {
    /// Reads the world only when issues are in view, with the snapshots of their `projects`: a milestone's intent
    /// needs neither the clock nor the forge.
    pub fn new(vault: &'a Vault, index: &'a Index, projects: Option<&BTreeSet<&str>>) -> Result<Self, Refusal> {
        let world = projects.map(|p| World::read(&vault.root, p)).transpose()?;
        Ok(Self {
            vault,
            index,
            world,
            integrity: RefCell::default(),
            accepted: RefCell::default(),
            created: OnceCell::new(),
            shaping: RefCell::default(),
            broken: RefCell::default(),
            reasked: RefCell::default(),
        })
    }

    /// The intent of one issue, milestone or design; no other kind has one.
    ///
    /// A live issue needs the world, so [`Intents::new`] must have been given its project, else this refuses
    /// `could_not_run`. An id the index lacks refuses `no_node`.
    pub fn row(&self, kind: Kind, id: &str, status: Option<&str>) -> Result<Verdict, Refusal> {
        let status = status.unwrap_or_default();
        match kind {
            // A settled issue has no intent, so it reads nothing: most issues a walk reaches are settled.
            Kind::Issue if !intents::is_live_issue(status) => Ok(Verdict::NONE),
            Kind::Issue => {
                let Some(world) = self.world.as_ref() else {
                    return Err(Refusal::failed(
                        id,
                        "a live issue's intent reads the clock and the forge; pass the issue's project to Intents::new",
                    ));
                };
                of(self.vault, self.index, world, id, status, self.integrity(id)?)
            }
            Kind::Milestone => self.milestone(id, status),
            Kind::Design => {
                let uncited: Vec<String> = drift(self.index, id)?.into_iter().map(|r| r.id).collect();
                // Shaping reads the project's concern answers only for its active root design, if it has not drifted.
                let (unproven, due) = match project(id) {
                    Some(p)
                        if uncited.is_empty()
                            && status == DesignStatus::Active.as_str()
                            && root(self.index, &p, id)? =>
                    {
                        once(&self.shaping, &p, || shaping(self.index, &p))?
                    }
                    _ => (Vec::new(), false),
                };
                Ok(intents::design(id, &uncited, &unproven, due))
            }
            _ => Ok(Verdict::NONE),
        }
    }

    /// A learning's derived `stale` (13, Learning), from its header. Evidence is read only when a cited design
    /// changed at all.
    pub fn stale(&self, id: &str, header: &Map<String, Value>) -> Result<bool, Refusal> {
        let mut changed = Vec::new();
        let mut cache = self.accepted.borrow_mut();
        let cites = header.get("cites").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
        for d in cites.filter(|d| Id::parse(d).ok().and_then(|d| DesignLevel::of(&d)).is_some()) {
            if !cache.contains_key(d) {
                cache.insert(d.to_string(), accepted(self.index, d)?);
            }
            changed.extend(cache[d].clone());
        }
        if changed.is_empty() {
            return Ok(false);
        }
        // With no evidence row, the last verification is the learning's creation (13, Learning).
        let created = || {
            let dir = format!("shared/{}", Kind::Learning.dir());
            self.created.get_or_init(|| self.vault.created(&dir)).get(id).cloned()
        };
        let last = self.index.records(id, Kind::Evidence)?.pop().map_or_else(created, |r| text(&r.header, "at"));
        Ok(LearningStatus::stale(&text(header, "status").unwrap_or_default(), last.as_deref(), &changed))
    }

    /// The blocked note of the project's counting integrity run, when it failed and names `id` (21, blocked).
    fn integrity(&self, id: &str) -> Result<Option<String>, Refusal> {
        let Some(project) = project(id) else { return Ok(None) };
        // The run is answered in place, not cloned through `once`: it names every node it failed, and a walk asks
        // once per live node.
        if !self.integrity.borrow().contains_key(&project) {
            let runs: Vec<IntegrityRun> = self.index.integrity_runs(&project)?.iter().map(integrity_run).collect();
            self.integrity.borrow_mut().insert(project.clone(), IntegrityRun::counting(&runs).cloned());
        }
        Ok(self.integrity.borrow()[&project].as_ref().and_then(|r| r.blocks(id)))
    }

    /// The intent of one milestone. Each read waits for the status, and the earlier reads, that let it matter.
    fn milestone(&self, id: &str, status: &str) -> Result<Verdict, Refusal> {
        let project = project(id).expect("a milestone sits under a project");
        let mut m = Milestone::default();
        match status.parse() {
            // Blocked holds only before `done` (21).
            Ok(MilestoneStatus::Planned) => m.integrity = self.integrity(id)?,
            Ok(MilestoneStatus::Approved) => {
                m = derivation(self.index, id)?;
                m.integrity = self.integrity(id)?;
                if !m.issues.iter().any(|s| intents::is_live_issue(s)) {
                    m.runs = runs(self.index, id)?;
                    m.broken = once(&self.broken, &project, || Ok(!self.index.broken(&project)?.is_empty()))?;
                }
            }
            Ok(MilestoneStatus::Done) => {
                // The decision `stamp` makes, read once per project (27, design change 10).
                let open = once(&self.reasked, &project, || {
                    let open = crate::inputs::open_waivers(self.vault, self.index, &project, crate::clock::now()?)?;
                    Ok(open.into_iter().map(|r| (r.milestone.to_string(), r.name)).collect())
                })?;
                m.reasked = open.into_iter().filter(|(of, _)| of == id).map(|(_, block)| block).collect();
            }
            _ => {}
        }
        m.status = status.into();
        Ok(intents::milestone(id, &m))
    }
}

/// The value of `key` in `cache`, read once per call.
fn once<T: Clone>(
    cache: &RefCell<BTreeMap<String, T>>,
    key: &str,
    read: impl FnOnce() -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    if let Some(v) = cache.borrow().get(key) {
        return Ok(v.clone());
    }
    let v = read()?;
    cache.borrow_mut().insert(key.to_string(), v.clone());
    Ok(v)
}

/// The node's project, when it has one.
fn project(id: &str) -> Option<String> {
    Id::parse(id).ok().and_then(|i| i.project().map(str::to_string))
}

/// `id` is the project's root design, its system design, else its product design: needs shaping's one node (21).
fn root(index: &Index, project: &str, id: &str) -> Result<bool, Refusal> {
    let system = format!("projects/{project}/designs/system");
    Ok(id == system || (id == format!("projects/{project}/designs/product") && index.header(&system)?.is_none()))
}

/// The project's unproven concerns, and whether a milestone of it is `planned` or `approved` (21, needs shaping). The
/// version records are read only when a concern could be unproven, and the milestones only when one is.
fn shaping(index: &Index, project: &str) -> Result<(Vec<String>, bool), Refusal> {
    let under = format!("projects/{project}/project");
    let answers: Vec<intents::ConcernAnswer> = index
        .records(&under, Kind::ConcernAnswer)?
        .iter()
        .map(|r| intents::ConcernAnswer {
            concern: text(&r.header, "concern").unwrap_or_default(),
            stamp: r.id.rsplit('/').next().unwrap_or_default().to_string(),
            state: text(&r.header, "state").unwrap_or_default(),
            decided_at: text(&r.header, "decided_at").and_then(|t| intents::timestamp(&t)),
        })
        .collect();
    if intents::unproven_concerns(&answers, None).is_empty() {
        return Ok((Vec::new(), false));
    }
    let unproven = intents::unproven_concerns(&answers, last_stamp(index, &under)?);
    if unproven.is_empty() {
        return Ok((unproven, false));
    }
    let mut due = false;
    for status in [MilestoneStatus::Planned, MilestoneStatus::Approved] {
        let f = Filter {
            kind: Kind::Milestone,
            status: Some(status.as_str()),
            project: Some(project),
            under: None,
            limit: 1,
        };
        due |= index.list(&f)?.1 > 0;
    }
    Ok((unproven, due))
}

/// The project's latest stamp of a `done` milestone: the `accepted_at` of the version record `stamp` writes for it.
fn last_stamp(index: &Index, project: &str) -> Result<Option<i64>, Refusal> {
    let mut last = None;
    for v in index.records(project, Kind::BlueprintVersion)? {
        let Some(m) = text(&v.header, "milestone") else { continue };
        let done = index.header(&m)?.and_then(|h| text(&h, "status"));
        if done.as_deref() == Some(MilestoneStatus::Done.as_str()) {
            last = last.max(text(&v.header, "accepted_at").and_then(|t| intents::timestamp(&t)));
        }
    }
    Ok(last)
}

/// When a design last changed version: the latest `accepted_at` of its accepted deltas (13, Delta).
fn accepted(index: &Index, design: &str) -> Result<Option<String>, Refusal> {
    let at = |d: &Record| {
        let body = Body::parse(&d.body);
        let last = body.sections.last()?.text.lines().find_map(|l| l.strip_prefix("accepted_at: "))?;
        Some(last.trim().trim_matches('"').to_string())
    };
    let deltas = index.records(design, Kind::Delta)?;
    let accepted = deltas.iter().filter(|d| text(&d.header, "state").as_deref() == Some(DeltaState::Accepted.as_str()));
    Ok(accepted.filter_map(at).max())
}

/// The intent of one live issue.
fn of(
    vault: &Vault,
    index: &Index,
    world: &World,
    id: &str,
    status: &str,
    integrity: Option<String>,
) -> Result<Verdict, Refusal> {
    let Some(header) = index.header(id)? else {
        return Err(Refusal::new(
            Code::NoNode,
            id,
            "an id names a node or record in the index",
            format!("bp search {}", id.rsplit('/').next().unwrap_or(id).replace(['-', '.', '_'], " ")),
        ));
    };
    let mut depends = Vec::new();
    for d in header.get("depends").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let status = index.header(d)?.map(|h| text(&h, "status").unwrap_or_default());
        let open_pr = world.pr(d).is_some();
        // The stack test reads the layers below a target only when `cut` would stack on it.
        let layers = if open_pr && status.as_deref() != Some("resolved") {
            crate::inputs::layers(index, d)?
        } else {
            Vec::new()
        };
        depends.push(Dependency { id: d.to_string(), status, open_pr, layers });
    }
    let records = index.records(id, Kind::Claim)?;
    let claims = claims(&records);
    let runs = runs(index, id)?;
    let reviews = index.records(id, Kind::Review)?;
    let review_heads = reviews.iter().filter_map(|r| text(&r.header, "head")).collect();
    // A merged pull request under a held claim is a land that stopped before its commit: `land` finishes it, through
    // the same gates.
    let held = claims.iter().any(|c| c.released.is_none());
    let open = world.pr(id).or_else(|| world.merged(id).filter(|_| held));
    // The body, the reviewer and the base matter only to needs landing, which needs a pull request.
    let (pr, blocks, base) = match open {
        Some(p) => {
            let claim = records.iter().zip(&claims).find(|(_, c)| c.released.is_none()).map(|(r, _)| r);
            let base = match claim {
                Some(c) => crate::inputs::base(index, c)?,
                None => Base::Unknown,
            };
            (Some(pull_request(p, &reviewer(index, id)?)), blocks(vault, id)?, base)
        }
        None => (None, Vec::new(), Base::Unknown),
    };
    // Git is asked only when an escalation needs the answer: one process per escalated issue, none otherwise.
    let answered = intents::latest_claim(&claims).filter(|c| c.escalated).is_some_and(|latest| {
        let record = &records[claims.iter().position(|c| std::ptr::eq(c, latest)).expect("the latest is one of them")];
        Id::parse(id).ok().zip(Id::parse(&record.id).ok()).is_some_and(|(id, claim)| vault.touched_after(&id, &claim))
    });
    // Dispositions are read only for a review at the head in view (21, needs response).
    let at_head = pr
        .as_ref()
        .and_then(|p| reviews.iter().rev().find(|r| text(&r.header, "head").as_deref() == Some(p.head.as_str())));
    let unanswered = at_head.map(|r| findings(index, r)).transpose()?.map(|(open, _)| open).unwrap_or_default();
    let issue = Issue {
        status: status.into(),
        claims,
        runs,
        blocks,
        review_heads,
        unanswered,
        depends,
        pr,
        base,
        answered,
        integrity,
    };
    Ok(intents::issue(&issue, world.now))
}

/// One integrity run's header, reduced. A run the check wrote always has these fields; one without them never counts.
fn integrity_run(h: &Map<String, Value>) -> IntegrityRun {
    IntegrityRun {
        id: text(h, "id").unwrap_or_default(),
        owner: text(h, "under").unwrap_or_default(),
        check: text(h, "check").unwrap_or_default(),
        at: text(h, "at").and_then(|t| intents::timestamp(&t)),
        attempt: h.get("attempt").and_then(Value::as_u64).unwrap_or_default(),
        error: text(h, "severity").as_deref() == Some("error"),
        verdict: text(h, "verdict").unwrap_or_default(),
        names: h
            .get("names")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("projects/bp/issues/0001.a.md");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "---\nkind: issue\ntitle: A\nstatus: open\nunder: projects/bp/milestones/m\n---\n")
            .unwrap();
        std::fs::create_dir_all(dir.path().join("shared")).unwrap();
        let vault = Vault { root: dir.path().to_path_buf() };
        (dir, vault)
    }

    #[test]
    fn a_live_issue_the_index_lacks_refuses_no_node() {
        let (_dir, vault) = vault();
        let index = Index::open(&vault).unwrap();
        let projects = BTreeSet::from(["bp"]);
        let intents = Intents::new(&vault, &index, Some(&projects)).unwrap();
        let refusal = intents.row(Kind::Issue, "projects/bp/issues/9999.none", Some("open")).unwrap_err();
        assert_eq!(refusal.code, Code::NoNode);
    }

    #[test]
    fn a_live_issue_with_no_world_refuses_instead_of_panicking() {
        let (_dir, vault) = vault();
        let index = Index::open(&vault).unwrap();
        let intents = Intents::new(&vault, &index, None).unwrap();
        let refusal = intents.row(Kind::Issue, "projects/bp/issues/0001.a", Some("open")).unwrap_err();
        assert_eq!(refusal.code, Code::CouldNotRun);
    }
}
