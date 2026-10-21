//! Composes the `Intents` function's inputs from the index and the forge snapshot. The function itself is pure
//! and lives in `bp-change`; this is the IO around it (23, rule 2).

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bp_change::MilestoneStatus;
use bp_change::intents::{self, Base, Block, Claim, Dependency, IntegrityRun, Issue, Milestone, PullRequest, Verdict};
use bp_design::DesignStatus;
use bp_forge::Snapshot;
use bp_reasoning::LearningStatus;
use bp_vault::{Body, DeltaState, DesignLevel, Filter, Id, Index, Kind, Record, Refusal, Vault};
use serde_json::{Map, Value};

/// The clock and the forge one call reads. The forge is a snapshot and never a live call (22, constraint 8).
pub struct World {
    /// Unix seconds. `BP_NOW` fixes it for parity runs and replays, which must see the clock the reference saw.
    pub now: i64,
    /// `BP_FORGE_SNAPSHOT`'s one snapshot, for every project.
    over: Option<Snapshot>,
    /// The snapshot of each project in view under the vault's `.bp/forge/`, as the repo verbs wrote it (27, design
    /// change 9).
    projects: Vec<(String, Snapshot)>,
}

impl World {
    /// Reads `BP_NOW`, then `BP_FORGE_SNAPSHOT` or the snapshot of each of `projects` in the vault at `root`.
    pub fn read(root: &Path, projects: &BTreeSet<&str>) -> Result<Self, Refusal> {
        let now = crate::clock::now()?;
        if let Some(dir) = std::env::var_os("BP_FORGE_SNAPSHOT") {
            let over = Snapshot::from_dir(Path::new(&dir)).map_err(|(path, e)| {
                Refusal::failed(
                    path.display().to_string(),
                    format!("{e}; point BP_FORGE_SNAPSHOT at a directory of gh JSON, or unset it"),
                )
            })?;
            return Ok(Self { now, over: Some(over), projects: Vec::new() });
        }
        Ok(Self { now, over: None, projects: crate::snapshot::read(root, projects)? })
    }

    /// The snapshot of the project, if one was read.
    fn snapshot(&self, project: &str) -> Option<&Snapshot> {
        self.over.as_ref().or_else(|| self.projects.iter().find(|(p, _)| p == project).map(|(_, s)| s))
    }

    /// The seconds since the oldest forge read in view; `None` when no snapshot was read. A `BP_NOW` before the
    /// snapshot reads as 0.
    pub fn forge_age(&self) -> Option<i64> {
        let oldest = self.over.iter().chain(self.projects.iter().map(|(_, s)| s)).filter_map(|s| s.oldest).min()?;
        let at = i64::try_from(oldest.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
        Some((self.now - at).max(0))
    }

    pub(crate) fn pr(&self, id: &str) -> Option<&bp_forge::PullRequest> {
        let (project, slug) = split(id)?;
        self.snapshot(project)?.open_for(slug)
    }

    fn merged(&self, id: &str) -> Option<&bp_forge::PullRequest> {
        let (project, slug) = split(id)?;
        self.snapshot(project)?.merged_for(slug)
    }
}

/// An issue id's project and slug: `projects/<project>/issues/<slug>`.
pub(crate) fn split(id: &str) -> Option<(&str, &str)> {
    let mut parts = id.split('/');
    Some((parts.nth(1)?, parts.next_back()?))
}

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
    /// Each project's open concerns, and whether a milestone is `planned` or `approved` (needs shaping).
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
    pub fn row(&self, kind: Kind, id: &str, status: Option<&str>) -> Result<Verdict, Refusal> {
        let status = status.unwrap_or_default();
        match kind {
            Kind::Issue => of(
                self.vault,
                self.index,
                self.world.as_ref().expect("the caller reads the world when an issue is in view"),
                id,
                status,
                self.integrity(id)?,
            ),
            Kind::Milestone => self.milestone(id, status),
            Kind::Design => {
                let uncited: Vec<String> = drift(self.index, id)?.into_iter().map(|r| r.id).collect();
                // Shaping reads the project's concern answers only for an active design that has not drifted.
                let (open, due) = match project(id) {
                    Some(p) if uncited.is_empty() && status == DesignStatus::Active.as_str() => {
                        once(&self.shaping, &p, || shaping(self.index, &p))?
                    }
                    _ => (Vec::new(), false),
                };
                Ok(intents::design(id, &uncited, &open, due))
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
        let counting = once(&self.integrity, &project, || {
            let runs: Vec<IntegrityRun> = self.index.integrity_runs(&project)?.iter().map(integrity_run).collect();
            Ok(IntegrityRun::counting(&runs).cloned())
        })?;
        Ok(counting.and_then(|r| r.blocks(id)))
    }

    /// The intent of one milestone. Each read waits for the status, and the earlier reads, that let it matter.
    fn milestone(&self, id: &str, status: &str) -> Result<Verdict, Refusal> {
        let project = project(id).expect("a milestone sits under a project");
        let mut m = Milestone::default();
        match status.parse() {
            Ok(MilestoneStatus::Approved) => {
                m = derivation(self.index, id)?;
                if !m.issues.iter().any(|s| intents::is_live_issue(s)) {
                    m.runs = runs(self.index, id)?;
                    m.waived = self
                        .index
                        .records(id, Kind::Waiver)?
                        .iter()
                        .map(|r| crate::waiver::reduce(id, r).block)
                        .collect();
                    m.broken = once(&self.broken, &project, || Ok(!self.index.broken(&project)?.is_empty()))?;
                }
            }
            Ok(MilestoneStatus::Done) => {
                // One count per project: the waiver module's read, which needs git, runs only when a waiver exists.
                let open = once(&self.reasked, &project, || {
                    let f = Filter { kind: Kind::Waiver, status: None, project: Some(&project), under: None, limit: 1 };
                    if self.index.list(&f)?.1 == 0 {
                        return Ok(Vec::new());
                    }
                    let open = crate::waiver::open(self.vault, self.index, &project, crate::clock::now()?)?;
                    Ok(open.into_iter().map(|r| (r.milestone.to_string(), r.name)).collect())
                })?;
                m.reasked = open.into_iter().filter(|(of, _)| of == id).map(|(_, block)| block).collect();
            }
            _ => {}
        }
        m.status = status.into();
        m.integrity = self.integrity(id)?;
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

/// The project's open concerns, and whether a milestone of it is `planned` or `approved` (21, needs shaping). The
/// milestones are read only when a concern is open.
fn shaping(index: &Index, project: &str) -> Result<(Vec<String>, bool), Refusal> {
    let answers = index.records(&format!("projects/{project}/project"), Kind::ConcernAnswer)?;
    let answers: Vec<intents::ConcernAnswer> = answers
        .iter()
        .map(|r| intents::ConcernAnswer {
            concern: text(&r.header, "concern").unwrap_or_default(),
            stamp: r.id.rsplit('/').next().unwrap_or_default().to_string(),
            state: text(&r.header, "state").unwrap_or_default(),
        })
        .collect();
    let open = intents::open_concerns(&answers);
    if open.is_empty() {
        return Ok((open, false));
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
    Ok((open, due))
}

/// The issues under the milestone `id`, and while fewer than a batch are live, its finish line and the blocks the
/// issues' checks name: needs derivation's records, which derive's stop reads too (21).
pub(crate) fn derivation(index: &Index, id: &str) -> Result<Milestone, Refusal> {
    let f = Filter { kind: Kind::Issue, status: None, project: None, under: Some(id), limit: crate::ALL };
    let (rows, _) = index.list(&f)?;
    let mut m = Milestone {
        issues: rows.iter().map(|r| r.status.clone().unwrap_or_default()).collect(),
        ..Milestone::default()
    };
    if m.issues.iter().filter(|s| intents::is_live_issue(s)).count() >= intents::BATCH {
        return Ok(m);
    }
    m.finish = named(&index.body(id)?, "Finish line");
    // With no block to cover, the issues' checks say nothing.
    for r in rows.iter().filter(|_| !m.finish.is_empty()) {
        m.covered.extend(named(&index.body(&r.id)?, "Check").into_iter().map(|b| b.name));
    }
    Ok(m)
}

/// The named blocks in the body's sections under `heading`, with their hashes now.
fn named(body: &str, heading: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for s in Body::parse(body).sections_named(heading) {
        let body = Body::parse(&s.text);
        out.extend(
            bp_check::named(&body)
                .into_iter()
                .map(|c| Block { name: c.attrs["name"].clone(), hash: bp_check::hash(c) }),
        );
    }
    out
}

/// The findings of a review that no disposition answers, counted from 1, and whether a disposition is `fixed` (21,
/// respond). The findings are the rows of the body's table after its header and separator lines; a disposition of
/// an index past the last row answers nothing.
pub(crate) fn findings(index: &Index, review: &Record) -> Result<(Vec<u64>, bool), Refusal> {
    let rows = review.body.lines().filter(|l| l.trim_start().starts_with('|')).count().saturating_sub(2);
    let dispositions = index.records(&review.id, Kind::Disposition)?;
    let answered: BTreeSet<u64> = dispositions.iter().filter_map(|d| d.header.get("finding")?.as_u64()).collect();
    let fixed = dispositions.iter().any(|d| text(&d.header, "as").as_deref() == Some("fixed"));
    Ok(((1..=rows as u64).filter(|n| !answered.contains(n)).collect(), fixed))
}

/// The drift runs under a design that no delta or thread cites: the runs of 21's drifted, which the intent, the
/// triage-drift slice and its stop read. The predicate on a run is `bp-change`'s.
pub(crate) fn drift(index: &Index, design: &str) -> Result<Vec<Record>, Refusal> {
    let mut out = Vec::new();
    for r in index.records(design, Kind::Run)? {
        let field = |k: &str| text(&r.header, k).unwrap_or_default();
        if intents::is_drift(&field("verdict"), &field("check_kind"))
            && index.citing(&Id::parse(&r.id).expect("the index holds ids"), &[Kind::Delta, Kind::Thread])?.is_empty()
        {
            out.push(r);
        }
    }
    Ok(out)
}

/// The designs a triage-drift seed covers: the design, or each design of the project (21, triage drift).
pub(crate) fn drift_designs(index: &Index, id: &Id, kind: Kind) -> Result<Vec<String>, Refusal> {
    match id.project() {
        Some(project) if kind == Kind::Project => index.ids_of(Kind::Design, project),
        _ => Ok(vec![id.to_string()]),
    }
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

/// The intent of one issue.
fn of(
    vault: &Vault,
    index: &Index,
    world: &World,
    id: &str,
    status: &str,
    integrity: Option<String>,
) -> Result<Verdict, Refusal> {
    if !intents::is_live_issue(status) {
        return Ok(Verdict::NONE);
    }
    let header = index.header(id)?.expect("a listed row is in the index");
    let mut depends = Vec::new();
    for d in header.get("depends").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let status = index.header(d)?.map(|h| text(&h, "status").unwrap_or_default());
        let open_pr = world.pr(d).is_some();
        // The stack test reads the layers below a target only when `cut` would stack on it.
        let layers =
            if open_pr && status.as_deref() != Some("resolved") { crate::stack::layers(index, d)? } else { Vec::new() };
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
                Some(c) => crate::stack::base(index, c)?,
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

/// The claim records, reduced: the one reading of a release that blocked and `walk --done` share (21, rule 1).
pub(crate) fn claims(records: &[Record]) -> Vec<Claim> {
    records
        .iter()
        .map(|c| {
            let release = intents::release(&c.body);
            let to = release.get("to").filter(|t| !t.is_empty());
            Claim {
                at: text(&c.header, "from").and_then(|t| intents::timestamp(&t)),
                // A `to` that is not a timestamp still releases; it reads as the latest time, so an escalation stays blocked.
                released: to.map(|t| intents::timestamp(t).unwrap_or(i64::MAX)),
                escalated: to.is_some() && release.get("reason").map(String::as_str) == Some("escalated"),
            }
        })
        .collect()
}

/// The issue's run records, reduced.
pub(crate) fn runs(index: &Index, id: &str) -> Result<Vec<intents::Run>, Refusal> {
    Ok(index
        .records(id, Kind::Run)?
        .iter()
        .map(|r| {
            let verdict = text(&r.header, "verdict");
            intents::Run {
                id: r.id.clone(),
                check: text(&r.header, "check").unwrap_or_default(),
                check_hash: text(&r.header, "check_hash").unwrap_or_default(),
                at: text(&r.header, "at").and_then(|t| intents::timestamp(&t)),
                commit: text(&r.header, "commit").unwrap_or_default(),
                attempt: r.header.get("attempt").and_then(Value::as_u64).unwrap_or_default(),
                // A converted run has no severity; a block's default is `error` (15).
                error: text(&r.header, "severity").is_none_or(|s| s != "warn"),
                failed: matches!(verdict.as_deref(), Some("fail" | "timed-out")),
                passed: matches!(verdict.as_deref(), Some("pass" | "skipped")),
            }
        })
        .collect())
}

/// The issue's named `error` blocks: each needs a passing run of its current text at the head before a land.
pub(crate) fn blocks(vault: &Vault, id: &str) -> Result<Vec<Block>, Refusal> {
    let file = vault.read(&Id::parse(id).expect("the index holds ids"))?;
    let named = bp_check::named(&file.body).into_iter().filter(|c| bp_check::is_error(c));
    Ok(named.map(|c| Block { name: c.attrs["name"].clone(), hash: bp_check::hash(c) }).collect())
}

/// The project's reviewer identity, the forge login whose approval alone counts; empty when unset (13, Project).
pub(crate) fn reviewer(index: &Index, id: &str) -> Result<String, Refusal> {
    let project = Id::parse(id).ok().and_then(|i| i.project().map(str::to_string)).unwrap_or_default();
    Ok(index.header(&format!("projects/{project}/project"))?.and_then(|h| text(&h, "reviewer")).unwrap_or_default())
}

/// The forge's pull request as the predicates read it: its head, its base, green, and the reviewer's approval at the
/// head.
pub(crate) fn pull_request(p: &bp_forge::PullRequest, reviewer: &str) -> PullRequest {
    let approved = p.approved_heads(reviewer).contains(&p.head_ref_oid.as_str());
    PullRequest { head: p.head_ref_oid.clone(), base: p.base_ref_name.clone(), green: p.is_green(), approved }
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

fn text(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).and_then(Value::as_str).map(str::to_string)
}
