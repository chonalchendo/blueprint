//! Composes the `Intents` function's inputs from the index and the forge snapshot. The function itself is pure
//! and lives in `bp-change`; this is the IO around it (23, rule 2).

use bp_change::intents::{self, Claim, Dependency, Issue, PullRequest, Verdict};
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
        let now = now()?;
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
}

/// The clock in Unix seconds: `BP_NOW` when set, so parity runs and replays see the clock the reference saw.
pub fn now() -> Result<i64, Refusal> {
    if let Ok(s) = std::env::var("BP_NOW") {
        return intents::timestamp(&s)
            .ok_or_else(|| Refusal::usage(format!("BP_NOW={s}"), "set BP_NOW to YYYY-MM-DDTHH:MM:SSZ, or unset it"));
    }
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    Ok(i64::try_from(since.expect("the clock is past 1970").as_secs()).expect("the clock fits i64"))
}

/// The intent of one issue or milestone. A milestone's intent reads only its status, so the clock and the forge
/// are read, and `world` given, only when an issue is in view.
pub fn row(
    vault: &Vault,
    index: &Index,
    world: Option<&World>,
    kind: Kind,
    id: &str,
    status: Option<&str>,
) -> Result<Verdict, Refusal> {
    let status = status.unwrap_or_default();
    match kind {
        Kind::Issue => {
            of(vault, index, world.expect("the caller reads the world when an issue is in view"), id, status)
        }
        _ => Ok(intents::milestone(status)),
    }
}

/// The intent of one issue.
fn of(vault: &Vault, index: &Index, world: &World, id: &str, status: &str) -> Result<Verdict, Refusal> {
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
    let claims: Vec<Claim> = index
        .records(id, Kind::Claim)?
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
    let runs = index
        .records(id, Kind::Run)?
        .iter()
        .map(|r| intents::Run {
            check: text(&r.header, "check").unwrap_or_default(),
            at: text(&r.header, "at").and_then(|t| intents::timestamp(&t)),
            commit: text(&r.header, "commit").unwrap_or_default(),
            failed: matches!(text(&r.header, "verdict").as_deref(), Some("fail" | "timed-out")),
        })
        .collect();
    let review_heads = index.records(id, Kind::Review)?.iter().filter_map(|r| text(&r.header, "head")).collect();
    let pr = world.pr(id).map(|p| PullRequest {
        head: p.head_ref_oid.clone(),
        green: p.is_green(),
        approved: p.is_approved(&p.head_ref_oid),
    });
    // Git is asked only when an escalation needs the answer: one process per escalated issue, none otherwise.
    let changed = match intents::latest_claim(&claims) {
        Some(c) if c.escalated => Id::parse(id).ok().and_then(|id| vault.last_commit(&id)),
        _ => None,
    };
    let issue = Issue { status: status.into(), claims, runs, review_heads, depends, pr, changed };
    Ok(intents::issue(&issue, world.now))
}

fn text(m: &Map<String, Value>, k: &str) -> Option<String> {
    m.get(k).and_then(Value::as_str).map(str::to_string)
}
