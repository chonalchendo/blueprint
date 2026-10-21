//! Composes the `Intents` function's inputs from the index and the forge snapshot. The function itself is pure
//! and lives in `bp-change`; this is the IO around it (23, rule 2).

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bp_change::intents::{self, Base, Block, Claim, Dependency, IntegrityRun, Issue, PullRequest, Verdict};
use bp_forge::Snapshot;
use bp_reasoning::LearningStatus;
use bp_vault::{Body, DeltaState, DesignLevel, Id, Index, Kind, Record, Refusal, Vault};
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

/// What one call reads to compute intents: the index, the world when an issue is in view, and each project's
/// counting integrity run, read once per project (24, call budget).
pub struct Intents<'a> {
    vault: &'a Vault,
    index: &'a Index,
    /// The clock and the forge, read only when an issue is in view.
    pub world: Option<World>,
    integrity: RefCell<BTreeMap<String, Option<IntegrityRun>>>,
    /// Each design's latest acceptance, read once per call for `stale`.
    accepted: RefCell<BTreeMap<String, Option<String>>>,
    /// Each learning's creation, read once per call for `stale` with no evidence row: learnings share one directory.
    created: OnceCell<BTreeMap<String, String>>,
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
            Kind::Milestone => Ok(intents::milestone(status, self.integrity(id)?)),
            Kind::Design => {
                let uncited: Vec<String> = drift(self.index, id)?.into_iter().map(|r| r.id).collect();
                Ok(intents::design(id, &uncited))
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
            let dir = id.rsplit_once('/').map_or("", |(d, _)| d);
            self.created.get_or_init(|| self.vault.created(dir)).get(id).cloned()
        };
        let last = self.index.records(id, Kind::Evidence)?.pop().map_or_else(created, |r| text(&r.header, "at"));
        Ok(LearningStatus::stale(&text(header, "status").unwrap_or_default(), last.as_deref(), &changed))
    }

    /// The blocked note of the project's counting integrity run, when it failed and names `id` (21, blocked).
    fn integrity(&self, id: &str) -> Result<Option<String>, Refusal> {
        let Some(project) = Id::parse(id).ok().and_then(|i| i.project().map(str::to_string)) else { return Ok(None) };
        let mut cache = self.integrity.borrow_mut();
        if !cache.contains_key(&project) {
            let runs: Vec<IntegrityRun> = self.index.integrity_runs(&project)?.iter().map(integrity_run).collect();
            cache.insert(project.clone(), IntegrityRun::counting(&runs).cloned());
        }
        Ok(cache[&project].as_ref().and_then(|r| r.blocks(id)))
    }
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
    let review_heads = index.records(id, Kind::Review)?.iter().filter_map(|r| text(&r.header, "head")).collect();
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
    let issue =
        Issue { status: status.into(), claims, runs, blocks, review_heads, depends, pr, base, answered, integrity };
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
