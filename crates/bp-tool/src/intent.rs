//! Composes the `Intents` function's inputs from the index and the forge snapshot. The function itself is pure
//! and lives in `bp-change`; this is the IO around it (23, rule 2).

use std::cell::RefCell;
use std::collections::BTreeMap;

use bp_change::intents::{self, Block, Claim, Dependency, IntegrityRun, Issue, PullRequest, Verdict};
use bp_forge::Snapshot;
use bp_vault::{Id, Index, Kind, Refusal, Vault};
use serde_json::{Map, Value};

/// The clock and the forge one call reads.
pub struct World {
    /// Unix seconds. `BP_NOW` fixes it for parity runs and replays, which must see the clock the reference saw.
    pub now: i64,
    /// `None` when the forge was not read.
    pub forge: Option<Snapshot>,
}

impl World {
    /// Reads `BP_NOW` and `BP_FORGE_SNAPSHOT`. The live forge read lands with the repo verbs.
    pub fn read() -> Result<Self, Refusal> {
        let now = crate::clock::now()?;
        let forge = match std::env::var_os("BP_FORGE_SNAPSHOT") {
            Some(dir) => Some(Snapshot::from_dir(std::path::Path::new(&dir)).map_err(|(path, e)| {
                Refusal::failed(
                    path.display().to_string(),
                    format!("{e}; point BP_FORGE_SNAPSHOT at a directory of gh JSON, or unset it"),
                )
            })?),
            None => None,
        };
        Ok(Self { now, forge })
    }

    fn pr(&self, id: &str) -> Option<&bp_forge::PullRequest> {
        self.forge.as_ref()?.open_for(id.rsplit('/').next()?)
    }

    fn merged(&self, id: &str) -> Option<&bp_forge::PullRequest> {
        self.forge.as_ref()?.merged_for(id.rsplit('/').next()?)
    }
}

/// What one call reads to compute intents: the index, the world when an issue is in view, and each project's
/// counting integrity run, read once per project (24, call budget).
pub struct Intents<'a> {
    vault: &'a Vault,
    index: &'a Index,
    /// The clock and the forge, read only when an issue is in view.
    pub world: Option<World>,
    integrity: RefCell<BTreeMap<String, Option<IntegrityRun>>>,
}

impl<'a> Intents<'a> {
    /// Reads the world only when `issues` are in view: a milestone's intent needs neither the clock nor the forge.
    pub fn new(vault: &'a Vault, index: &'a Index, issues: bool) -> Result<Self, Refusal> {
        let world = if issues { Some(World::read()?) } else { None };
        Ok(Self { vault, index, world, integrity: RefCell::default() })
    }

    /// The intent of one issue or milestone.
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
            _ => Ok(intents::milestone(status, self.integrity(id)?)),
        }
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
        let target = index.header(d)?;
        depends.push(Dependency {
            id: d.to_string(),
            status: target.as_ref().map(|h| text(h, "status").unwrap_or_default()),
            open_pr: world.pr(d).is_some(),
        });
    }
    let records = index.records(id, Kind::Claim)?;
    let claims: Vec<Claim> = records
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
        .collect();
    let runs = runs(index, id)?;
    let review_heads = index.records(id, Kind::Review)?.iter().filter_map(|r| text(&r.header, "head")).collect();
    // A merged pull request under a held claim is a land that stopped before its commit: `land` finishes it, through
    // the same gates.
    let held = claims.iter().any(|c| c.released.is_none());
    let open = world.pr(id).or_else(|| world.merged(id).filter(|_| held));
    // The body and the reviewer matter only to needs landing, which needs a pull request.
    let (pr, blocks) = match open {
        Some(p) => (Some(pull_request(p, &reviewer(index, id)?)), blocks(vault, id)?),
        None => (None, Vec::new()),
    };
    // Git is asked only when an escalation needs the answer: one process per escalated issue, none otherwise.
    let answered = intents::latest_claim(&claims).filter(|c| c.escalated).is_some_and(|latest| {
        let record = &records[claims.iter().position(|c| std::ptr::eq(c, latest)).expect("the latest is one of them")];
        Id::parse(id).ok().zip(Id::parse(&record.id).ok()).is_some_and(|(id, claim)| vault.touched_after(&id, &claim))
    });
    let issue = Issue { status: status.into(), claims, runs, blocks, review_heads, depends, pr, answered, integrity };
    Ok(intents::issue(&issue, world.now))
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

/// The forge's pull request as the predicates read it: its head, green, and the reviewer's approval at the head.
pub(crate) fn pull_request(p: &bp_forge::PullRequest, reviewer: &str) -> PullRequest {
    PullRequest {
        head: p.head_ref_oid.clone(),
        green: p.is_green(),
        approved: p.is_approved(&p.head_ref_oid, reviewer),
    }
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
